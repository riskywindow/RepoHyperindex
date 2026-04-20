"""Engine adapter boundary for the Phase 1 Hyperbench harness."""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Protocol

from hyperbench.schemas import (
    CorpusManifest,
    ExactQuery,
    GoldenExpectation,
    GoldenSet,
    ImpactQuery,
    QueryPack,
    QueryType,
    SemanticQuery,
    SymbolQuery,
    SymbolScope,
)


class AdapterError(RuntimeError):
    """Raised when an adapter cannot complete the requested harness operation."""


@dataclass(frozen=True)
class CorpusBundle:
    """Loaded benchmark corpus bundle used by adapters and the runner."""

    root_dir: Path
    manifest: CorpusManifest
    query_packs: list[QueryPack]
    golden_sets_by_pack_id: dict[str, GoldenSet]
    expectations_by_query_id: dict[str, GoldenExpectation]
    ground_truth: dict[str, object]
    edit_scenarios: list[dict[str, object]]


@dataclass(frozen=True)
class PreparedCorpus:
    """Result of adapter corpus preparation."""

    corpus_id: str
    latency_ms: float
    notes: list[str] = field(default_factory=list)
    metadata: dict[str, object] = field(default_factory=dict)
    metric_rows: list[dict[str, object]] = field(default_factory=list)


@dataclass(frozen=True)
class QueryHit:
    """Normalized query hit emitted by an adapter."""

    path: str
    symbol: str | None
    rank: int
    reason: str
    score: float


@dataclass(frozen=True)
class QueryExecutionResult:
    """Normalized adapter response for a single query."""

    query_id: str
    query_type: QueryType
    latency_ms: float
    hits: list[QueryHit]
    notes: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class RefreshExecutionResult:
    """Normalized adapter response for one incremental refresh scenario."""

    scenario_id: str
    latency_ms: float
    changed_queries: list[str]
    notes: list[str] = field(default_factory=list)
    metadata: dict[str, object] = field(default_factory=dict)
    metric_rows: list[dict[str, object]] = field(default_factory=list)


class EngineAdapter(Protocol):
    """Protocol that future adapters, including the Rust engine, can implement."""

    name: str

    def prepare_corpus(self, bundle: CorpusBundle) -> PreparedCorpus: ...

    def execute_exact_query(
        self,
        bundle: CorpusBundle,
        query: ExactQuery,
    ) -> QueryExecutionResult: ...

    def execute_symbol_query(
        self,
        bundle: CorpusBundle,
        query: SymbolQuery,
    ) -> QueryExecutionResult: ...

    def execute_semantic_query(
        self,
        bundle: CorpusBundle,
        query: SemanticQuery,
    ) -> QueryExecutionResult: ...

    def execute_impact_query(
        self,
        bundle: CorpusBundle,
        query: ImpactQuery,
    ) -> QueryExecutionResult: ...

    def run_incremental_refresh(
        self,
        bundle: CorpusBundle,
        scenario: dict[str, object],
    ) -> RefreshExecutionResult: ...

    def close(self) -> None: ...


class FixtureAdapter:
    """Adapter that answers queries directly from synthetic goldens and fixtures."""

    name = "fixture"

    def prepare_corpus(self, bundle: CorpusBundle) -> PreparedCorpus:
        repo_dir = bundle.root_dir / "repo"
        notes = [
            "FixtureAdapter uses checked-in or generated fixture artifacts only.",
            f"repo_dir={repo_dir}",
        ]
        return PreparedCorpus(corpus_id=bundle.manifest.corpus_id, latency_ms=4.0, notes=notes)

    def execute_exact_query(
        self,
        bundle: CorpusBundle,
        query: ExactQuery,
    ) -> QueryExecutionResult:
        return self._result_from_expectation(bundle, query.query_id, QueryType.EXACT)

    def execute_symbol_query(
        self,
        bundle: CorpusBundle,
        query: SymbolQuery,
    ) -> QueryExecutionResult:
        return self._result_from_expectation(bundle, query.query_id, QueryType.SYMBOL)

    def execute_semantic_query(
        self,
        bundle: CorpusBundle,
        query: SemanticQuery,
    ) -> QueryExecutionResult:
        return self._result_from_expectation(bundle, query.query_id, QueryType.SEMANTIC)

    def execute_impact_query(
        self,
        bundle: CorpusBundle,
        query: ImpactQuery,
    ) -> QueryExecutionResult:
        return self._result_from_expectation(bundle, query.query_id, QueryType.IMPACT)

    def run_incremental_refresh(
        self,
        bundle: CorpusBundle,
        scenario: dict[str, object],
    ) -> RefreshExecutionResult:
        scenario_id = _coerce_str(scenario.get("scenario_id"), "scenario_id")
        changed_queries = [
            _coerce_str(query_id, "expected_changed_queries[]")
            for query_id in scenario.get("expected_changed_queries", [])
        ]
        latency_ms = 2.0 + (sum(ord(char) for char in scenario_id) % 5)
        return RefreshExecutionResult(
            scenario_id=scenario_id,
            latency_ms=float(latency_ms),
            changed_queries=changed_queries,
            notes=["Derived from synthetic edit_scenarios.json fixture metadata."],
        )

    def close(self) -> None:
        return None

    def _result_from_expectation(
        self,
        bundle: CorpusBundle,
        query_id: str,
        query_type: QueryType,
    ) -> QueryExecutionResult:
        expectation = bundle.expectations_by_query_id.get(query_id)
        if expectation is None:
            raise AdapterError(
                f"FixtureAdapter could not find a golden expectation for query '{query_id}'."
            )
        hits = [
            QueryHit(
                path=expected_hit.path,
                symbol=expected_hit.symbol,
                rank=index,
                reason=expected_hit.reason.value,
                score=max(0.1, 1.0 - (index - 1) * 0.1),
            )
            for index, expected_hit in enumerate(expectation.expected_hits, start=1)
        ]
        latency_ms = _deterministic_latency_ms(query_id, query_type)
        notes = ["Generated directly from typed golden expectations."]
        if expectation.notes:
            notes.append(expectation.notes)
        return QueryExecutionResult(
            query_id=query_id,
            query_type=query_type,
            latency_ms=latency_ms,
            hits=hits,
            notes=notes,
        )


class ShellAdapter:
    """Placeholder adapter boundary for a future Rust engine binary."""

    name = "shell"

    def __init__(self, engine_bin: str | None = None) -> None:
        self.engine_bin = engine_bin

    def prepare_corpus(self, bundle: CorpusBundle) -> PreparedCorpus:
        raise AdapterError(
            "ShellAdapter is a placeholder for the future Rust engine binary. "
            "Use --adapter fixture for runnable Phase 1 harness flows."
        )

    def execute_exact_query(
        self,
        bundle: CorpusBundle,
        query: ExactQuery,
    ) -> QueryExecutionResult:
        raise self._not_ready_error()

    def execute_symbol_query(
        self,
        bundle: CorpusBundle,
        query: SymbolQuery,
    ) -> QueryExecutionResult:
        raise self._not_ready_error()

    def execute_semantic_query(
        self,
        bundle: CorpusBundle,
        query: SemanticQuery,
    ) -> QueryExecutionResult:
        raise self._not_ready_error()

    def execute_impact_query(
        self,
        bundle: CorpusBundle,
        query: ImpactQuery,
    ) -> QueryExecutionResult:
        raise self._not_ready_error()

    def run_incremental_refresh(
        self,
        bundle: CorpusBundle,
        scenario: dict[str, object],
    ) -> RefreshExecutionResult:
        raise self._not_ready_error()

    def _not_ready_error(self) -> AdapterError:
        engine_hint = (
            f" Engine binary path received: {self.engine_bin}."
            if self.engine_bin is not None
            else ""
        )
        return AdapterError(
            "ShellAdapter is not implemented yet and exists only to preserve the adapter "
            f"boundary for a future Rust engine.{engine_hint}"
        )

    def close(self) -> None:
        return None


