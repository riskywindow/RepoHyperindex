"""Manifest loading, bootstrap, and snapshot helpers for Hyperbench corpora."""

from __future__ import annotations

import json
import os
import subprocess
from dataclasses import dataclass
from datetime import UTC, datetime
from hashlib import sha256
from pathlib import Path

from pydantic import ValidationError

from hyperbench.query_packs import (
    GOLDEN_SET_DIR,
    QUERY_PACK_DIR,
    SYNTHETIC_TARGET_COUNTS,
    load_query_artifacts,
    validate_query_artifacts,
)
from hyperbench.schemas import (
    BenchmarkHardwareCatalog,
    CorpusManifest,
    CorpusSnapshot,
    GoldenSet,
    QueryPack,
    RealRepoCatalog,
    RealRepoSelection,
    SyntheticCorpusConfig,
)

DEFAULT_CONFIG_DIR = Path("bench/configs")
DEFAULT_CORPORA_DIR = Path("bench/corpora")
CODE_FILE_SUFFIXES = {".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"}
IGNORED_DIRECTORIES = {".git", "node_modules", "__pycache__", ".venv", ".pytest_cache"}
NETWORK_ERROR_MARKERS = (
    "could not resolve host",
    "failed to connect",
    "network is unreachable",
    "timed out",
    "couldn't connect",
    "unable to access",
)


class CorporaError(RuntimeError):
    """Base error for corpora operations."""


class ManifestValidationError(CorporaError):
    """Raised when config validation fails."""


class BootstrapError(CorporaError):
    """Raised when bootstrap planning or execution fails."""


class SnapshotError(CorporaError):
    """Raised when snapshot metadata cannot be generated."""


@dataclass(frozen=True)
class Phase1ConfigContext:
    """Loaded Phase 1 config documents used for corpora validation."""

    repo_catalog: RealRepoCatalog
    hardware_catalog: BenchmarkHardwareCatalog
    synthetic_config: SyntheticCorpusConfig
    synthetic_manifest: CorpusManifest
    query_packs: list[QueryPack]
    golden_sets: list[GoldenSet]


@dataclass(frozen=True)
class ValidationReport:
    """Structured validation result for config validation commands."""

    errors: list[str]
    warnings: list[str]

    @property
    def is_valid(self) -> bool:
        return not self.errors


@dataclass(frozen=True)
class BootstrapPlanEntry:
    """Planned or executed action for a managed corpus repo."""

    repo_id: str
    action: str
    destination: Path
    pinned_ref: str | None
    notes: list[str]


def load_repo_catalog(config_dir: Path | str = DEFAULT_CONFIG_DIR) -> RealRepoCatalog:
    """Load the real-repo selection catalog from the given config directory."""
    config_path = Path(config_dir) / "repos.yaml"
    return RealRepoCatalog.from_path(config_path)


def load_phase1_config_context(config_dir: Path | str = DEFAULT_CONFIG_DIR) -> Phase1ConfigContext:
    """Load the Phase 1 config documents relevant to corpora validation."""
    config_root = Path(config_dir)
    query_pack_dir = config_root / QUERY_PACK_DIR.name
    golden_set_dir = config_root / GOLDEN_SET_DIR.name
    query_packs, golden_sets = load_query_artifacts(query_pack_dir, golden_set_dir)
    return Phase1ConfigContext(
        repo_catalog=RealRepoCatalog.from_path(config_root / "repos.yaml"),
        hardware_catalog=BenchmarkHardwareCatalog.from_path(config_root / "hardware-targets.yaml"),
        synthetic_config=SyntheticCorpusConfig.from_path(config_root / "synthetic-corpus.yaml"),
        synthetic_manifest=CorpusManifest.from_path(config_root / "corpus-manifest.synthetic.yaml"),
        query_packs=query_packs,
        golden_sets=golden_sets,
    )


