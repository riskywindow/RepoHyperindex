"""Integration coverage for the daemon-backed semantic adapter."""

from __future__ import annotations

import csv
import json
import shutil
import subprocess
from pathlib import Path

import pytest
from hyperbench.cli import main
from hyperbench.schemas import SyntheticCorpusConfig
from hyperbench.synth import generate_synthetic_corpus_bundle


def test_daemon_semantic_adapter_smoke_run_generates_comparable_artifacts(
    tmp_path: Path,
) -> None:
    if shutil.which("cargo") is None:
        pytest.skip("cargo is required for the daemon adapter smoke test")

    hyperd_bin = _ensure_hyperd_binary()
    bundle_dir = _generate_default_bundle(tmp_path / "bundle")
    fixture_dir = tmp_path / "fixture-smoke"
    daemon_dir = tmp_path / "daemon-semantic-smoke"
    report_dir = tmp_path / "report"
    compare_dir = tmp_path / "compare"

    assert (
        main(
            [
                "run",
                "--adapter",
                "fixture",
                "--corpus-path",
                str(bundle_dir),
                "--query-pack-id",
                "synthetic-saas-medium-semantic-pack",
                "--output-dir",
                str(fixture_dir),
                "--mode",
                "smoke",
            ]
        )
        == 0
    )
    assert (
        main(
            [
                "run",
                "--adapter",
                "daemon-semantic",
                "--engine-bin",
                str(hyperd_bin),
                "--daemon-build-temperature",
                "cold",
                "--corpus-path",
                str(bundle_dir),
                "--query-pack-id",
                "synthetic-saas-medium-semantic-pack",
                "--output-dir",
                str(daemon_dir),
                "--mode",
                "smoke",
            ]
        )
        == 0
    )
    assert main(["report", "--run-dir", str(daemon_dir), "--output-dir", str(report_dir)]) == 0
    assert (
        main(
            [
                "compare",
                "--baseline-run-dir",
                str(fixture_dir),
                "--candidate-run-dir",
                str(daemon_dir),
                "--budgets-path",
                "bench/configs/budgets.yaml",
                "--output-dir",
                str(compare_dir),
            ]
        )
        == 0
    )

    summary = json.loads((daemon_dir / "summary.json").read_text(encoding="utf-8"))
    report_json = json.loads((report_dir / "report.json").read_text(encoding="utf-8"))
    compare_json = json.loads((compare_dir / "compare.json").read_text(encoding="utf-8"))

    with (daemon_dir / "refresh_results.csv").open(encoding="utf-8", newline="") as handle:
        refresh_rows = list(csv.DictReader(handle))

    assert summary["adapter"] == "daemon-semantic"
    assert summary["query_count"] == 1
    assert summary["query_counts_by_type"] == {"semantic": 1}
    assert summary["refresh_scenario_count"] == 2
    assert summary["benchmark_dimensions"]["adapter_transport"] in {
        "daemon_protocol",
        "daemon_protocol_stdio",
    }
    assert summary["benchmark_dimensions"]["build_temperature"] == "cold"
    assert summary["prepare"]["metadata"]["engine_backend"] == "phase6-semantic-daemon"
    assert (
        summary["prepare"]["metadata"]["semantic_build"]["refresh_mode"] == "full_rebuild"
    )
    assert summary["prepare"]["metadata"]["semantic_status_before"]["state"] == "not_ready"
    assert summary["prepare"]["metadata"]["semantic_status_ready"]["state"] == "ready"
    assert report_json["benchmark_dimensions"]["engine_backend"] == "phase6-semantic-daemon"
    assert compare_json["metric_deltas"]
    assert refresh_rows
    assert float(refresh_rows[0]["semantic_build_latency_ms"]) >= 0.0
    assert float(refresh_rows[0]["semantic_query_latency_ms"]) >= 0.0
    assert "semantic_refresh_mode" in refresh_rows[0]
    assert "semantic_query_id" in refresh_rows[0]


def _ensure_hyperd_binary() -> Path:
    hyperd_bin = Path("target/debug/hyperd")
    if hyperd_bin.exists():
        return hyperd_bin
    subprocess.run(
        ["cargo", "build", "-p", "hyperindex-daemon"],
        check=True,
        capture_output=True,
        text=True,
    )
    return hyperd_bin


def _generate_default_bundle(output_dir: Path) -> Path:
    config = SyntheticCorpusConfig.from_path("bench/configs/synthetic-corpus.yaml")
    generate_synthetic_corpus_bundle(config, output_dir)
    return output_dir