@dataclass
class _DaemonRepoSession:
    repo_root: Path
    repo_id: str
    clean_snapshot_id: str


class DaemonSymbolAdapter:
    """Daemon-protocol adapter for the real Phase 4 symbol engine."""

    name = "daemon-symbol"

    def __init__(
        self,
        *,
        engine_bin: str | None = None,
        build_temperature: str = "cold",
        workspace_root: str | None = None,
    ) -> None:
        if build_temperature not in {"cold", "warm"}:
            raise AdapterError("build_temperature must be either 'cold' or 'warm'.")
        self.engine_bin = engine_bin
        self.build_temperature = build_temperature
        self._workspace_override = Path(workspace_root).resolve() if workspace_root else None
        self._temp_workspace: tempfile.TemporaryDirectory[str] | None = None
        self._workspace_root: Path | None = None
        self._runtime_root: Path | None = None
        self._socket_path: Path | None = None
        self._log_path: Path | None = None
        self._config_path: Path | None = None
        self._daemon_process: subprocess.Popen[str] | None = None
        self._transport_mode = "unix_socket"
        self._request_counter = 0
        self._query_session: _DaemonRepoSession | None = None
        self._prepare_metadata: dict[str, object] = {}

    def prepare_corpus(self, bundle: CorpusBundle) -> PreparedCorpus:
        repo_source = bundle.root_dir / "repo"
        if not repo_source.exists():
            raise AdapterError(f"Corpus bundle does not contain repo/: {repo_source}")
        started_at = time.perf_counter()
        self._ensure_daemon_started()
        session = self._bootstrap_repo_copy(repo_source, label="query")
        parse_build, parse_latency_ms = self._measure_request(
            "parse_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
                "force": False,
            },
        )
        symbol_build, symbol_latency_ms = self._measure_request(
            "symbol_index_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
                "force": False,
            },
        )
        prime_parse_build = None
        prime_symbol_build = None
        if self.build_temperature == "warm":
            prime_parse_build = parse_build
            prime_symbol_build = symbol_build
            parse_build, parse_latency_ms = self._measure_request(
                "parse_build",
                {
                    "repo_id": session.repo_id,
                    "snapshot_id": session.clean_snapshot_id,
                    "force": False,
                },
            )
            symbol_build, symbol_latency_ms = self._measure_request(
                "symbol_index_build",
                {
                    "repo_id": session.repo_id,
                    "snapshot_id": session.clean_snapshot_id,
                    "force": False,
                },
            )
        total_latency_ms = (time.perf_counter() - started_at) * 1000.0
        self._query_session = session
        self._prepare_metadata = {
            "transport": (
                "daemon_protocol"
                if self._transport_mode == "unix_socket"
                else "daemon_protocol_stdio"
            ),
            "engine_backend": "phase4-symbol-daemon",
            "build_temperature": self.build_temperature,
            "launcher": self._launcher_label(),
            "transport_mode": self._transport_mode,
            "workspace_root": str(self._workspace_root),
            "runtime_root": str(self._runtime_root),
            "socket_path": str(self._socket_path),
            "log_path": str(self._log_path),
            "repo_id": session.repo_id,
            "clean_snapshot_id": session.clean_snapshot_id,
            "parse_build": _compact_parse_build(parse_build),
            "symbol_build": _compact_symbol_build(symbol_build),
        }
        if prime_parse_build is not None:
            self._prepare_metadata["prime_parse_build"] = _compact_parse_build(prime_parse_build)
        if prime_symbol_build is not None:
            self._prepare_metadata["prime_symbol_build"] = _compact_symbol_build(prime_symbol_build)
        notes = [
            "Real Phase 4 symbol engine via daemon protocol.",
            f"build_temperature={self.build_temperature}",
            f"repo_id={session.repo_id}",
            f"clean_snapshot_id={session.clean_snapshot_id}",
        ]
        return PreparedCorpus(
            corpus_id=bundle.manifest.corpus_id,
            latency_ms=total_latency_ms,
            notes=notes,
            metadata=self._prepare_metadata,
            metric_rows=[
                _metric_row("prepare-latency", "latency", "ms", total_latency_ms),
                _metric_row("prepare-parse-build-latency", "latency", "ms", parse_latency_ms),
                _metric_row("prepare-symbol-build-latency", "latency", "ms", symbol_latency_ms),
                _metric_row(
                    "prepare-parse-parsed-file-count",
                    "custom",
                    "count",
                    float(parse_build["build"]["counts"]["parsed_file_count"]),
                ),
                _metric_row(
                    "prepare-parse-reused-file-count",
                    "custom",
                    "count",
                    float(parse_build["build"]["counts"]["reused_file_count"]),
                ),
                _metric_row(
                    "prepare-symbol-file-count",
                    "custom",
                    "count",
                    float(symbol_build["build"]["stats"]["file_count"]),
                ),
                _metric_row(
                    "prepare-symbol-loaded-from-existing",
                    "custom",
                    "count",
                    1.0 if symbol_build["build"].get("loaded_from_existing_build") else 0.0,
                ),
            ],
        )

    def execute_exact_query(
        self,
        bundle: CorpusBundle,
        query: ExactQuery,
    ) -> QueryExecutionResult:
        raise AdapterError(
            "DaemonSymbolAdapter currently supports symbol queries only. "
            "Limit the run to the symbol query pack."
        )

    def execute_symbol_query(
        self,
        bundle: CorpusBundle,
        query: SymbolQuery,
    ) -> QueryExecutionResult:
        if query.scope != SymbolScope.REPO:
            raise AdapterError(
                f"DaemonSymbolAdapter currently supports repo-scoped symbol queries only; "
                f"received scope={query.scope.value!r} for {query.query_id}."
            )
        session = self._require_query_session()
        started_at = time.perf_counter()
        response = self._request(
            "symbol_search",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
                "query": {
                    "text": query.symbol,
                    "mode": "exact",
                    "kinds": [
                        "file",
                        "module",
                        "namespace",
                        "class",
                        "interface",
                        "type_alias",
                        "enum",
                        "enum_member",
                        "function",
                        "method",
                        "constructor",
                        "property",
                        "field",
                        "variable",
                        "constant",
                    ],
                    "path_prefix": None,
                },
                "limit": query.limit,
            },
        )
        hits: list[QueryHit] = []
        seen_paths: set[str] = set()
        for hit in response.get("hits", []):
            symbol = hit.get("symbol")
            if not isinstance(symbol, dict):
                continue
            symbol_id = _coerce_optional_str(symbol.get("symbol_id"))
            display_name = _coerce_optional_str(symbol.get("display_name"))
            resolved_paths = self._definition_paths_for_symbol(
                repo_id=session.repo_id,
                snapshot_id=session.clean_snapshot_id,
                symbol_id=symbol_id,
            )
            candidate_paths = resolved_paths or [_coerce_str(symbol.get("path"), "symbol.path")]
            for path in candidate_paths:
                if path in seen_paths:
                    continue
                seen_paths.add(path)
                hits.append(
                    QueryHit(
                        path=path,
                        symbol=display_name,
                        rank=len(hits) + 1,
                        reason="definition",
                        score=float(hit.get("score", 0.0)),
                    )
                )
        diagnostics = response.get("diagnostics", [])
        notes = [
            "Served by the daemon-backed Phase 4 symbol search.",
            f"snapshot_id={session.clean_snapshot_id}",
        ]
        if diagnostics:
            notes.append(f"diagnostics={len(diagnostics)}")
        return QueryExecutionResult(
            query_id=query.query_id,
            query_type=QueryType.SYMBOL,
            latency_ms=(time.perf_counter() - started_at) * 1000.0,
            hits=hits,
            notes=notes,
        )

    def execute_semantic_query(
        self,
        bundle: CorpusBundle,
        query: SemanticQuery,
    ) -> QueryExecutionResult:
        raise AdapterError(
            "DaemonSymbolAdapter currently supports symbol queries only. "
            "Semantic benchmarking remains out of scope for this slice."
        )

    def execute_impact_query(
        self,
        bundle: CorpusBundle,
        query: ImpactQuery,
    ) -> QueryExecutionResult:
        raise AdapterError(
            "DaemonSymbolAdapter currently supports symbol queries only. "
            "Impact benchmarking remains out of scope for this slice."
        )

    def run_incremental_refresh(
        self,
        bundle: CorpusBundle,
        scenario: dict[str, object],
    ) -> RefreshExecutionResult:
        scenario_id = _coerce_str(scenario.get("scenario_id"), "scenario_id")
        session = self._bootstrap_repo_copy(bundle.root_dir / "repo", label=scenario_id)
        self._request(
            "parse_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
                "force": False,
            },
        )
        self._request(
            "symbol_index_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
                "force": False,
            },
        )
        target_path = _coerce_str(scenario.get("target_path"), "target_path")
        before_snippet = _coerce_str(scenario.get("before_snippet"), "before_snippet")
        after_snippet = _coerce_str(scenario.get("after_snippet"), "after_snippet")
        target_file = session.repo_root / target_path
        try:
            original = target_file.read_text(encoding="utf-8")
        except FileNotFoundError as exc:
            raise AdapterError(f"Refresh target file does not exist: {target_file}") from exc
        if before_snippet not in original:
            raise AdapterError(
                f"Refresh scenario '{scenario_id}' could not find the expected snippet in "
                f"{target_path}."
            )
        target_file.write_text(original.replace(before_snippet, after_snippet, 1), encoding="utf-8")

        dirty_snapshot = self._create_snapshot(session.repo_id)
        parse_build, parse_latency_ms = self._measure_request(
            "parse_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": dirty_snapshot,
                "force": False,
            },
        )
        symbol_build, symbol_latency_ms = self._measure_request(
            "symbol_index_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": dirty_snapshot,
                "force": False,
            },
        )
        latency_ms = parse_latency_ms + symbol_latency_ms
        changed_queries = [
            _coerce_str(query_id, "expected_changed_queries[]")
            for query_id in scenario.get("expected_changed_queries", [])
        ]
        build_record = symbol_build["build"]
        parse_record = parse_build["build"]
        notes = [
            "Incremental refresh measured against a fresh clean baseline repo copy.",
            f"dirty_snapshot_id={dirty_snapshot}",
            f"refresh_mode={build_record.get('refresh_mode') or 'unknown'}",
        ]
        if build_record.get("fallback_reason"):
            notes.append(f"fallback_reason={build_record['fallback_reason']}")
        return RefreshExecutionResult(
            scenario_id=scenario_id,
            latency_ms=latency_ms,
            changed_queries=changed_queries,
            notes=notes,
            metadata={
                "target_path": target_path,
                "dirty_snapshot_id": dirty_snapshot,
                "parse_build_latency_ms": parse_latency_ms,
                "symbol_build_latency_ms": symbol_latency_ms,
                "parse_build": _compact_parse_build(parse_build),
                "symbol_build": _compact_symbol_build(symbol_build),
                "refresh_mode": build_record.get("refresh_mode"),
                "fallback_reason": build_record.get("fallback_reason"),
                "loaded_from_existing_build": bool(
                    build_record.get("loaded_from_existing_build", False)
                ),
                "parsed_file_count": parse_record["counts"]["parsed_file_count"],
                "reused_file_count": parse_record["counts"]["reused_file_count"],
            },
            metric_rows=[
                _metric_row("refresh-parse-build-latency", "latency", "ms", parse_latency_ms),
                _metric_row("refresh-symbol-build-latency", "latency", "ms", symbol_latency_ms),
                _metric_row(
                    "refresh-parse-parsed-file-count",
                    "custom",
                    "count",
                    float(parse_record["counts"]["parsed_file_count"]),
                ),
                _metric_row(
                    "refresh-parse-reused-file-count",
                    "custom",
                    "count",
                    float(parse_record["counts"]["reused_file_count"]),
                ),
                _metric_row(
                    "refresh-incremental-count",
                    "custom",
                    "count",
                    1.0 if build_record.get("refresh_mode") == "incremental" else 0.0,
                ),
                _metric_row(
                    "refresh-full-rebuild-count",
                    "custom",
                    "count",
                    1.0 if build_record.get("refresh_mode") == "full_rebuild" else 0.0,
                ),
            ],
        )

    def close(self) -> None:
        if self._daemon_process is not None:
            self._stop_daemon()
        self._daemon_process = None
        if self._temp_workspace is not None:
            self._temp_workspace.cleanup()
            self._temp_workspace = None
        self._workspace_root = None
        self._runtime_root = None
        self._socket_path = None
        self._log_path = None
        self._config_path = None
        self._transport_mode = "unix_socket"
        self._query_session = None
        self._prepare_metadata = {}

    def _require_query_session(self) -> _DaemonRepoSession:
        if self._query_session is None:
            raise AdapterError("prepare_corpus() must run before executing daemon-backed queries.")
        return self._query_session

    def _ensure_daemon_started(self) -> None:
        if self._daemon_process is not None:
            return
        self._workspace_root = self._workspace_override or Path(
            self._temp_directory().name
        ).resolve()
        self._runtime_root = self._workspace_root / ".hyperindex"
        self._socket_path = self._runtime_root / "hyperd.sock"
        self._log_path = self._runtime_root / "logs" / "hyperd.log"
        self._config_path = self._workspace_root / "hyperbench-hyperd.toml"
        if self._workspace_override:
            self._workspace_root.mkdir(parents=True, exist_ok=True)
        self._log_path.parent.mkdir(parents=True, exist_ok=True)
        self._write_runtime_config("unix_socket")
        log_handle = self._log_path.open("a", encoding="utf-8")
        command = self._launcher_command()
        try:
            self._daemon_process = subprocess.Popen(
                command,
                cwd=self._workspace_root,
                stdout=log_handle,
                stderr=subprocess.STDOUT,
                text=True,
            )
        finally:
            log_handle.close()
        deadline = time.monotonic() + 120.0
        while time.monotonic() < deadline:
            if self._daemon_process.poll() is not None:
                log_excerpt = self._read_log_tail()
                if log_excerpt and "bind failed" in log_excerpt:
                    self._daemon_process = None
                    self._transport_mode = "stdio"
                    self._write_runtime_config("stdio")
                    return
                raise AdapterError(
                    "hyperd exited before becoming ready."
                    + (f" Last log line: {log_excerpt}" if log_excerpt else "")
                )
            try:
                self._request("daemon_status", {})
                return
            except AdapterError:
                time.sleep(0.05)
        raise AdapterError(
            f"Timed out waiting for hyperd to become ready. See {self._log_path}."
        )

    def _bootstrap_repo_copy(self, repo_source: Path, *, label: object) -> _DaemonRepoSession:
        workspace_root = self._workspace_root
        if workspace_root is None:
            raise AdapterError("Daemon workspace is not initialized.")
        repo_root = workspace_root / "repos" / _safe_directory_name(str(label or "repo"))
        if repo_root.exists():
            shutil.rmtree(repo_root)
        shutil.copytree(repo_source, repo_root)
        self._git_init_repo(repo_root)
        add_response = self._request(
            "repos_add",
            {
                "repo_root": str(repo_root),
                "display_name": f"Hyperbench {label}",
                "notes": ["hyperbench-daemon-adapter"],
                "ignore_patterns": [],
                "watch_on_add": False,
            },
        )
        repo_id = _coerce_str(add_response["repo"]["repo_id"], "repo.repo_id")
        clean_snapshot_id = self._create_snapshot(repo_id)
        return _DaemonRepoSession(
            repo_root=repo_root,
            repo_id=repo_id,
            clean_snapshot_id=clean_snapshot_id,
        )

    def _git_init_repo(self, repo_root: Path) -> None:
        subprocess.run(["git", "init"], cwd=repo_root, check=True, capture_output=True, text=True)
        subprocess.run(
            ["git", "checkout", "-b", "trunk"],
            cwd=repo_root,
            check=True,
            capture_output=True,
            text=True,
        )
        subprocess.run(
            ["git", "add", "."],
            cwd=repo_root,
            check=True,
            capture_output=True,
            text=True,
        )
        env = os.environ.copy()
        env.update(
            {
                "GIT_AUTHOR_NAME": "Codex",
                "GIT_AUTHOR_EMAIL": "codex@example.com",
                "GIT_COMMITTER_NAME": "Codex",
                "GIT_COMMITTER_EMAIL": "codex@example.com",
            }
        )
        subprocess.run(
            ["git", "commit", "-m", "hyperbench baseline"],
            cwd=repo_root,
            env=env,
            check=True,
            capture_output=True,
            text=True,
        )

    def _create_snapshot(self, repo_id: str) -> str:
        response = self._request(
            "snapshots_create",
            {
                "repo_id": repo_id,
                "include_working_tree": True,
                "buffer_ids": [],
            },
        )
        snapshot = response.get("snapshot")
        if not isinstance(snapshot, dict):
            raise AdapterError("snapshots_create returned an invalid snapshot payload.")
        return _coerce_str(snapshot.get("snapshot_id"), "snapshot.snapshot_id")

    def _definition_paths_for_symbol(
        self,
        *,
        repo_id: str,
        snapshot_id: str,
        symbol_id: str | None,
    ) -> list[str]:
        if not symbol_id:
            return []
        response = self._request(
            "definition_lookup",
            {
                "repo_id": repo_id,
                "snapshot_id": snapshot_id,
                "symbol_id": symbol_id,
            },
        )
        definitions = response.get("definitions", [])
        paths: list[str] = []
        for occurrence in definitions:
            if not isinstance(occurrence, dict):
                continue
            path = _coerce_optional_str(occurrence.get("path"))
            if path and path not in paths:
                paths.append(path)
        return paths

    def _measure_request(
        self,
        method: str,
        params: dict[str, object],
    ) -> tuple[dict[str, object], float]:
        started_at = time.perf_counter()
        response = self._request(method, params)
        return response, (time.perf_counter() - started_at) * 1000.0

    def _request(self, method: str, params: dict[str, object]) -> dict[str, object]:
        self._request_counter += 1
        request = {
            "protocol_version": "repo-hyperindex.local/v1",
            "request_id": f"hyperbench-{self._request_counter:04d}",
            "method": method,
            "params": params,
        }
        encoded = json.dumps(request, sort_keys=True).encode("utf-8")
        if self._transport_mode == "stdio":
            chunks = self._request_via_stdio(method, encoded)
        else:
            chunks = self._request_via_unix_socket(method, encoded)
        try:
            response = json.loads(b"".join(chunks).decode("utf-8"))
        except json.JSONDecodeError as exc:
            raise AdapterError(f"Daemon response for {method} was not valid JSON.") from exc
        if response.get("status") == "error":
            error = response.get("error")
            if not isinstance(error, dict):
                raise AdapterError(f"Daemon request {method} failed without an error payload.")
            raise AdapterError(
                f"Daemon request {method} failed: "
                f"{error.get('code', 'unknown_error')} - {error.get('message', 'unknown error')}"
            )
        result = response.get("result")
        if not isinstance(result, dict):
            raise AdapterError(f"Daemon request {method} returned an invalid result payload.")
        return result

    def _request_via_unix_socket(self, method: str, encoded: bytes) -> list[bytes]:
        socket_path = self._socket_path
        if socket_path is None:
            raise AdapterError("Daemon socket is not initialized.")
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                client.settimeout(30.0)
                client.connect(str(socket_path))
                client.sendall(encoded)
                client.shutdown(socket.SHUT_WR)
                chunks: list[bytes] = []
                while True:
                    chunk = client.recv(65536)
                    if not chunk:
                        break
                    chunks.append(chunk)
        except OSError as exc:
            raise AdapterError(f"Daemon protocol request failed for {method}: {exc}") from exc
        return chunks

    def _request_via_stdio(self, method: str, encoded: bytes) -> list[bytes]:
        completed = subprocess.run(
            self._launcher_command(),
            cwd=self._workspace_root,
            input=encoded,
            capture_output=True,
            check=False,
        )
        if completed.returncode != 0:
            stderr = completed.stderr.decode("utf-8", errors="replace").strip()
            raise AdapterError(
                f"Daemon stdio request failed for {method}: "
                f"{stderr or f'exit code {completed.returncode}'}"
            )
        return [completed.stdout]

    def _launcher_command(self) -> list[str]:
        config_path = self._config_path
        if config_path is None:
            raise AdapterError("Daemon config path is not initialized.")
        if self.engine_bin is not None:
            return [
                str(Path(self.engine_bin).resolve()),
                "--config-path",
                str(config_path),
                "serve",
            ]
        env_bin = os.environ.get("HYPERD_BIN")
        if env_bin:
            return [
                str(Path(env_bin).resolve()),
                "--config-path",
                str(config_path),
                "serve",
            ]
        repo_root = Path(__file__).resolve().parents[2]
        target_bin = repo_root / "target" / "debug" / "hyperd"
        if target_bin.exists():
            return [str(target_bin), "--config-path", str(config_path), "serve"]
        return [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "hyperindex-daemon",
            "--bin",
            "hyperd",
            "--",
            "--config-path",
            str(config_path),
            "serve",
        ]

    def _launcher_label(self) -> str:
        if self.engine_bin is not None:
            return "explicit-binary"
        if os.environ.get("HYPERD_BIN"):
            return "env-binary"
        repo_root = Path(__file__).resolve().parents[2]
        if (repo_root / "target" / "debug" / "hyperd").exists():
            return "workspace-binary"
        return "cargo-run"

    def _stop_daemon(self) -> None:
        assert self._daemon_process is not None
        try:
            self._request("shutdown", {"graceful": True, "timeout_ms": 5_000})
        except AdapterError:
            self._daemon_process.terminate()
        try:
            self._daemon_process.wait(timeout=10.0)
        except subprocess.TimeoutExpired:
            self._daemon_process.kill()
            self._daemon_process.wait(timeout=5.0)

    def _temp_directory(self) -> tempfile.TemporaryDirectory[str]:
        if self._temp_workspace is None:
            self._temp_workspace = tempfile.TemporaryDirectory(prefix="hyperbench-daemon-")
        return self._temp_workspace

    def _read_log_tail(self) -> str | None:
        if self._log_path is None or not self._log_path.exists():
            return None
        lines = self._log_path.read_text(encoding="utf-8").splitlines()
        return lines[-1] if lines else None

    def _write_runtime_config(self, transport_kind: str) -> None:
        if self._runtime_root is None or self._config_path is None or self._socket_path is None:
            raise AdapterError("Daemon runtime paths are not initialized.")
        runtime_root = self._runtime_root
        state_dir = runtime_root / "state"
        data_dir = runtime_root / "data"
        manifests_dir = data_dir / "manifests"
        logs_dir = runtime_root / "logs"
        temp_dir = runtime_root / "tmp"
        parse_artifact_dir = data_dir / "parse-artifacts"
        symbol_store_dir = data_dir / "symbols"
        for path in (
            runtime_root,
            state_dir,
            data_dir,
            manifests_dir,
            logs_dir,
            temp_dir,
            parse_artifact_dir,
            symbol_store_dir,
        ):
            path.mkdir(parents=True, exist_ok=True)
        self._config_path.write_text(
            "\n".join(
                [
                    'version = 1',
                    'protocol_version = "repo-hyperindex.local/v1"',
                    "",
                    "[directories]",
                    f'runtime_root = "{runtime_root}"',
                    f'state_dir = "{state_dir}"',
                    f'data_dir = "{data_dir}"',
                    f'manifests_dir = "{manifests_dir}"',
                    f'logs_dir = "{logs_dir}"',
                    f'temp_dir = "{temp_dir}"',
                    "",
                    "[transport]",
                    f'kind = "{transport_kind}"',
                    f'socket_path = "{self._socket_path}"',
                    "connect_timeout_ms = 2000",
                    "request_timeout_ms = 30000",
                    "max_frame_bytes = 1048576",
                    "",
                    "[repo_registry]",
                    'backend = "sqlite"',
                    f'sqlite_path = "{state_dir / "runtime.sqlite3"}"',
                    f'manifests_dir = "{manifests_dir}"',
                    "",
                    "[watch]",
                    'backend = "poll"',
                    "poll_interval_ms = 250",
                    "debounce_ms = 100",
                    "batch_max_events = 256",
                    "",
                    "[scheduler]",
                    "max_concurrent_repos = 1",
                    "coalesce_window_ms = 150",
                    "idle_flush_ms = 500",
                    "job_lease_ms = 30000",
                    "",
                    "[logging]",
                    'verbosity = "info"',
                    'format = "text"',
                    "",
                    "[ignores]",
                    'global_patterns = [".git/**", "node_modules/**", "target/**", ".next/**"]',
                    "repo_patterns = []",
                    "exclude_dot_git = true",
                    "exclude_node_modules = true",
                    "exclude_target = true",
                    "",
                    "[parser]",
                    "enabled = true",
                    "max_file_bytes = 2097152",
                    "diagnostics_max_per_file = 32",
                    'cache_mode = "persistent"',
                    f'artifact_dir = "{parse_artifact_dir}"',
                    "",
                    "[[parser.language_packs]]",
                    'pack_id = "ts_js_core"',
                    "enabled = true",
                    'languages = ["typescript", "tsx", "javascript", "jsx", "mts", "cts"]',
                    'include_globs = ['
                    '"**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx", "**/*.mts", "**/*.cts"'
                    ']',
                    'grammar_version = "tree-sitter-typescript@phase4-contract"',
                    "",
                    "[symbol_index]",
                    "enabled = true",
                    f'store_dir = "{symbol_store_dir}"',
                    "default_search_limit = 25",
                    "max_search_limit = 200",
                    "persist_occurrences = true",
                    "",
                ]
            )
            + "\n",
            encoding="utf-8",
        )


