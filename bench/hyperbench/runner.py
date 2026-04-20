"""End-to-end benchmark runner for the Phase 1 Hyperbench harness."""

from __future__ import annotations

import os
import platform
import resource
import subprocess
import sys
import time
from collections import Counter
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

from pydantic import ValidationError

from hyperbench.adapter import (
    CorpusBundle,
    DaemonImpactAdapter,
    DaemonSemanticAdapter,
    DaemonSymbolAdapter,
    EngineAdapter,
    FixtureAdapter,
    PreparedCorpus,
    QueryExecutionResult,
    RefreshExecutionResult,
    ShellAdapter,
    load_bundle_support_file,
)
from hyperbench.metrics import build_metric_samples, summarize_metric_samples
from hyperbench.query_packs import load_query_artifacts, validate_query_artifacts
from hyperbench.report import write_csv, write_json, write_jsonl
from hyperbench.schemas import (
    CorpusManifest,
    ExactQuery,
    GoldenExpectation,
    ImpactQuery,
    QueryPack,
    SemanticQuery,
    SymbolQuery,
)


class RunnerError(RuntimeError):
    """Raised when a benchmark run cannot be loaded or executed."""


@dataclass(frozen=True)
class RunArtifacts:
    """Paths for the machine-readable outputs of one benchmark run."""

    output_dir: Path
    summary_path: Path
    events_path: Path
    metrics_path: Path
    query_results_csv_path: Path
    refresh_results_csv_path: Path
    metric_summaries_csv_path: Path


@dataclass(frozen=True)
class RunResult:
    """Summary of a completed benchmark run."""

    run_id: str
    adapter_name: str
    corpus_id: str
    query_pack_ids: list[str]
    mode: str
    query_count: int
    refresh_scenario_count: int
    artifacts: RunArtifacts


def create_adapter(
    adapter_name: str,
    *,
    engine_bin: str | None = None,
    daemon_build_temperature: str = "cold",
    daemon_workspace_root: str | None = None,
) -> EngineAdapter:
    """Construct an adapter implementation by name."""
    if adapter_name == "fixture":
        return FixtureAdapter()
    if adapter_name == "daemon":
        return DaemonSymbolAdapter(
            engine_bin=engine_bin,
            build_temperature=daemon_build_temperature,
            workspace_root=daemon_workspace_root,
        )
    if adapter_name == "daemon-impact":
        return DaemonImpactAdapter(
            engine_bin=engine_bin,
            build_temperature=daemon_build_temperature,
            workspace_root=daemon_workspace_root,
        )
    if adapter_name == "daemon-semantic":
        return DaemonSemanticAdapter(
            engine_bin=engine_bin,
            build_temperature=daemon_build_temperature,
            workspace_root=daemon_workspace_root,
        )
    if adapter_name == "shell":
        return ShellAdapter(engine_bin=engine_bin)
    raise RunnerError(f"Unsupported adapter '{adapter_name}'.")


def resolve_corpus_path(
    *,
    corpus_path: str | None = None,
    corpus_id: str | None = None,
    corpora_dir: str | Path = "bench/corpora/synthetic",
) -> Path:
    """Resolve a runnable corpus bundle path from either a direct path or corpus id."""
    if corpus_path:
        return Path(corpus_path)
    if corpus_id:
        return Path(corpora_dir) / corpus_id
    raise RunnerError("Either corpus_path or corpus_id is required.")


