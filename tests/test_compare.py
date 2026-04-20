"""Report and compare tests for completed Hyperbench runs."""

from __future__ import annotations

import json
from pathlib import Path

from hyperbench.cli import main
from hyperbench.schemas import SyntheticCorpusConfig
from hyperbench.synth import generate_synthetic_corpus_bundle


def test_report_and_compare_commands_generate_reviewable_artifacts(tmp_path: Path) -> None:
    bundle_dir = _generate_default_bundle(tmp_path / "bundle")
    baseline_dir = tmp_path / "baseline"
    candidate_dir = tmp_path / "candidate"
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
                "--output-dir",
                str(baseline_dir),
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
                "fixture",
                "--corpus-path",
                str(bundle_dir),
                "--output-dir",
                str(candidate_dir),
                "--mode",
                "smoke",
            ]
        )
        == 0
    )

    assert main(["report", "--run-dir", str(candidate_dir), "--output-dir", str(report_dir)]) == 0
    assert (
        main(
            [
                "compare",
                "--baseline-run-dir",
                str(baseline_dir),
                "--candidate-run-dir",
                str(candidate_dir),
                "--budgets-path",
                "bench/configs/budgets.yaml",
                "--output-dir",
                str(compare_dir),
            ]
        )
        == 0
    )

    report_json = json.loads((report_dir / "report.json").read_text(encoding="utf-8"))
    report_md = (report_dir / "report.md").read_text(encoding="utf-8")
    compare_json = json.loads((compare_dir / "compare.json").read_text(encoding="utf-8"))
    compare_md = (compare_dir / "compare.md").read_text(encoding="utf-8")

    assert report_json["corpus"]["corpus_id"] == "synthetic-saas-medium"
    assert report_json["benchmark_dimensions"]["query_types"] == [
        "exact",
        "impact",
        "semantic",
        "symbol",
    ]
    assert "## Instrumentation" in report_md
    assert "query-latency-p95" in compare_md
    assert compare_json["verdict"] in {"pass", "warn", "fail"}
    assert compare_json["budget_results"]


def _generate_default_bundle(output_dir: Path) -> Path:
    config = SyntheticCorpusConfig.from_path("bench/configs/synthetic-corpus.yaml")
    generate_synthetic_corpus_bundle(config, output_dir)
    return output_dir