class DaemonImpactAdapter(DaemonSymbolAdapter):
    """Daemon-protocol adapter for the real Phase 5 impact engine."""

    name = "daemon-impact"

    def prepare_corpus(self, bundle: CorpusBundle) -> PreparedCorpus:
        repo_source = bundle.root_dir / "repo"
        if not repo_source.exists():
            raise AdapterError(f"Corpus bundle does not contain repo/: {repo_source}")
        representative_query = _select_representative_impact_query(bundle)
        started_at = time.perf_counter()
        self._ensure_daemon_started()
        session = self._bootstrap_repo_copy(repo_source, label="impact-query")
        parse_build, parse_latency_ms = self._measure_request(
            "parse_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
                "force": False,
            },
        )
        symbol_build, symbol_latency_ms = self._measure_request(
            "symbol_index_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
                "force": False,
            },
        )
        status_before = self._request(
            "impact_status",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
            },
        )
        impact_response, impact_latency_ms, impact_notes = self._measure_impact_query(
            session=session,
            snapshot_id=session.clean_snapshot_id,
            query=representative_query,
        )
        prime_parse_build = None
        prime_symbol_build = None
        prime_impact_response = None
        prime_impact_latency_ms = None
        if self.build_temperature == "warm":
            prime_parse_build = parse_build
            prime_symbol_build = symbol_build
            prime_impact_response = impact_response
            prime_impact_latency_ms = impact_latency_ms
            parse_build, parse_latency_ms = self._measure_request(
                "parse_build",
                {
                    "repo_id": session.repo_id,
                    "snapshot_id": session.clean_snapshot_id,
                    "force": False,
                },
            )
            symbol_build, symbol_latency_ms = self._measure_request(
                "symbol_index_build",
                {
                    "repo_id": session.repo_id,
                    "snapshot_id": session.clean_snapshot_id,
                    "force": False,
                },
            )
            impact_response, impact_latency_ms, impact_notes = self._measure_impact_query(
                session=session,
                snapshot_id=session.clean_snapshot_id,
                query=representative_query,
            )
        total_latency_ms = (time.perf_counter() - started_at) * 1000.0
        impact_manifest = dict(impact_response.get("manifest") or {})
        impact_refresh_stats = dict(impact_manifest.get("refresh_stats", {}))
        self._query_session = session
        self._prepare_metadata = {
            "transport": (
                "daemon_protocol"
                if self._transport_mode == "unix_socket"
                else "daemon_protocol_stdio"
            ),
            "engine_backend": "phase5-impact-daemon",
            "build_temperature": self.build_temperature,
            "launcher": self._launcher_label(),
            "transport_mode": self._transport_mode,
            "workspace_root": str(self._workspace_root),
            "runtime_root": str(self._runtime_root),
            "socket_path": str(self._socket_path),
            "log_path": str(self._log_path),
            "repo_id": session.repo_id,
            "clean_snapshot_id": session.clean_snapshot_id,
            "parse_build": _compact_parse_build(parse_build),
            "symbol_build": _compact_symbol_build(symbol_build),
            "impact_status_before": _compact_impact_status(status_before),
            "representative_query": {
                "query_id": representative_query.query_id,
                "target_type": representative_query.target_type.value,
                "target": representative_query.target,
                "change_hint": representative_query.change_hint.value,
            },
            "impact_analyze": _compact_impact_analyze(impact_response),
        }
        if prime_parse_build is not None:
            self._prepare_metadata["prime_parse_build"] = _compact_parse_build(prime_parse_build)
        if prime_symbol_build is not None:
            self._prepare_metadata["prime_symbol_build"] = _compact_symbol_build(prime_symbol_build)
        if prime_impact_response is not None:
            self._prepare_metadata["prime_impact_analyze"] = _compact_impact_analyze(
                prime_impact_response
            )
            self._prepare_metadata["prime_impact_analyze_latency_ms"] = prime_impact_latency_ms
        notes = [
            "Real Phase 5 impact engine via daemon protocol.",
            f"build_temperature={self.build_temperature}",
            f"repo_id={session.repo_id}",
            f"clean_snapshot_id={session.clean_snapshot_id}",
            f"representative_query_id={representative_query.query_id}",
            *impact_notes,
        ]
        metric_rows = [
            _metric_row("prepare-latency", "latency", "ms", total_latency_ms),
            _metric_row("prepare-parse-build-latency", "latency", "ms", parse_latency_ms),
            _metric_row("prepare-symbol-build-latency", "latency", "ms", symbol_latency_ms),
            _metric_row("prepare-impact-analyze-latency", "latency", "ms", impact_latency_ms),
            _metric_row(
                "prepare-parse-parsed-file-count",
                "custom",
                "count",
                float(parse_build["build"]["counts"]["parsed_file_count"]),
            ),
            _metric_row(
                "prepare-parse-reused-file-count",
                "custom",
                "count",
                float(parse_build["build"]["counts"]["reused_file_count"]),
            ),
            _metric_row(
                "prepare-symbol-file-count",
                "custom",
                "count",
                float(symbol_build["build"]["stats"]["file_count"]),
            ),
            _metric_row(
                "prepare-symbol-loaded-from-existing",
                "custom",
                "count",
                1.0 if symbol_build["build"].get("loaded_from_existing_build") else 0.0,
            ),
            _metric_row(
                "prepare-impact-hit-count",
                "custom",
                "count",
                float(_impact_hit_count(impact_response)),
            ),
            _metric_row(
                "prepare-impact-loaded-from-existing",
                "custom",
                "count",
                1.0 if impact_manifest.get("loaded_from_existing_build") else 0.0,
            ),
            _metric_row(
                "prepare-impact-full-compute-count",
                "custom",
                "count",
                1.0 if impact_manifest.get("refresh_mode") == "full_rebuild" else 0.0,
            ),
            _metric_row(
                "prepare-impact-incremental-count",
                "custom",
                "count",
                1.0 if impact_manifest.get("refresh_mode") == "incremental" else 0.0,
            ),
        ]
        if impact_refresh_stats.get("elapsed_ms") is not None:
            metric_rows.append(
                _metric_row(
                    "prepare-impact-refresh-elapsed-ms",
                    "latency",
                    "ms",
                    float(impact_refresh_stats["elapsed_ms"]),
                )
            )
        for field_name, metric_name in (
            ("files_touched", "prepare-impact-files-touched"),
            ("entities_recomputed", "prepare-impact-entities-recomputed"),
            ("edges_refreshed", "prepare-impact-edges-refreshed"),
        ):
            if impact_refresh_stats.get(field_name) is not None:
                metric_rows.append(
                    _metric_row(
                        metric_name,
                        "custom",
                        "count",
                        float(impact_refresh_stats[field_name]),
                    )
                )
        return PreparedCorpus(
            corpus_id=bundle.manifest.corpus_id,
            latency_ms=total_latency_ms,
            notes=notes,
            metadata=self._prepare_metadata,
            metric_rows=metric_rows,
        )

    def execute_exact_query(
        self,
        bundle: CorpusBundle,
        query: ExactQuery,
    ) -> QueryExecutionResult:
        raise AdapterError(
            "DaemonImpactAdapter currently supports impact queries only. "
            "Limit the run to the impact query pack."
        )

    def execute_symbol_query(
        self,
        bundle: CorpusBundle,
        query: SymbolQuery,
    ) -> QueryExecutionResult:
        raise AdapterError(
            "DaemonImpactAdapter currently supports impact queries only. "
            "Limit the run to the impact query pack."
        )

    def execute_semantic_query(
        self,
        bundle: CorpusBundle,
        query: SemanticQuery,
    ) -> QueryExecutionResult:
        raise AdapterError(
            "DaemonImpactAdapter currently supports impact queries only. "
            "Limit the run to the impact query pack."
        )

    def execute_impact_query(
        self,
        bundle: CorpusBundle,
        query: ImpactQuery,
    ) -> QueryExecutionResult:
        session = self._require_query_session()
        response, latency_ms, notes = self._measure_impact_query(
            session=session,
            snapshot_id=session.clean_snapshot_id,
            query=query,
        )
        diagnostics = response.get("diagnostics", [])
        if diagnostics:
            notes.append(f"diagnostics={len(diagnostics)}")
        return QueryExecutionResult(
            query_id=query.query_id,
            query_type=QueryType.IMPACT,
            latency_ms=latency_ms,
            hits=_normalize_impact_hits(response),
            notes=notes,
        )

    def run_incremental_refresh(
        self,
        bundle: CorpusBundle,
        scenario: dict[str, object],
    ) -> RefreshExecutionResult:
        scenario_id = _coerce_str(scenario.get("scenario_id"), "scenario_id")
        session = self._bootstrap_repo_copy(bundle.root_dir / "repo", label=f"impact-{scenario_id}")
        self._request(
            "parse_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
                "force": False,
            },
        )
        self._request(
            "symbol_index_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": session.clean_snapshot_id,
                "force": False,
            },
        )
        refresh_query = _refresh_query_for_scenario(bundle, scenario)
        self._measure_impact_query(
            session=session,
            snapshot_id=session.clean_snapshot_id,
            query=refresh_query,
        )
        target_path = _coerce_str(scenario.get("target_path"), "target_path")
        before_snippet = _coerce_str(scenario.get("before_snippet"), "before_snippet")
        after_snippet = _coerce_str(scenario.get("after_snippet"), "after_snippet")
        target_file = session.repo_root / target_path
        try:
            original = target_file.read_text(encoding="utf-8")
        except FileNotFoundError as exc:
            raise AdapterError(f"Refresh target file does not exist: {target_file}") from exc
        if before_snippet not in original:
            raise AdapterError(
                f"Refresh scenario '{scenario_id}' could not find the expected snippet in "
                f"{target_path}."
            )
        target_file.write_text(original.replace(before_snippet, after_snippet, 1), encoding="utf-8")

        dirty_snapshot = self._create_snapshot(session.repo_id)
        parse_build, parse_latency_ms = self._measure_request(
            "parse_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": dirty_snapshot,
                "force": False,
            },
        )
        symbol_build, symbol_latency_ms = self._measure_request(
            "symbol_index_build",
            {
                "repo_id": session.repo_id,
                "snapshot_id": dirty_snapshot,
                "force": False,
            },
        )
        impact_response, impact_latency_ms, impact_notes = self._measure_impact_query(
            session=session,
            snapshot_id=dirty_snapshot,
            query=refresh_query,
        )
        latency_ms = parse_latency_ms + symbol_latency_ms + impact_latency_ms
        changed_queries = [
            _coerce_str(query_id, "expected_changed_queries[]")
            for query_id in scenario.get("expected_changed_queries", [])
        ]
        build_record = symbol_build["build"]
        parse_record = parse_build["build"]
        impact_manifest = dict(impact_response.get("manifest") or {})
        impact_refresh_stats = dict(impact_manifest.get("refresh_stats", {}))
        refresh_mode = impact_manifest.get("refresh_mode")
        notes = [
            "Incremental impact refresh measured against a fresh clean baseline repo copy.",
            f"dirty_snapshot_id={dirty_snapshot}",
            f"refresh_mode={refresh_mode or 'unknown'}",
            f"refresh_query_id={refresh_query.query_id}",
            *impact_notes,
        ]
        if impact_manifest.get("fallback_reason"):
            notes.append(f"fallback_reason={impact_manifest['fallback_reason']}")
        return RefreshExecutionResult(
            scenario_id=scenario_id,
            latency_ms=latency_ms,
            changed_queries=changed_queries,
            notes=notes,
            metadata={
                "target_path": target_path,
                "dirty_snapshot_id": dirty_snapshot,
                "parse_build_latency_ms": parse_latency_ms,
                "symbol_build_latency_ms": symbol_latency_ms,
                "impact_analyze_latency_ms": impact_latency_ms,
                "parse_build": _compact_parse_build(parse_build),
                "symbol_build": _compact_symbol_build(symbol_build),
                "impact_analyze": _compact_impact_analyze(impact_response),
                "refresh_mode": refresh_mode,
                "fallback_reason": impact_manifest.get("fallback_reason"),
                "loaded_from_existing_build": bool(
                    impact_manifest.get("loaded_from_existing_build", False)
                ),
                "symbol_refresh_mode": build_record.get("refresh_mode"),
                "symbol_fallback_reason": build_record.get("fallback_reason"),
                "symbol_loaded_from_existing_build": bool(
                    build_record.get("loaded_from_existing_build", False)
                ),
                "impact_refresh_mode": refresh_mode,
                "impact_fallback_reason": impact_manifest.get("fallback_reason"),
                "impact_loaded_from_existing_build": bool(
                    impact_manifest.get("loaded_from_existing_build", False)
                ),
                "impact_refresh_elapsed_ms": impact_refresh_stats.get("elapsed_ms"),
                "impact_refresh_files_touched": impact_refresh_stats.get("files_touched"),
                "impact_refresh_entities_recomputed": impact_refresh_stats.get(
                    "entities_recomputed"
                ),
                "impact_refresh_edges_refreshed": impact_refresh_stats.get("edges_refreshed"),
                "impact_query_id": refresh_query.query_id,
                "impact_query_target_type": refresh_query.target_type.value,
                "impact_query_target": refresh_query.target,
                "parsed_file_count": parse_record["counts"]["parsed_file_count"],
                "reused_file_count": parse_record["counts"]["reused_file_count"],
            },
            metric_rows=[
                _metric_row("refresh-parse-build-latency", "latency", "ms", parse_latency_ms),
                _metric_row("refresh-symbol-build-latency", "latency", "ms", symbol_latency_ms),
                _metric_row("refresh-impact-analyze-latency", "latency", "ms", impact_latency_ms),
                _metric_row(
                    "refresh-parse-parsed-file-count",
                    "custom",
                    "count",
                    float(parse_record["counts"]["parsed_file_count"]),
                ),
                _metric_row(
                    "refresh-parse-reused-file-count",
                    "custom",
                    "count",
                    float(parse_record["counts"]["reused_file_count"]),
                ),
                _metric_row(
                    "refresh-impact-hit-count",
                    "custom",
                    "count",
                    float(_impact_hit_count(impact_response)),
                ),
                _metric_row(
                    "refresh-impact-incremental-count",
                    "custom",
                    "count",
                    1.0 if refresh_mode == "incremental" else 0.0,
                ),
                _metric_row(
                    "refresh-impact-full-compute-count",
                    "custom",
                    "count",
                    1.0 if refresh_mode == "full_rebuild" else 0.0,
                ),
                _metric_row(
                    "refresh-symbol-incremental-count",
                    "custom",
                    "count",
                    1.0 if build_record.get("refresh_mode") == "incremental" else 0.0,
                ),
                _metric_row(
                    "refresh-symbol-full-rebuild-count",
                    "custom",
                    "count",
                    1.0 if build_record.get("refresh_mode") == "full_rebuild" else 0.0,
                ),
            ]
            + _impact_refresh_metric_rows(impact_refresh_stats),
        )

    def _measure_impact_query(
        self,
        *,
        session: _DaemonRepoSession,
        snapshot_id: str,
        query: ImpactQuery,
    ) -> tuple[dict[str, object], float, list[str]]:
        params, notes = _impact_request_payload(query)
        params["repo_id"] = session.repo_id
        params["snapshot_id"] = snapshot_id
        started_at = time.perf_counter()
        try:
            response = self._request("impact_analyze", params)
        except AdapterError as exc:
            retry_target = _retryable_file_target(query, str(exc))
            if retry_target is not None:
                notes.append(
                    (
                        "impact target resolution fell back to backing file "
                        f"{retry_target} after daemon target-not-found"
                    )
                )
                fallback_params = {
                    **params,
                    "target": {
                        "target_kind": "file",
                        "path": retry_target,
                    },
                }
                try:
                    response = self._request("impact_analyze", fallback_params)
                    params = fallback_params
                except AdapterError as fallback_exc:
                    if "impact_target_not_found" not in str(fallback_exc):
                        raise
                    notes.append("impact target remained unresolved after file fallback")
                    response = _empty_impact_response(params, str(fallback_exc))
            elif "impact_target_not_found" in str(exc):
                notes.append("impact target was not resolved by the current daemon contract")
                response = _empty_impact_response(params, str(exc))
            else:
                raise
        latency_ms = (time.perf_counter() - started_at) * 1000.0
        response_manifest = dict(response.get("manifest") or {})
        notes.extend(
            [
                f"snapshot_id={snapshot_id}",
                f"refresh_mode={response_manifest.get('refresh_mode') or 'unknown'}",
                (
                    "loaded_from_existing_build="
                    f"{bool(response_manifest.get('loaded_from_existing_build', False))}"
                ),
            ]
        )
        return response, latency_ms, notes