def load_corpus_bundle(corpus_path: str | Path) -> CorpusBundle:
    """Load a generated corpus bundle from disk."""
    root_dir = Path(corpus_path)
    if not root_dir.exists():
        raise RunnerError(f"Corpus bundle path does not exist: {root_dir}")
    if not root_dir.is_dir():
        raise RunnerError(f"Corpus bundle path must be a directory: {root_dir}")

    manifest_path = _first_existing(
        root_dir / "corpus-manifest.json",
        root_dir / "corpus-manifest.yaml",
        root_dir / "corpus-manifest.yml",
    )
    if manifest_path is None:
        raise RunnerError(f"No corpus manifest found under {root_dir}.")
    try:
        manifest = CorpusManifest.from_path(manifest_path)
    except (ValidationError, ValueError) as exc:
        raise RunnerError(f"Invalid corpus manifest at {manifest_path}: {exc}") from exc

    query_pack_dir = root_dir / "query-packs"
    golden_dir = root_dir / "goldens"
    try:
        query_packs, golden_sets = load_query_artifacts(query_pack_dir, golden_dir)
    except (ValidationError, ValueError) as exc:
        raise RunnerError(
            f"Invalid query-pack or golden-set artifact under {root_dir}: {exc}"
        ) from exc
    if not query_packs or not golden_sets:
        raise RunnerError(
            f"Corpus bundle at {root_dir} must include generated query-packs/ and goldens/."
        )
    try:
        validate_query_artifacts(query_packs, golden_sets)
    except ValueError as exc:
        raise RunnerError(str(exc)) from exc
    _validate_bundle_manifest_alignment(
        manifest=manifest,
        query_packs=query_packs,
        golden_sets=golden_sets,
        root_dir=root_dir,
    )

    ground_truth = load_bundle_support_file(root_dir / "ground_truth.json")
    edit_scenarios_document = load_bundle_support_file(root_dir / "edit_scenarios.json")
    scenarios = edit_scenarios_document.get("scenarios", [])
    if not isinstance(scenarios, list):
        raise RunnerError("edit_scenarios.json must contain a list under 'scenarios'.")

    goldens_by_pack_id = {golden.query_pack_id: golden for golden in golden_sets}
    expectations_by_query_id: dict[str, GoldenExpectation] = {}
    for golden in golden_sets:
        for expectation in golden.expectations:
            expectations_by_query_id[expectation.query_id] = expectation

    return CorpusBundle(
        root_dir=root_dir,
        manifest=manifest,
        query_packs=query_packs,
        golden_sets_by_pack_id=goldens_by_pack_id,
        expectations_by_query_id=expectations_by_query_id,
        ground_truth=ground_truth,
        edit_scenarios=scenarios,
    )