def validate_phase1_config_dir(config_dir: Path | str = DEFAULT_CONFIG_DIR) -> ValidationReport:
    """Validate corpora-related manifests and their cross-document references."""
    errors: list[str] = []
    warnings: list[str] = []
    try:
        context = load_phase1_config_context(config_dir)
    except ValidationError as exc:
        raise ManifestValidationError(str(exc)) from exc

    manifest = context.synthetic_manifest
    synthetic = context.synthetic_config
    hardware_ids = {target.target_id for target in context.hardware_catalog.hardware_targets}

    if manifest.synthetic_config_id != synthetic.config_id:
        errors.append(
            "corpus-manifest.synthetic.yaml synthetic_config_id must match "
            "synthetic-corpus.yaml config_id"
        )
    if manifest.corpus_id != synthetic.corpus_id:
        errors.append(
            "synthetic corpus manifest corpus_id must match "
            "synthetic-corpus.yaml corpus_id"
        )
    try:
        validate_query_artifacts(
            context.query_packs,
            context.golden_sets,
            synthetic_corpus_ids={synthetic.corpus_id},
            required_synthetic_counts=SYNTHETIC_TARGET_COUNTS,
        )
    except ValueError as exc:
        errors.extend(str(exc).splitlines())

    synthetic_pack_ids = sorted(
        pack.query_pack_id for pack in context.query_packs if pack.corpus_id == manifest.corpus_id
    )
    synthetic_golden_ids = sorted(
        golden.golden_set_id
        for golden in context.golden_sets
        if golden.corpus_id == manifest.corpus_id
    )
    if sorted(manifest.query_pack_ids) != synthetic_pack_ids:
        errors.append(
            "corpus-manifest.synthetic.yaml query_pack_ids must match the synthetic "
            "query pack IDs under bench/configs/query-packs"
        )
    if sorted(manifest.golden_set_ids) != synthetic_golden_ids:
        errors.append(
            "corpus-manifest.synthetic.yaml golden_set_ids must match the synthetic "
            "golden set IDs under bench/configs/goldens"
        )

    missing_hardware_ids = sorted(set(manifest.hardware_target_ids) - hardware_ids)
    if missing_hardware_ids:
        errors.append(
            "corpus-manifest.synthetic.yaml references unknown hardware_target_ids: "
            + ", ".join(missing_hardware_ids)
        )

    for repo in context.repo_catalog.repos:
        if repo.status.value == "selected" and repo.pinned_ref is None:
            warnings.append(
                f"repos.yaml entry '{repo.repo_id}' has no pinned_ref yet; "
                "bootstrap dry-run is available, but real bootstrap requires "
                "a pinned commit or tag."
            )

    return ValidationReport(errors=errors, warnings=warnings)


def bootstrap_repos(
    config_dir: Path | str = DEFAULT_CONFIG_DIR,
    corpora_dir: Path | str = DEFAULT_CORPORA_DIR,
    *,
    dry_run: bool = False,
    repo_ids: list[str] | None = None,
) -> list[BootstrapPlanEntry]:
    """Clone or update selected repos into the managed corpora directory."""
    catalog = load_repo_catalog(config_dir)
    target_dir = Path(corpora_dir)
    selected = _select_repos(catalog.repos, repo_ids)
    if not selected:
        raise BootstrapError("No repos matched the requested bootstrap selection.")

    plan: list[BootstrapPlanEntry] = []
    for repo in selected:
        destination = target_dir / repo.repo_id
        action = "update" if (destination / ".git").exists() else "clone"
        notes: list[str] = []
        if repo.pinned_ref is None:
            notes.append("missing pinned_ref")
            if not dry_run:
                raise BootstrapError(
                    f"Repo '{repo.repo_id}' is missing pinned_ref in bench/configs/repos.yaml. "
                    "Add a pinned commit SHA or tag before running bootstrap, or use --dry-run "
                    "to inspect the plan locally."
                )
        entry = BootstrapPlanEntry(
            repo_id=repo.repo_id,
            action=action,
            destination=destination,
            pinned_ref=repo.pinned_ref,
            notes=notes,
        )
        plan.append(entry)
        if dry_run:
            continue
        _materialize_repo(repo, destination)
    return plan


def create_corpus_snapshot(
    source_path: Path | str,
    *,
    manifest_path: Path | str | None = None,
    repo_id: str | None = None,
    config_dir: Path | str = DEFAULT_CONFIG_DIR,
) -> CorpusSnapshot:
    """Build snapshot metadata for a local corpus checkout or fixture path."""
    path = Path(source_path).resolve()
    if not path.exists():
        raise SnapshotError(f"Snapshot source path does not exist: {path}")
    if not path.is_dir():
        raise SnapshotError(f"Snapshot source path must be a directory: {path}")

    manifest_hash = _resolve_manifest_hash(
        manifest_path=manifest_path,
        repo_id=repo_id,
        config_dir=config_dir,
    )
    warnings: list[str] = []
    notes: list[str] = []
    commit_sha = _git_head_sha(path)
    if commit_sha is None:
        warnings.append("No git HEAD could be resolved for the source path; commit_sha is unset.")

    file_count, loc, package_count = _scan_corpus_tree(path)
    if package_count is None:
        warnings.append("No package.json files were found, so package_count is unset.")
    else:
        notes.append("package_count derived from package.json file count.")

    if loc == 0:
        warnings.append("No TypeScript/JavaScript LOC were found under the snapshot path.")

    return CorpusSnapshot(
        source_path=str(path),
        manifest_hash=manifest_hash,
        recorded_at=datetime.now(UTC),
        commit_sha=commit_sha,
        file_count=file_count,
        loc=loc,
        package_count=package_count,
        notes=notes,
        warnings=warnings,
    )