def load_bundle_support_file(path: Path) -> dict[str, object]:
    """Load a JSON support file from a corpus bundle."""
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise AdapterError(f"Bundle support file does not exist: {path}") from exc
    except json.JSONDecodeError as exc:
        raise AdapterError(f"Bundle support file is not valid JSON: {path}") from exc


def _coerce_str(value: object, field_name: str) -> str:
    if not isinstance(value, str) or not value:
        raise AdapterError(f"Expected non-empty string for {field_name}.")
    return value


def _coerce_optional_str(value: object) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not value:
        return None
    return value


def _metric_row(metric_name: str, metric_kind: str, unit: str, value: float) -> dict[str, object]:
    return {
        "metric_name": metric_name,
        "metric_kind": metric_kind,
        "unit": unit,
        "value": value,
        "tags": {},
    }


def _compact_parse_build(response: dict[str, object]) -> dict[str, object]:
    build = dict(response.get("build", {}))
    counts = dict(build.get("counts", {}))
    return {
        "build_id": build.get("build_id"),
        "loaded_from_existing_build": build.get("loaded_from_existing_build", False),
        "counts": {
            "planned_file_count": counts.get("planned_file_count"),
            "parsed_file_count": counts.get("parsed_file_count"),
            "reused_file_count": counts.get("reused_file_count"),
            "skipped_file_count": counts.get("skipped_file_count"),
            "diagnostic_count": counts.get("diagnostic_count"),
        },
    }