def run_benchmark(
    *,
    adapter: EngineAdapter,
    corpus_bundle: CorpusBundle,
    output_dir: str | Path,
    mode: str = "full",
    query_pack_ids: list[str] | None = None,
) -> RunResult:
    """Execute the end-to-end benchmark harness for the selected corpus bundle."""
    try:
        if mode not in {"full", "smoke"}:
            raise RunnerError("mode must be either 'full' or 'smoke'")

        selected_query_packs = _select_query_packs(corpus_bundle, query_pack_ids, mode)
        selected_scenarios = (
            corpus_bundle.edit_scenarios
            if mode == "full"
            else corpus_bundle.edit_scenarios[: min(2, len(corpus_bundle.edit_scenarios))]
        )
        if not selected_query_packs:
            raise RunnerError("No query packs were selected for the benchmark run.")

        output_root = Path(output_dir)
        output_root.mkdir(parents=True, exist_ok=True)
        run_started_at = datetime.now(UTC)
        wall_clock_start = time.perf_counter()
        run_metadata = _collect_run_metadata(
            adapter_name=adapter.name,
            corpus_bundle=corpus_bundle,
            mode=mode,
            query_pack_ids=[pack.query_pack_id for pack in selected_query_packs],
        )

        prepared = adapter.prepare_corpus(corpus_bundle)
        query_rows: list[dict[str, object]] = []
        refresh_rows: list[dict[str, object]] = []
        event_rows: list[dict[str, object]] = [
            {
                "event_type": "run-metadata",
                "run_started_at": run_started_at.isoformat(),
                "metadata": run_metadata,
            },
            {
                "event_type": "prepare",
                "adapter": adapter.name,
                "corpus_id": corpus_bundle.manifest.corpus_id,
                "latency_ms": prepared.latency_ms,
                "notes": prepared.notes,
                "metadata": prepared.metadata,
            },
        ]
        selected_query_ids = {
            query.query_id for pack in selected_query_packs for query in pack.queries
        }
        run_metric_rows: list[dict[str, object]] = list(prepared.metric_rows)

        for pack in selected_query_packs:
            golden = corpus_bundle.golden_sets_by_pack_id.get(pack.query_pack_id)
            if golden is None:
                raise RunnerError(f"Missing golden set for query pack '{pack.query_pack_id}'.")
            for query in pack.queries:
                expectation = corpus_bundle.expectations_by_query_id[query.query_id]
                result = _execute_query(adapter, corpus_bundle, query)
                evaluated_row = _evaluate_query_result(
                    query, result, expectation, pack.query_pack_id
                )
                query_rows.append(evaluated_row)
                event_rows.append(
                    {
                        "event_type": "query",
                        "adapter": adapter.name,
                        "corpus_id": corpus_bundle.manifest.corpus_id,
                        "query_pack_id": pack.query_pack_id,
                        "query_id": query.query_id,
                        "query_type": query.type,
                        "latency_ms": result.latency_ms,
                        "passed": evaluated_row["passed"],
                        "actual_hit_count": evaluated_row["actual_hit_count"],
                        "expected_hit_count": evaluated_row["expected_hit_count"],
                        "notes": result.notes,
                    }
                )

        for scenario in selected_scenarios:
            refresh_result = adapter.run_incremental_refresh(corpus_bundle, scenario)
            refresh_row = _normalize_refresh_row(refresh_result, selected_query_ids)
            refresh_rows.append(refresh_row)
            run_metric_rows.extend(refresh_result.metric_rows)
            event_rows.append(
                {
                    "event_type": "refresh",
                    "adapter": adapter.name,
                    "corpus_id": corpus_bundle.manifest.corpus_id,
                    **refresh_row,
                    "metadata": refresh_result.metadata,
                }
            )

        wall_clock_ms = (time.perf_counter() - wall_clock_start) * 1000.0
        peak_rss_bytes = _peak_rss_bytes()
        run_metric_rows.append(
            {
                "metric_name": "wall-clock",
                "metric_kind": "latency",
                "unit": "ms",
                "value": wall_clock_ms,
                "tags": {"scope": "run"},
            }
        )
        if peak_rss_bytes is not None:
            run_metric_rows.append(
                {
                    "metric_name": "peak-rss",
                    "metric_kind": "system",
                    "unit": "bytes",
                    "value": float(peak_rss_bytes),
                    "tags": {"scope": "run"},
                }
            )

        metric_samples = build_metric_samples(
            query_rows=query_rows,
            refresh_rows=refresh_rows,
            run_metric_rows=run_metric_rows,
        )
        metric_summaries = summarize_metric_samples(metric_samples)

        run_id = _build_run_id(adapter.name, corpus_bundle.manifest.corpus_id, mode)
        summary_payload = _build_summary_payload(
            run_id=run_id,
            run_started_at=run_started_at,
            adapter_name=adapter.name,
            corpus_bundle=corpus_bundle,
            prepared=prepared,
            query_rows=query_rows,
            refresh_rows=refresh_rows,
            metric_summaries=metric_summaries,
            mode=mode,
            selected_query_packs=selected_query_packs,
            run_metadata=run_metadata,
            peak_rss_bytes=peak_rss_bytes,
            wall_clock_ms=wall_clock_ms,
            output_disk_usage_bytes=None,
        )
        event_rows.append(
            {
                "event_type": "run-summary",
                "run_id": run_id,
                "adapter": adapter.name,
                "corpus_id": corpus_bundle.manifest.corpus_id,
                "query_count": len(query_rows),
                "refresh_scenario_count": len(refresh_rows),
                "query_pass_count": summary_payload["query_pass_count"],
            }
        )

        artifacts = RunArtifacts(
            output_dir=output_root,
            summary_path=output_root / "summary.json",
            events_path=output_root / "events.jsonl",
            metrics_path=output_root / "metrics.jsonl",
            query_results_csv_path=output_root / "query_results.csv",
            refresh_results_csv_path=output_root / "refresh_results.csv",
            metric_summaries_csv_path=output_root / "metric_summaries.csv",
        )
        write_jsonl(artifacts.events_path, event_rows)
        write_csv(
            artifacts.query_results_csv_path,
            query_rows,
            [
                "query_pack_id",
                "query_id",
                "query_type",
                "latency_ms",
                "expected_hit_count",
                "actual_hit_count",
                "matched_hit_count",
                "top_hit_path",
                "expected_top_hit_path",
                "passed",
                "notes",
            ],
        )
        write_csv(
            artifacts.refresh_results_csv_path,
            refresh_rows,
            [
                "scenario_id",
                "latency_ms",
                "changed_query_count",
                "changed_queries",
                "refresh_mode",
                "fallback_reason",
                "loaded_from_existing_build",
                "parse_build_latency_ms",
                "symbol_build_latency_ms",
                "impact_analyze_latency_ms",
                "symbol_refresh_mode",
                "symbol_fallback_reason",
                "symbol_loaded_from_existing_build",
                "impact_refresh_mode",
                "impact_fallback_reason",
                "impact_loaded_from_existing_build",
                "impact_refresh_elapsed_ms",
                "impact_refresh_files_touched",
                "impact_refresh_entities_recomputed",
                "impact_refresh_edges_refreshed",
                "impact_query_id",
                "impact_query_target_type",
                "impact_query_target",
                "semantic_build_latency_ms",
                "semantic_query_latency_ms",
                "semantic_refresh_mode",
                "semantic_fallback_reason",
                "semantic_loaded_from_existing_build",
                "semantic_refresh_elapsed_ms",
                "semantic_refresh_files_touched",
                "semantic_refresh_chunks_rebuilt",
                "semantic_refresh_embeddings_regenerated",
                "semantic_refresh_vector_entries_added",
                "semantic_refresh_vector_entries_updated",
                "semantic_refresh_vector_entries_removed",
                "semantic_query_id",
                "target_path",
                "notes",
            ],
        )
        write_csv(
            artifacts.metric_summaries_csv_path,
            [
                {
                    "metric_name": summary.metric_name,
                    "metric_kind": summary.metric_kind.value,
                    "unit": summary.unit.value,
                    "sample_count": summary.sample_count,
                    "minimum": summary.minimum,
                    "maximum": summary.maximum,
                    "mean": summary.mean,
                    "p50": summary.p50,
                    "p95": summary.p95,
                    "p99": summary.p99,
                }
                for summary in metric_summaries
            ],
            [
                "metric_name",
                "metric_kind",
                "unit",
                "sample_count",
                "minimum",
                "maximum",
                "mean",
                "p50",
                "p95",
                "p99",
            ],
        )
        write_json(artifacts.summary_path, summary_payload)
        output_disk_usage_bytes = _directory_size_bytes(output_root)
        run_metric_rows.append(
            {
                "metric_name": "output-disk-usage",
                "metric_kind": "system",
                "unit": "bytes",
                "value": float(output_disk_usage_bytes),
                "tags": {"scope": "run"},
            }
        )
        metric_samples = build_metric_samples(
            query_rows=query_rows,
            refresh_rows=refresh_rows,
            run_metric_rows=run_metric_rows,
        )
        metric_summaries = summarize_metric_samples(metric_samples)
        metrics_rows = [
            {
                "record_type": "sample",
                "metric_name": sample.metric_name,
                "metric_kind": sample.metric_kind.value,
                "unit": sample.unit.value,
                "value": sample.value,
                "tags": sample.tags,
            }
            for sample in metric_samples
        ] + [
            {
                "record_type": "summary",
                "metric_name": summary.metric_name,
                "metric_kind": summary.metric_kind.value,
                "unit": summary.unit.value,
                "sample_count": summary.sample_count,
                "minimum": summary.minimum,
                "maximum": summary.maximum,
                "mean": summary.mean,
                "p50": summary.p50,
                "p95": summary.p95,
                "p99": summary.p99,
            }
            for summary in metric_summaries
        ]
        write_jsonl(artifacts.metrics_path, metrics_rows)
        write_csv(
            artifacts.metric_summaries_csv_path,
            [
                {
                    "metric_name": summary.metric_name,
                    "metric_kind": summary.metric_kind.value,
                    "unit": summary.unit.value,
                    "sample_count": summary.sample_count,
                    "minimum": summary.minimum,
                    "maximum": summary.maximum,
                    "mean": summary.mean,
                    "p50": summary.p50,
                    "p95": summary.p95,
                    "p99": summary.p99,
                }
                for summary in metric_summaries
            ],
            [
                "metric_name",
                "metric_kind",
                "unit",
                "sample_count",
                "minimum",
                "maximum",
                "mean",
                "p50",
                "p95",
                "p99",
            ],
        )
        summary_payload = _build_summary_payload(
            run_id=run_id,
            run_started_at=run_started_at,
            adapter_name=adapter.name,
            corpus_bundle=corpus_bundle,
            prepared=prepared,
            query_rows=query_rows,
            refresh_rows=refresh_rows,
            metric_summaries=metric_summaries,
            mode=mode,
            selected_query_packs=selected_query_packs,
            run_metadata=run_metadata,
            peak_rss_bytes=peak_rss_bytes,
            wall_clock_ms=wall_clock_ms,
            output_disk_usage_bytes=output_disk_usage_bytes,
        )
        write_json(artifacts.summary_path, summary_payload)

        return RunResult(
            run_id=run_id,
            adapter_name=adapter.name,
            corpus_id=corpus_bundle.manifest.corpus_id,
            query_pack_ids=[pack.query_pack_id for pack in selected_query_packs],
            mode=mode,
            query_count=len(query_rows),
            refresh_scenario_count=len(refresh_rows),
            artifacts=artifacts,
        )
    finally:
        adapter.close()


