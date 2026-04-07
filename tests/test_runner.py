"""End-to-end harness runner tests for the Phase 1 benchmark harness."""

from __future__ import annotations

import csv
import json
from pathlib import Path

import pytest
from hyperbench.cli import main
from hyperbench.runner import (
    RunnerError,
    create_adapter,
    load_corpus_bundle,
    run_benchmark,
)
from hyperbench.schemas import SyntheticCorpusConfig
from hyperbench.synth import generate_synthetic_corpus_bundle


def test_fixture_adapter_full_run_writes_machine_readable_outputs(tmp_path: Path) -> None:
    bundle_dir = _generate_default_bundle(tmp_path / "bundle")
    output_dir = tmp_path / "run-output"

    result = run_benchmark(
        adapter=create_adapter("fixture"),
        corpus_bundle=load_corpus_bundle(bundle_dir),
        output_dir=output_dir,
        mode="full",
    )

    assert result.query_count == 210
    assert result.refresh_scenario_count == 4
    assert result.artifacts.summary_path.exists()
    assert result.artifacts.events_path.exists()
    assert result.artifacts.metrics_path.exists()
    assert result.artifacts.query_results_csv_path.exists()
    assert result.artifacts.refresh_results_csv_path.exists()
    assert result.artifacts.metric_summaries_csv_path.exists()

    summary = json.loads(result.artifacts.summary_path.read_text(encoding="utf-8"))
    assert summary["adapter"] == "fixture"
    assert summary["query_count"] == 210
    assert summary["query_pass_count"] == 210
    assert summary["run_metadata"]["adapter_name"] == "fixture"
    assert summary["run_metadata"]["corpus_id"] == "synthetic-saas-medium"
    assert summary["instrumentation"]["wall_clock_ms"] is not None
    assert summary["instrumentation"]["query_latency_p50_ms"] is not None
    assert summary["instrumentation"]["query_latency_p95_ms"] is not None
    assert summary["instrumentation"]["output_disk_usage_bytes"] is not None
    assert summary["query_counts_by_type"] == {
        "exact": 100,
        "impact": 30,
        "semantic": 30,
        "symbol": 50,
    }

    event_lines = result.artifacts.events_path.read_text(encoding="utf-8").strip().splitlines()
    assert any('"event_type": "prepare"' in line for line in event_lines)
    assert any('"event_type": "query"' in line for line in event_lines)
    assert any('"event_type": "refresh"' in line for line in event_lines)

    with result.artifacts.query_results_csv_path.open(encoding="utf-8", newline="") as handle:
        query_rows = list(csv.DictReader(handle))
    assert len(query_rows) == 210
    assert any(row["query_id"] == "semantic-hero-session-invalidation" for row in query_rows)
    assert all(row["passed"] == "True" for row in query_rows)


def test_hyperbench_run_smoke_cli_succeeds_with_fixture_adapter(
    tmp_path: Path,
    capsys,
) -> None:
    bundle_dir = _generate_default_bundle(tmp_path / "bundle")
    output_dir = tmp_path / "smoke-output"

    exit_code = main(
        [
            "run",
            "--adapter",
            "fixture",
            "--corpus-path",
            str(bundle_dir),
            "--output-dir",
            str(output_dir),
            "--mode",
            "smoke",
        ]
    )
    captured = capsys.readouterr()

    assert exit_code == 0
    assert "Benchmark run completed:" in captured.out
    summary = json.loads((output_dir / "summary.json").read_text(encoding="utf-8"))
    assert summary["mode"] == "smoke"
    assert summary["query_count"] == 4
    assert summary["refresh_scenario_count"] == 2


def test_shell_adapter_fails_clearly_without_real_engine(tmp_path: Path) -> None:
    bundle_dir = _generate_default_bundle(tmp_path / "bundle")
    output_dir = tmp_path / "shell-output"

    exit_code = main(
        [
            "run",
            "--adapter",
            "shell",
            "--corpus-path",
            str(bundle_dir),
            "--output-dir",
            str(output_dir),
        ]
    )

    assert exit_code == 2


def test_run_unknown_query_pack_id_fails_cleanly(tmp_path: Path, capsys) -> None:
    bundle_dir = _generate_default_bundle(tmp_path / "bundle")
    output_dir = tmp_path / "run-output"

    exit_code = main(
        [
            "run",
            "--adapter",
            "fixture",
            "--corpus-path",
            str(bundle_dir),
            "--query-pack-id",
            "missing-pack",
            "--output-dir",
            str(output_dir),
        ]
    )
    captured = capsys.readouterr()

    assert exit_code == 2
    assert "Unknown query_pack_id values: missing-pack" in captured.err


def test_load_corpus_bundle_rejects_manifest_query_pack_mismatch(tmp_path: Path) -> None:
    bundle_dir = _generate_default_bundle(tmp_path / "bundle")
    manifest_path = bundle_dir / "corpus-manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["query_pack_ids"] = ["synthetic-saas-medium-exact-pack"]
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    with pytest.raises(RunnerError, match="manifest at .* out of sync with query-packs/"):
        load_corpus_bundle(bundle_dir)


def _generate_default_bundle(output_dir: Path) -> Path:
    config = SyntheticCorpusConfig.from_path("bench/configs/synthetic-corpus.yaml")
    generate_synthetic_corpus_bundle(config, output_dir)
    return output_dir