def _compact_symbol_build(response: dict[str, object]) -> dict[str, object]:
    build = dict(response.get("build", {}))
    stats = dict(build.get("stats", {}))
    return {
        "build_id": build.get("build_id"),
        "refresh_mode": build.get("refresh_mode"),
        "fallback_reason": build.get("fallback_reason"),
        "loaded_from_existing_build": build.get("loaded_from_existing_build", False),
        "stats": {
            "file_count": stats.get("file_count"),
            "symbol_count": stats.get("symbol_count"),
            "occurrence_count": stats.get("occurrence_count"),
            "edge_count": stats.get("edge_count"),
            "diagnostic_count": stats.get("diagnostic_count"),
        },
    }


def _compact_impact_status(response: dict[str, object]) -> dict[str, object]:
    capabilities = dict(response.get("capabilities", {}))
    manifest = dict(response.get("manifest") or {})
    return {
        "state": response.get("state"),
        "capabilities": {
            "status": capabilities.get("status"),
            "analyze": capabilities.get("analyze"),
            "explain": capabilities.get("explain"),
            "materialized_store": capabilities.get("materialized_store"),
        },
        "manifest": _compact_impact_manifest(manifest),
        "diagnostic_count": len(response.get("diagnostics", [])),
    }


def _compact_impact_analyze(response: dict[str, object]) -> dict[str, object]:
    summary = dict(response.get("summary", {}))
    stats = dict(response.get("stats", {}))
    manifest = dict(response.get("manifest") or {})
    return {
        "change_hint": response.get("change_hint"),
        "target": response.get("target"),
        "summary": {
            "direct_count": summary.get("direct_count"),
            "transitive_count": summary.get("transitive_count"),
            "certainty_counts": summary.get("certainty_counts"),
        },
        "stats": {
            "nodes_visited": stats.get("nodes_visited"),
            "edges_traversed": stats.get("edges_traversed"),
            "depth_reached": stats.get("depth_reached"),
            "candidates_considered": stats.get("candidates_considered"),
            "elapsed_ms": stats.get("elapsed_ms"),
            "cutoffs_triggered": stats.get("cutoffs_triggered", []),
        },
        "hit_count": _impact_hit_count(response),
        "manifest": _compact_impact_manifest(manifest),
        "diagnostic_count": len(response.get("diagnostics", [])),
    }