def _select_query_packs(
    corpus_bundle: CorpusBundle,
    query_pack_ids: list[str] | None,
    mode: str,
) -> list[QueryPack]:
    pack_map = {pack.query_pack_id: pack for pack in corpus_bundle.query_packs}
    missing = sorted(query_id for query_id in (query_pack_ids or []) if query_id not in pack_map)
    if missing:
        raise RunnerError("Unknown query_pack_id values: " + ", ".join(missing))
    selected = (
        [pack_map[query_pack_id] for query_pack_id in query_pack_ids]
        if query_pack_ids
        else list(corpus_bundle.query_packs)
    )

    if mode == "full":
        return selected

    smoke_packs: list[QueryPack] = []
    for pack in selected:
        smoke_packs.append(pack.model_copy(update={"queries": [_select_smoke_query(pack)]}))
    return smoke_packs


def _select_smoke_query(pack: QueryPack):
    for query in pack.queries:
        if "hero" in query.tags:
            return query
    for query in pack.queries:
        if "invalidate-session" in query.query_id or "hero" in query.query_id:
            return query
    return pack.queries[0]


def _execute_query(
    adapter: EngineAdapter,
    bundle: CorpusBundle,
    query: ExactQuery | SymbolQuery | SemanticQuery | ImpactQuery,
) -> QueryExecutionResult:
    if isinstance(query, ExactQuery):
        return adapter.execute_exact_query(bundle, query)
    if isinstance(query, SymbolQuery):
        return adapter.execute_symbol_query(bundle, query)
    if isinstance(query, SemanticQuery):
        return adapter.execute_semantic_query(bundle, query)
    if isinstance(query, ImpactQuery):
        return adapter.execute_impact_query(bundle, query)
    raise RunnerError(f"Unsupported query model: {type(query)!r}")


