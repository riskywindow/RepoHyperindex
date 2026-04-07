"""Engine adapter boundary for the Phase 1 Hyperbench harness."""

from __future__ import annotations

import json
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
    "EngineAdapter",
    "FixtureAdapter",
    "PreparedCorpus",
    "QueryExecutionResult",
    "QueryHit",
    "RefreshExecutionResult",
    "ShellAdapter",
    "load_bundle_support_file",
]