def _compact_impact_manifest(manifest: dict[str, object]) -> dict[str, object]:
    refresh_stats = dict(manifest.get("refresh_stats", {}))
    return {
        "build_id": manifest.get("build_id"),
        "refresh_mode": manifest.get("refresh_mode"),
        "fallback_reason": manifest.get("fallback_reason"),
        "loaded_from_existing_build": manifest.get("loaded_from_existing_build", False),
        "storage": manifest.get("storage"),
        "refresh_stats": {
            "mode": refresh_stats.get("mode"),
            "trigger": refresh_stats.get("trigger"),
            "files_touched": refresh_stats.get("files_touched"),
            "entities_recomputed": refresh_stats.get("entities_recomputed"),
            "edges_refreshed": refresh_stats.get("edges_refreshed"),
            "elapsed_ms": refresh_stats.get("elapsed_ms"),
        },
    }


def _impact_hit_count(response: dict[str, object]) -> int:
    total = 0
    for group in response.get("groups", []):
        if not isinstance(group, dict):
            continue
        hits = group.get("hits", [])
        if isinstance(hits, list):
            total += len(hits)
    return total


def _impact_request_payload(query: ImpactQuery) -> tuple[dict[str, object], list[str]]:
    target_kind, target_payload, notes = _resolve_impact_target(query)
    if target_kind == "symbol":
        target = {
            "target_kind": "symbol",
            "value": target_payload,
            "symbol_id": None,
            "path": None,
        }
    else:
        target = {"target_kind": "file", "path": target_payload}
    return (
        {
            "target": target,
            "change_hint": query.change_hint.value,
            "limit": query.limit,
            "include_transitive": True,
            "include_reason_paths": True,
            "max_transitive_depth": None,
            "max_nodes_visited": None,
            "max_edges_traversed": None,
            "max_candidates_considered": None,
        },
        notes,
    )