def _evaluate_query_result(
    query: ExactQuery | SymbolQuery | SemanticQuery | ImpactQuery,
    result: QueryExecutionResult,
    expectation: GoldenExpectation,
    query_pack_id: str,
) -> dict[str, object]:
    hits_by_path = {hit.path: hit for hit in result.hits}
    matched_hit_count = 0
    for expected_hit in expectation.expected_hits:
        actual_hit = hits_by_path.get(expected_hit.path)
        if actual_hit is None:
            continue
        if actual_hit.rank <= expected_hit.rank_max:
            matched_hit_count += 1

    top_hit_path = result.hits[0].path if result.hits else ""
    expected_top_hit_path = (
        expectation.expected_top_hit.path if expectation.expected_top_hit else ""
    )
    top_hit_pass = (
        not expectation.expected_top_hit or top_hit_path == expectation.expected_top_hit.path
    )
    passed = matched_hit_count == len(expectation.expected_hits) and top_hit_pass
    return {
        "query_pack_id": query_pack_id,
        "query_id": query.query_id,
        "query_type": query.type,
        "latency_ms": result.latency_ms,
        "expected_hit_count": len(expectation.expected_hits),
        "actual_hit_count": len(result.hits),
        "matched_hit_count": matched_hit_count,
        "top_hit_path": top_hit_path,
        "expected_top_hit_path": expected_top_hit_path,
        "passed": passed,
        "notes": " | ".join(result.notes),
    }