def _select_repos(
    repos: list[RealRepoSelection],
    repo_ids: list[str] | None,
) -> list[RealRepoSelection]:
    selected = [repo for repo in repos if repo.status.value == "selected"]
    if not repo_ids:
        return selected
    requested = set(repo_ids)
    return [repo for repo in selected if repo.repo_id in requested]


def _materialize_repo(repo: RealRepoSelection, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists() and not (destination / ".git").exists():
        raise BootstrapError(
            f"Bootstrap destination exists but is not a git repository: {destination}. "
            "Remove or relocate it before re-running bootstrap."
        )

    if (destination / ".git").exists():
        _fetch_pinned_ref(repo, destination)
    else:
        _clone_repo(repo, destination)
        _fetch_pinned_ref(repo, destination)

    _run_git(
        ["git", "-C", str(destination), "checkout", "--detach", "FETCH_HEAD"],
        repo.repo_id,
    )


def _clone_repo(repo: RealRepoSelection, destination: Path) -> None:
    command = ["git", "clone"]
    if repo.clone_strategy.value == "filter_blobless":
        command.extend(["--filter=blob:none"])
    elif repo.clone_strategy.value == "shallow":
        command.extend(["--depth", "1"])
    command.extend([str(repo.clone_url), str(destination)])
    _run_git(command, repo.repo_id)


def _fetch_pinned_ref(repo: RealRepoSelection, destination: Path) -> None:
    if repo.pinned_ref is None:
        raise BootstrapError(f"Repo '{repo.repo_id}' cannot be fetched without pinned_ref.")

    command = ["git", "-C", str(destination), "fetch", "origin"]
    if repo.clone_strategy.value == "shallow":
        command.extend(["--depth", "1"])
    command.append(repo.pinned_ref)
    _run_git(command, repo.repo_id)


def _run_git(command: list[str], repo_id: str) -> str:
    try:
        completed = subprocess.run(
            command,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as exc:
        raise BootstrapError(
            "git is required for corpora bootstrap but was not found on PATH."
        ) from exc
    except subprocess.CalledProcessError as exc:
        combined_output = "\n".join(part for part in (exc.stdout, exc.stderr) if part).strip()
        lower_output = combined_output.lower()
        if any(marker in lower_output for marker in NETWORK_ERROR_MARKERS):
            raise BootstrapError(
                f"Network access appears unavailable while bootstrapping repo '{repo_id}'. "
                "Retry when git network access is available, or re-run with --dry-run to "
                f"inspect the bootstrap plan without cloning.\nGit output:\n{combined_output}"
            ) from exc
        raise BootstrapError(
            f"Git command failed while bootstrapping repo '{repo_id}': {' '.join(command)}\n"
            f"{combined_output}"
        ) from exc
    return completed.stdout.strip()


def _resolve_manifest_hash(
    *,
    manifest_path: Path | str | None,
    repo_id: str | None,
    config_dir: Path | str,
) -> str:
    if manifest_path is not None:
        path = Path(manifest_path)
        if not path.exists():
            raise SnapshotError(f"Manifest path does not exist: {path}")
        return sha256(path.read_bytes()).hexdigest()
    if repo_id is None:
        raise SnapshotError(
            "snapshot requires either --manifest-path or --repo-id "
            "to compute manifest_hash"
        )

    catalog = load_repo_catalog(config_dir)
    for repo in catalog.repos:
        if repo.repo_id == repo_id:
            payload = json.dumps(repo.model_dump(mode="json"), sort_keys=True).encode("utf-8")
            return sha256(payload).hexdigest()
    raise SnapshotError(f"Repo id '{repo_id}' was not found in {Path(config_dir) / 'repos.yaml'}")


def _git_head_sha(path: Path) -> str | None:
    try:
        completed = subprocess.run(
            ["git", "-C", str(path), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
    return completed.stdout.strip() or None


def _scan_corpus_tree(path: Path) -> tuple[int, int, int | None]:
    file_count = 0
    loc = 0
    package_count = 0

    for root, dirs, files in os.walk(path):
        dirs[:] = [name for name in dirs if name not in IGNORED_DIRECTORIES]
        for file_name in files:
            file_path = Path(root) / file_name
            file_count += 1
            if file_name == "package.json":
                package_count += 1
            if file_path.suffix.lower() in CODE_FILE_SUFFIXES:
                loc += _count_non_empty_lines(file_path)

    return file_count, loc, (package_count or None)


def _count_non_empty_lines(path: Path) -> int:
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return 0
    return sum(1 for line in text.splitlines() if line.strip())