def _resolve_impact_target(query: ImpactQuery) -> tuple[str, str, list[str]]:
    if query.target_type.value == "symbol":
        return "symbol", query.target, []
    if query.target_type.value == "file":
        return "file", query.target, []
    backing_path = query.target.split("#", 1)[0]
    if not backing_path:
        raise AdapterError(
            f"Impact query {query.query_id} could not be degraded to a file-backed target."
        )
    return (
        "file",
        backing_path,
        [
            (
                f"target_type={query.target_type.value} is not yet public in the daemon contract; "
                f"degrading to backing file {backing_path}"
            )
        ],
    )


def _normalize_impact_hits(response: dict[str, object]) -> list[QueryHit]:
    raw_hits: list[dict[str, object]] = []
    for group in response.get("groups", []):
        if not isinstance(group, dict):
            continue
        for hit in group.get("hits", []):
            if isinstance(hit, dict):
                raw_hits.append(hit)
    raw_hits.sort(key=lambda hit: int(hit.get("rank", 0)))
    hits: list[QueryHit] = []
    seen_paths: set[str] = set()
    for hit in raw_hits:
        entity = hit.get("entity")
        if not isinstance(entity, dict):
            continue
        path, symbol = _impact_entity_path_and_symbol(entity)
        if path in seen_paths:
            continue
        seen_paths.add(path)
        hits.append(
            QueryHit(
                path=path,
                symbol=symbol,
                rank=len(hits) + 1,
                reason=str(hit.get("primary_reason", "impact")),
                score=float(hit.get("score", 0.0)),
            )
        )
    return hits