def _normalize_refresh_row(
    result: RefreshExecutionResult,
    selected_query_ids: set[str],
) -> dict[str, object]:
    changed_queries = [
        query_id for query_id in result.changed_queries if query_id in selected_query_ids
    ]
    metadata = dict(result.metadata)
    return {
        "scenario_id": result.scenario_id,
        "latency_ms": result.latency_ms,
        "changed_query_count": len(changed_queries),
        "changed_queries": ",".join(changed_queries),
        "refresh_mode": metadata.get("refresh_mode", ""),
        "fallback_reason": metadata.get("fallback_reason", ""),
        "loaded_from_existing_build": metadata.get("loaded_from_existing_build", False),
        "parse_build_latency_ms": metadata.get("parse_build_latency_ms", ""),
        "symbol_build_latency_ms": metadata.get("symbol_build_latency_ms", ""),
        "impact_analyze_latency_ms": metadata.get("impact_analyze_latency_ms", ""),
        "symbol_refresh_mode": metadata.get("symbol_refresh_mode", ""),
        "symbol_fallback_reason": metadata.get("symbol_fallback_reason", ""),
        "symbol_loaded_from_existing_build": metadata.get(
            "symbol_loaded_from_existing_build", False
        ),
        "impact_refresh_mode": metadata.get("impact_refresh_mode", ""),
        "impact_fallback_reason": metadata.get("impact_fallback_reason", ""),
        "impact_loaded_from_existing_build": metadata.get(
            "impact_loaded_from_existing_build", False
        ),
        "impact_refresh_elapsed_ms": metadata.get("impact_refresh_elapsed_ms", ""),
        "impact_refresh_files_touched": metadata.get("impact_refresh_files_touched", ""),
        "impact_refresh_entities_recomputed": metadata.get(
            "impact_refresh_entities_recomputed", ""
        ),
        "impact_refresh_edges_refreshed": metadata.get("impact_refresh_edges_refreshed", ""),
        "impact_query_id": metadata.get("impact_query_id", ""),
        "impact_query_target_type": metadata.get("impact_query_target_type", ""),
        "impact_query_target": metadata.get("impact_query_target", ""),
        "semantic_build_latency_ms": metadata.get("semantic_build_latency_ms", ""),
        "semantic_query_latency_ms": metadata.get("semantic_query_latency_ms", ""),
        "semantic_refresh_mode": metadata.get("semantic_refresh_mode", ""),
        "semantic_fallback_reason": metadata.get("semantic_fallback_reason", ""),
        "semantic_loaded_from_existing_build": metadata.get(
            "semantic_loaded_from_existing_build", False
        ),
        "semantic_refresh_elapsed_ms": metadata.get("semantic_refresh_elapsed_ms", ""),
        "semantic_refresh_files_touched": metadata.get("semantic_refresh_files_touched", ""),
        "semantic_refresh_chunks_rebuilt": metadata.get("semantic_refresh_chunks_rebuilt", ""),
        "semantic_refresh_embeddings_regenerated": metadata.get(
            "semantic_refresh_embeddings_regenerated", ""
        ),
        "semantic_refresh_vector_entries_added": metadata.get(
            "semantic_refresh_vector_entries_added", ""
        ),
        "semantic_refresh_vector_entries_updated": metadata.get(
            "semantic_refresh_vector_entries_updated", ""
        ),
        "semantic_refresh_vector_entries_removed": metadata.get(
            "semantic_refresh_vector_entries_removed", ""
        ),
        "semantic_query_id": metadata.get("semantic_query_id", ""),
        "target_path": metadata.get("target_path", ""),
        "notes": " | ".join(result.notes),
    }


