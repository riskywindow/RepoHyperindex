"""Tests for corpora validation, bootstrap planning, and snapshot metadata."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest
from hyperbench.corpora import (
    BootstrapError,
    SnapshotError,
    bootstrap_repos,
    create_corpus_snapshot,
    validate_phase1_config_dir,
)

REPO_ROOT = Path(__file__).resolve().parents[1]
CONFIG_DIR = REPO_ROOT / "bench" / "configs"


def test_validate_phase1_config_dir_succeeds_with_warnings() -> None:
    report = validate_phase1_config_dir(CONFIG_DIR)
    assert report.is_valid
    assert report.warnings
    assert "pinned_ref" in report.warnings[0]


def test_validate_phase1_config_dir_reports_cross_document_error(tmp_path: Path) -> None:
    temp_config_dir = tmp_path / "configs"
    shutil.copytree(CONFIG_DIR, temp_config_dir)
    broken_golden = (
        temp_config_dir
        / "goldens"
        / "synthetic-saas-medium-semantic-goldens.json"
    )
    broken_golden.write_text(
        broken_golden.read_text(encoding="utf-8").replace(
            '"query_pack_id": "synthetic-saas-medium-semantic-pack"',
            '"query_pack_id": "mismatched-pack"',
        ),
        encoding="utf-8",
    )

    report = validate_phase1_config_dir(temp_config_dir)
    assert not report.is_valid
    assert any(
        "golden sets reference unknown query_pack_id values: mismatched-pack" in err
        for err in report.errors
    )


def test_bootstrap_dry_run_returns_plan_for_selected_repos() -> None:
    plan = bootstrap_repos(CONFIG_DIR, REPO_ROOT / "bench" / "corpora", dry_run=True)
    assert len(plan) == 3
    assert any(entry.repo_id == "svelte-cli" for entry in plan)
    assert any("missing pinned_ref" in entry.notes for entry in plan)


def test_bootstrap_errors_when_pinned_ref_missing() -> None:
    with pytest.raises(BootstrapError) as exc_info:
        bootstrap_repos(CONFIG_DIR, REPO_ROOT / "bench" / "corpora", dry_run=False)

    assert "missing pinned_ref" in str(exc_info.value)


def test_bootstrap_network_unavailable_error_is_actionable(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    config_dir = tmp_path / "configs"
    config_dir.mkdir()
    (config_dir / "repos.yaml").write_text(
        """
schema_version: "1"
generated_for_phase: phase1
selection_note: test
repos:
  - repo_id: test-repo
    status: selected
    owner: example
    name: repo
    repo_url: https://github.com/example/repo
    clone_url: https://github.com/example/repo.git
    rationale: test repo
    expected_tier: S
    tier_verification_status: partial
    why_useful: test usefulness
    license:
      spdx_id: MIT
      verification_status: verified
      source_url: https://github.com/example/repo
    clone_strategy: shallow
    pinned_ref: deadbeef
    pinning_policy: pin this
    source_urls:
      - https://github.com/example/repo
    risks:
      - network access required
    manual_verification:
      - verify tier later
  - repo_id: medium-repo
    status: selected
    owner: example
    name: medium
    repo_url: https://github.com/example/medium
    clone_url: https://github.com/example/medium.git
    rationale: medium repo
    expected_tier: M
    tier_verification_status: partial
    why_useful: test usefulness
    license:
      spdx_id: MIT
      verification_status: verified
      source_url: https://github.com/example/medium
    clone_strategy: shallow
    pinned_ref: deadbeef
    pinning_policy: pin this
    source_urls:
      - https://github.com/example/medium
    risks:
      - network access required
    manual_verification:
      - verify tier later
  - repo_id: large-repo
    status: selected
    owner: example
    name: large
    repo_url: https://github.com/example/large
    clone_url: https://github.com/example/large.git
    rationale: large repo
    expected_tier: L
    tier_verification_status: partial
    why_useful: test usefulness
    license:
      spdx_id: MIT
      verification_status: verified
      source_url: https://github.com/example/large
    clone_strategy: shallow
    pinned_ref: deadbeef
    pinning_policy: pin this
    source_urls:
      - https://github.com/example/large
    risks:
      - network access required
    manual_verification:
      - verify tier later
""",
        encoding="utf-8",
    )

    def fake_run(*args: object, **kwargs: object) -> subprocess.CompletedProcess[str]:
        raise subprocess.CalledProcessError(
            returncode=128,
            cmd=args[0],
            stderr=(
                "fatal: unable to access 'https://github.com/example/repo.git/': "
                "Could not resolve host: github.com"
            ),
        )

    monkeypatch.setattr("hyperbench.corpora.subprocess.run", fake_run)

    with pytest.raises(BootstrapError) as exc_info:
        bootstrap_repos(config_dir, tmp_path / "corpora")

    assert "Network access appears unavailable" in str(exc_info.value)
    assert "--dry-run" in str(exc_info.value)


def test_snapshot_metadata_for_local_git_repo(tmp_path: Path) -> None:
    corpus_dir = tmp_path / "sample-corpus"
    packages_dir = corpus_dir / "packages" / "auth"
    packages_dir.mkdir(parents=True)

    (corpus_dir / "package.json").write_text('{"name":"root"}\n', encoding="utf-8")
    (packages_dir / "package.json").write_text('{"name":"auth"}\n', encoding="utf-8")
    (packages_dir / "index.ts").write_text(
        "export function invalidateSession() {\n  return true;\n}\n",
        encoding="utf-8",
    )
    (corpus_dir / "web.js").write_text("export const value = 1;\n", encoding="utf-8")

    subprocess.run(["git", "init", str(corpus_dir)], check=True, capture_output=True, text=True)
    subprocess.run(
        ["git", "-C", str(corpus_dir), "config", "user.email", "test@example.com"],
        check=True,
        capture_output=True,
        text=True,
    )
    subprocess.run(
        ["git", "-C", str(corpus_dir), "config", "user.name", "Hyperbench Test"],
        check=True,
        capture_output=True,
        text=True,
    )
    subprocess.run(
        ["git", "-C", str(corpus_dir), "add", "."],
        check=True,
        capture_output=True,
        text=True,
    )
    subprocess.run(
        ["git", "-C", str(corpus_dir), "commit", "-m", "fixture"],
        check=True,
        capture_output=True,
        text=True,
    )

    manifest_path = tmp_path / "fixture-manifest.yaml"
    manifest_path.write_text("name: fixture\n", encoding="utf-8")

    snapshot = create_corpus_snapshot(corpus_dir, manifest_path=manifest_path)
    assert len(snapshot.commit_sha or "") >= 7
    assert snapshot.file_count >= 3
    assert snapshot.loc == 4
    assert snapshot.package_count == 2
    assert snapshot.manifest_hash


def test_snapshot_requires_manifest_source(tmp_path: Path) -> None:
    source_dir = tmp_path / "fixture"
    source_dir.mkdir()

    with pytest.raises(SnapshotError) as exc_info:
        create_corpus_snapshot(source_dir)

    assert "requires either --manifest-path or --repo-id" in str(exc_info.value)