def _impact_entity_path_and_symbol(entity: dict[str, object]) -> tuple[str, str | None]:
    entity_kind = entity.get("entity_kind")
    if entity_kind == "symbol":
        return (
            _coerce_str(entity.get("path"), "impact.entity.path"),
            _coerce_optional_str(entity.get("display_name")),
        )
    if entity_kind == "file":
        return _coerce_str(entity.get("path"), "impact.entity.path"), None
    if entity_kind == "package":
        package_root = _coerce_optional_str(entity.get("package_root"))
        package_name = _coerce_optional_str(entity.get("package_name"))
        return (
            package_root or f"package:{package_name or 'unknown'}",
            package_name,
        )
    if entity_kind == "test":
        return (
            _coerce_str(entity.get("path"), "impact.entity.path"),
            _coerce_optional_str(entity.get("display_name")),
        )
    raise AdapterError(f"Unsupported impact entity kind in daemon response: {entity_kind!r}")


def _select_representative_impact_query(bundle: CorpusBundle) -> ImpactQuery:
    impact_queries = [
        query
        for pack in bundle.query_packs
        for query in pack.queries
        if isinstance(query, ImpactQuery)
    ]
    if not impact_queries:
        raise AdapterError("Corpus bundle does not include any impact queries.")
    for query in impact_queries:
        if "hero" in query.tags:
            return query
    for query in impact_queries:
        if query.query_id == "impact-invalidate-session":
            return query
    return impact_queries[0]


def _refresh_query_for_scenario(bundle: CorpusBundle, scenario: dict[str, object]) -> ImpactQuery:
    for query_id in scenario.get("expected_changed_queries", []):
        if not isinstance(query_id, str):
            continue
        for pack in bundle.query_packs:
            for query in pack.queries:
                if isinstance(query, ImpactQuery) and query.query_id == query_id:
                    return query
    return _select_representative_impact_query(bundle)


def _impact_refresh_metric_rows(refresh_stats: dict[str, object]) -> list[dict[str, object]]:
    metric_rows: list[dict[str, object]] = []
    if refresh_stats.get("elapsed_ms") is not None:
        metric_rows.append(
            _metric_row(
                "refresh-impact-refresh-elapsed-ms",
                "latency",
                "ms",
                float(refresh_stats["elapsed_ms"]),
            )
        )
    for field_name, metric_name in (
        ("files_touched", "refresh-impact-files-touched"),
        ("entities_recomputed", "refresh-impact-entities-recomputed"),
        ("edges_refreshed", "refresh-impact-edges-refreshed"),
    ):
        if refresh_stats.get(field_name) is not None:
            metric_rows.append(
                _metric_row(
                    metric_name,
                    "custom",
                    "count",
                    float(refresh_stats[field_name]),
                )
            )
    return metric_rows


def _retryable_file_target(query: ImpactQuery, error_message: str) -> str | None:
    if "impact_target_not_found" not in error_message:
        return None
    if query.target_type.value != "symbol":
        return None
    backing_path = query.target.split("#", 1)[0]
    return backing_path or None


def _empty_impact_response(
    params: dict[str, object],
    error_message: str,
) -> dict[str, object]:
    return {
        "repo_id": params.get("repo_id"),
        "snapshot_id": params.get("snapshot_id"),
        "target": params.get("target"),
        "change_hint": params.get("change_hint"),
        "summary": {
            "direct_count": 0,
            "transitive_count": 0,
            "certainty_counts": {
                "certain": 0,
                "likely": 0,
                "possible": 0,
            },
        },
        "stats": {
            "nodes_visited": 0,
            "edges_traversed": 0,
            "depth_reached": 0,
            "candidates_considered": 0,
            "elapsed_ms": 0,
            "cutoffs_triggered": [],
        },
        "groups": [],
        "diagnostics": [
            {
                "severity": "warning",
                "code": "impact_target_not_found",
                "message": error_message,
            }
        ],
        "manifest": None,
    }


def _safe_directory_name(raw: str) -> str:
    pieces = [char.lower() if char.isalnum() else "-" for char in raw]
    slug = "".join(pieces).strip("-")
    return slug or "repo"


def _deterministic_latency_ms(query_id: str, query_type: QueryType) -> float:
    base = {
        QueryType.EXACT: 6.0,
        QueryType.SYMBOL: 8.0,
        QueryType.SEMANTIC: 14.0,
        QueryType.IMPACT: 18.0,
    }[query_type]
    offset = sum(ord(char) for char in query_id) % 7
    return base + float(offset)


__all__ = [
    "AdapterError",
    "CorpusBundle",
    "DaemonImpactAdapter",
    "DaemonSymbolAdapter",
    "EngineAdapter",
    "FixtureAdapter",
    "PreparedCorpus",
    "QueryExecutionResult",
    "QueryHit",
    "RefreshExecutionResult",
    "ShellAdapter",
    "load_bundle_support_file",
]