def _build_summary_payload(
    *,
    run_id: str,
    run_started_at: datetime,
    adapter_name: str,
    corpus_bundle: CorpusBundle,
    prepared: PreparedCorpus,
    query_rows: list[dict[str, object]],
    refresh_rows: list[dict[str, object]],
    metric_summaries: list,
    mode: str,
    selected_query_packs: list[QueryPack],
    run_metadata: dict[str, object],
    peak_rss_bytes: int | None,
    wall_clock_ms: float,
    output_disk_usage_bytes: int | None,
) -> dict[str, object]:
    type_counter = Counter(str(row["query_type"]) for row in query_rows)
    pass_counter = sum(1 for row in query_rows if bool(row["passed"]))
    query_pass_rate = (pass_counter / len(query_rows)) if query_rows else 0.0
    refresh_mode_counter = Counter(
        str(row["refresh_mode"]) for row in refresh_rows if str(row.get("refresh_mode") or "")
    )
    fallback_count = sum(1 for row in refresh_rows if row.get("fallback_reason"))
    return {
        "schema_version": "1",
        "run_id": run_id,
        "created_at": datetime.now(UTC).isoformat(),
        "run_started_at": run_started_at.isoformat(),
        "adapter": adapter_name,
        "mode": mode,
        "corpus": {
            "corpus_id": corpus_bundle.manifest.corpus_id,
            "display_name": corpus_bundle.manifest.display_name,
            "bundle_path": str(corpus_bundle.root_dir),
        },
        "run_metadata": run_metadata,
        "query_pack_ids": [pack.query_pack_id for pack in selected_query_packs],
        "query_count": len(query_rows),
        "query_pass_count": pass_counter,
        "query_pass_rate": query_pass_rate,
        "refresh_scenario_count": len(refresh_rows),
        "query_counts_by_type": dict(sorted(type_counter.items())),
        "benchmark_dimensions": {
            "query_types": sorted(type_counter),
            "adapter_transport": prepared.metadata.get("transport"),
            "engine_backend": prepared.metadata.get("engine_backend"),
            "build_temperature": prepared.metadata.get("build_temperature"),
        },
        "prepare": {
            "latency_ms": prepared.latency_ms,
            "notes": prepared.notes,
            "metadata": prepared.metadata,
        },
        "refresh_summary": {
            "scenario_count": len(refresh_rows),
            "mode_counts": dict(sorted(refresh_mode_counter.items())),
            "fallback_count": fallback_count,
        },
        "instrumentation": {
            "wall_clock_ms": wall_clock_ms,
            "query_latency_p50_ms": _summary_stat(metric_summaries, "query-latency", "p50"),
            "query_latency_p95_ms": _summary_stat(metric_summaries, "query-latency", "p95"),
            "refresh_latency_p50_ms": _summary_stat(metric_summaries, "refresh-latency", "p50"),
            "refresh_latency_p95_ms": _summary_stat(metric_summaries, "refresh-latency", "p95"),
            "peak_rss_bytes": peak_rss_bytes,
            "output_disk_usage_bytes": output_disk_usage_bytes,
        },
        "metric_summaries": [
            {
                "metric_name": summary.metric_name,
                "metric_kind": summary.metric_kind.value,
                "unit": summary.unit.value,
                "sample_count": summary.sample_count,
                "minimum": summary.minimum,
                "maximum": summary.maximum,
                "mean": summary.mean,
                "p50": summary.p50,
                "p95": summary.p95,
                "p99": summary.p99,
            }
            for summary in metric_summaries
        ],
        "metric_summary_count": len(metric_summaries),
    }


def _build_run_id(adapter_name: str, corpus_id: str, mode: str) -> str:
    timestamp = datetime.now(UTC).strftime("%Y%m%d%H%M%S%f")
    return f"{adapter_name}-{corpus_id}-{mode}-{timestamp}"


def _first_existing(*paths: Path) -> Path | None:
    for path in paths:
        if path.exists():
            return path
    return None


def _validate_bundle_manifest_alignment(
    *,
    manifest: CorpusManifest,
    query_packs: list[QueryPack],
    golden_sets: list,
    root_dir: Path,
) -> None:
    pack_ids = sorted(pack.query_pack_id for pack in query_packs)
    if sorted(manifest.query_pack_ids) != pack_ids:
        raise RunnerError(
            f"Corpus bundle manifest at {root_dir} is out of sync with query-packs/: "
            "manifest query_pack_ids do not match the generated bundle artifacts."
        )

    golden_ids = sorted(golden.golden_set_id for golden in golden_sets)
    if sorted(manifest.golden_set_ids) != golden_ids:
        raise RunnerError(
            f"Corpus bundle manifest at {root_dir} is out of sync with goldens/: "
            "manifest golden_set_ids do not match the generated bundle artifacts."
        )

    mismatched_pack_ids = sorted(
        pack.query_pack_id for pack in query_packs if pack.corpus_id != manifest.corpus_id
    )
    if mismatched_pack_ids:
        raise RunnerError(
            f"Corpus bundle at {root_dir} includes query packs for the wrong corpus_id: "
            + ", ".join(mismatched_pack_ids)
        )

    mismatched_golden_ids = sorted(
        golden.golden_set_id for golden in golden_sets if golden.corpus_id != manifest.corpus_id
    )
    if mismatched_golden_ids:
        raise RunnerError(
            f"Corpus bundle at {root_dir} includes golden sets for the wrong corpus_id: "
            + ", ".join(mismatched_golden_ids)
        )


def _collect_run_metadata(
    *,
    adapter_name: str,
    corpus_bundle: CorpusBundle,
    mode: str,
    query_pack_ids: list[str],
) -> dict[str, object]:
    return {
        "os": {
            "platform": platform.platform(),
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
        },
        "cpu": {
            "processor": platform.processor() or "unknown",
            "logical_cores": os.cpu_count(),
        },
        "ram_bytes": _ram_bytes(),
        "tool_versions": {
            "python": sys.version.split()[0],
            "hyperbench": "0.1.0",
            "uv": _command_version(["uv", "--version"]),
            "git": _command_version(["git", "--version"]),
        },
        "git_sha": _git_sha(),
        "adapter_name": adapter_name,
        "corpus_id": corpus_bundle.manifest.corpus_id,
        "mode": mode,
        "query_pack_ids": query_pack_ids,
    }


def _ram_bytes() -> int | None:
    try:
        page_size = os.sysconf("SC_PAGE_SIZE")
        page_count = os.sysconf("SC_PHYS_PAGES")
    except (AttributeError, OSError, ValueError):
        return None
    if not isinstance(page_size, int) or not isinstance(page_count, int):
        return None
    return page_size * page_count


def _command_version(command: list[str]) -> str | None:
    try:
        completed = subprocess.run(
            command,
            check=True,
            capture_output=True,
            text=True,
        )
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
    return completed.stdout.strip() or completed.stderr.strip() or None


def _git_sha() -> str | None:
    repo_root = Path(__file__).resolve().parents[2]
    try:
        completed = subprocess.run(
            ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
    return completed.stdout.strip() or None


def _peak_rss_bytes() -> int | None:
    try:
        usage = resource.getrusage(resource.RUSAGE_SELF)
    except (AttributeError, ValueError):
        return None
    peak = int(usage.ru_maxrss)
    if peak <= 0:
        return None
    if platform.system() == "Darwin":
        return peak
    return peak * 1024


def _directory_size_bytes(path: Path) -> int:
    total = 0
    for child in path.rglob("*"):
        if child.is_file():
            total += child.stat().st_size
    return total


def _summary_stat(metric_summaries: list, metric_name: str, field_name: str) -> float | None:
    for summary in metric_summaries:
        if summary.metric_name == metric_name:
            return getattr(summary, field_name)
    return None


__all__ = [
    "RunArtifacts",
    "RunResult",
    "RunnerError",
    "create_adapter",
    "load_corpus_bundle",
    "resolve_corpus_path",
    "run_benchmark",
]
