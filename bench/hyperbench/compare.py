"""Regression comparison and budget evaluation helpers for Hyperbench."""

from __future__ import annotations

from pathlib import Path

from hyperbench.report import load_run_summary, write_json
from hyperbench.schemas import (
    BudgetResult,
    BudgetSeverity,
    BudgetStatus,
    CompareBudget,
    CompareOutput,
    CompareVerdict,
    MetricDelta,
    MetricUnit,
)


class CompareError(RuntimeError):
    """Raised when compare artifacts cannot be generated."""


def write_compare_artifacts(
    *,
    baseline_run_dir: str | Path,
    candidate_run_dir: str | Path,
    budgets_path: str | Path,
    output_dir: str | Path,
) -> tuple[Path, Path]:
    """Generate JSON and Markdown comparison artifacts for two completed runs."""
    baseline_summary = load_run_summary(baseline_run_dir)
    candidate_summary = load_run_summary(candidate_run_dir)
    budget = CompareBudget.from_path(budgets_path)
    compare_output = build_compare_output(
        baseline_summary=baseline_summary,
        candidate_summary=candidate_summary,
        budget=budget,
    )
    output_root = Path(output_dir)
    output_root.mkdir(parents=True, exist_ok=True)
    compare_json_path = output_root / "compare.json"
    compare_markdown_path = output_root / "compare.md"
    write_json(compare_json_path, compare_output.model_dump(mode="json"))
    compare_markdown_path.write_text(
        render_compare_markdown(
            baseline_summary=baseline_summary,
            candidate_summary=candidate_summary,
            compare_output=compare_output,
        ),
        encoding="utf-8",
    )
    return compare_json_path, compare_markdown_path


def build_compare_output(
    *,
    baseline_summary: dict[str, object],
    candidate_summary: dict[str, object],
    budget: CompareBudget,
) -> CompareOutput:
    """Build a normalized comparison output with budget evaluation."""
    baseline_metrics = extract_compare_metrics(baseline_summary)
    candidate_metrics = extract_compare_metrics(candidate_summary)

    metric_names = sorted(set(baseline_metrics) | set(candidate_metrics))
    metric_deltas: list[MetricDelta] = []
    for metric_name in metric_names:
        baseline_metric = baseline_metrics.get(metric_name)
        candidate_metric = candidate_metrics.get(metric_name)
        if baseline_metric is None or candidate_metric is None:
            continue
        if baseline_metric.get("value") is None or candidate_metric.get("value") is None:
            continue
        if baseline_metric["unit"] != candidate_metric["unit"]:
            continue
        baseline_value = float(baseline_metric["value"])
        candidate_value = float(candidate_metric["value"])
        absolute_delta = candidate_value - baseline_value
        percent_delta = None
        if baseline_value != 0.0:
            percent_delta = (absolute_delta / baseline_value) * 100.0
        metric_deltas.append(
            MetricDelta(
                metric_name=metric_name,
                unit=MetricUnit(str(candidate_metric["unit"])),
                baseline_value=baseline_value,
                candidate_value=candidate_value,
                absolute_delta=absolute_delta,
                percent_delta=percent_delta,
            )
        )

    budget_results = evaluate_budget(
        budget=budget,
        baseline_metrics=baseline_metrics,
        candidate_metrics=candidate_metrics,
    )
    verdict = _budget_results_verdict(budget_results)
    return CompareOutput(
        baseline_run_id=str(baseline_summary["run_id"]),
        candidate_run_id=str(candidate_summary["run_id"]),
        verdict=verdict,
        metric_deltas=metric_deltas,
        budget_results=budget_results,
    )


def extract_compare_metrics(summary: dict[str, object]) -> dict[str, dict[str, object]]:
    """Extract the flattened comparable metric surface from a run summary."""
    instrumentation = dict(summary.get("instrumentation", {}))
    metric_summaries = {
        str(entry.get("metric_name")): entry for entry in summary.get("metric_summaries", [])
    }
    return {
        "query-latency-p50": _metric("ms", instrumentation.get("query_latency_p50_ms")),
        "query-latency-p95": _metric("ms", instrumentation.get("query_latency_p95_ms")),
        "impact-latency-p50": _metric(
            "ms",
            _summary_stat(metric_summaries, "impact-latency", "p50"),
        ),
        "impact-latency-p95": _metric(
            "ms",
            _summary_stat(metric_summaries, "impact-latency", "p95"),
        ),
        "semantic-latency-p50": _metric(
            "ms",
            _summary_stat(metric_summaries, "semantic-latency", "p50"),
        ),
        "semantic-latency-p95": _metric(
            "ms",
            _summary_stat(metric_summaries, "semantic-latency", "p95"),
        ),
        "refresh-latency-p50": _metric("ms", instrumentation.get("refresh_latency_p50_ms")),
        "refresh-latency-p95": _metric("ms", instrumentation.get("refresh_latency_p95_ms")),
        "wall-clock": _metric("ms", instrumentation.get("wall_clock_ms")),
        "peak-rss": _metric("bytes", instrumentation.get("peak_rss_bytes")),
        "output-disk-usage": _metric("bytes", instrumentation.get("output_disk_usage_bytes")),
        "query-pass-rate": _metric("ratio", summary.get("query_pass_rate")),
        "prepare-latency": _metric(
            "ms",
            _summary_stat(metric_summaries, "prepare-latency", "mean"),
        ),
        "prepare-parse-build-latency": _metric(
            "ms",
            _summary_stat(metric_summaries, "prepare-parse-build-latency", "mean"),
        ),
        "prepare-symbol-build-latency": _metric(
            "ms",
            _summary_stat(metric_summaries, "prepare-symbol-build-latency", "mean"),
        ),
        "prepare-impact-analyze-latency": _metric(
            "ms",
            _summary_stat(metric_summaries, "prepare-impact-analyze-latency", "mean"),
        ),
        "prepare-semantic-build-latency": _metric(
            "ms",
            _summary_stat(metric_summaries, "prepare-semantic-build-latency", "mean"),
        ),
        "refresh-parse-build-latency-p50": _metric(
            "ms",
            _summary_stat(metric_summaries, "refresh-parse-build-latency", "p50"),
        ),
        "refresh-symbol-build-latency-p50": _metric(
            "ms",
            _summary_stat(metric_summaries, "refresh-symbol-build-latency", "p50"),
        ),
        "refresh-impact-analyze-latency-p50": _metric(
            "ms",
            _summary_stat(metric_summaries, "refresh-impact-analyze-latency", "p50"),
        ),
        "refresh-semantic-build-latency-p50": _metric(
            "ms",
            _summary_stat(metric_summaries, "refresh-semantic-build-latency", "p50"),
        ),
        "refresh-semantic-query-latency-p50": _metric(
            "ms",
            _summary_stat(metric_summaries, "refresh-semantic-query-latency", "p50"),
        ),
        "refresh-impact-refresh-elapsed-ms-p50": _metric(
            "ms",
            _summary_stat(metric_summaries, "refresh-impact-refresh-elapsed-ms", "p50"),
        ),
        "refresh-semantic-refresh-elapsed-ms-p50": _metric(
            "ms",
            _summary_stat(metric_summaries, "refresh-semantic-refresh-elapsed-ms", "p50"),
        ),
    }


def evaluate_budget(
    *,
    budget: CompareBudget,
    baseline_metrics: dict[str, dict[str, object]],
    candidate_metrics: dict[str, dict[str, object]],
) -> list[BudgetResult]:
    """Evaluate a compare budget against baseline and candidate metrics."""
    results: list[BudgetResult] = []
    for threshold in budget.thresholds:
        candidate_metric = candidate_metrics.get(threshold.metric_name)
        baseline_metric = baseline_metrics.get(threshold.metric_name)
        if candidate_metric is None or candidate_metric.get("value") is None:
            results.append(
                BudgetResult(
                    metric_name=threshold.metric_name,
                    status=BudgetStatus.WARN,
                    message="Metric unavailable; skipped budget evaluation.",
                )
            )
            continue

        if str(candidate_metric["unit"]) != threshold.unit.value:
            results.append(
                BudgetResult(
                    metric_name=threshold.metric_name,
                    status=BudgetStatus.WARN,
                    message=(
                        "Metric unit mismatch; expected "
                        f"{threshold.unit.value}, got {candidate_metric['unit']}."
                    ),
                )
            )
            continue

        observed_value = float(candidate_metric["value"])
        violations: list[str] = []
        soft_warnings: list[str] = []
        if threshold.max_value is not None and observed_value > threshold.max_value:
            violations.append(f"value {observed_value:.3f} exceeds max {threshold.max_value:.3f}")
        if threshold.min_value is not None and observed_value < threshold.min_value:
            violations.append(f"value {observed_value:.3f} is below min {threshold.min_value:.3f}")
        if threshold.max_regression_pct is not None:
            regression_message = _regression_check(
                metric_name=threshold.metric_name,
                threshold_max_regression_pct=threshold.max_regression_pct,
                baseline_metric=baseline_metric,
                candidate_metric=candidate_metric,
            )
            if regression_message is not None:
                if "skipped" in regression_message or "unavailable" in regression_message:
                    soft_warnings.append(regression_message)
                else:
                    violations.append(regression_message)

        if violations:
            status = _severity_to_status(threshold.severity)
            results.append(
                BudgetResult(
                    metric_name=threshold.metric_name,
                    status=status,
                    message="; ".join(violations),
                    observed_value=observed_value,
                )
            )
            continue

        if soft_warnings:
            results.append(
                BudgetResult(
                    metric_name=threshold.metric_name,
                    status=BudgetStatus.WARN,
                    message="; ".join(soft_warnings),
                    observed_value=observed_value,
                )
            )
            continue

        results.append(
            BudgetResult(
                metric_name=threshold.metric_name,
                status=BudgetStatus.PASS,
                message="Budget check passed.",
                observed_value=observed_value,
            )
        )

    return results


def render_compare_markdown(
    *,
    baseline_summary: dict[str, object],
    candidate_summary: dict[str, object],
    compare_output: CompareOutput,
) -> str:
    """Render a concise Markdown compare summary suitable for PRs or issues."""
    lines = [
        f"# Hyperbench Compare: {compare_output.candidate_run_id}",
        "",
        f"- Baseline: `{compare_output.baseline_run_id}`",
        f"- Candidate: `{compare_output.candidate_run_id}`",
        f"- Verdict: `{compare_output.verdict.value}`",
        f"- Baseline adapter: `{baseline_summary.get('adapter')}`",
        f"- Candidate adapter: `{candidate_summary.get('adapter')}`",
        f"- Corpus: `{dict(candidate_summary.get('corpus', {})).get('corpus_id')}`",
        "- Baseline build temperature: "
        f"`{_benchmark_dimension(baseline_summary, 'build_temperature')}`",
        "- Candidate build temperature: "
        f"`{_benchmark_dimension(candidate_summary, 'build_temperature')}`",
        "",
        "## Metric Deltas",
        "",
        "| Metric | Baseline | Candidate | Delta |",
        "| --- | ---: | ---: | ---: |",
    ]
    if compare_output.metric_deltas:
        for delta in compare_output.metric_deltas:
            lines.append(
                "| "
                f"{delta.metric_name} | "
                f"{delta.baseline_value:.3f} {delta.unit.value} | "
                f"{delta.candidate_value:.3f} {delta.unit.value} | "
                f"{_format_delta(delta)} |"
            )
    else:
        lines.append("| no-overlap | n/a | n/a | n/a |")

    lines.extend(
        [
            "",
            "## Budget Results",
            "",
            "| Metric | Status | Message |",
            "| --- | --- | --- |",
        ]
    )
    for result in compare_output.budget_results:
        lines.append(f"| {result.metric_name} | {result.status.value} | {result.message} |")
    return "\n".join(lines) + "\n"


def _metric(unit: str, value: object) -> dict[str, object]:
    return {"unit": unit, "value": None if value is None else float(value)}


def _summary_stat(
    metric_summaries: dict[str, dict[str, object]],
    metric_name: str,
    field_name: str,
) -> float | None:
    summary = metric_summaries.get(metric_name)
    if not isinstance(summary, dict):
        return None
    value = summary.get(field_name)
    return None if value is None else float(value)


def _benchmark_dimension(summary: dict[str, object], key: str) -> str:
    benchmark_dimensions = summary.get("benchmark_dimensions")
    if not isinstance(benchmark_dimensions, dict):
        return "unavailable"
    value = benchmark_dimensions.get(key)
    return str(value) if value is not None else "unavailable"


def _severity_to_status(severity: BudgetSeverity) -> BudgetStatus:
    return BudgetStatus.FAIL if severity == BudgetSeverity.FAIL else BudgetStatus.WARN


def _budget_results_verdict(results: list[BudgetResult]) -> CompareVerdict:
    statuses = {result.status for result in results}
    if BudgetStatus.FAIL in statuses:
        return CompareVerdict.FAIL
    if BudgetStatus.WARN in statuses:
        return CompareVerdict.WARN
    return CompareVerdict.PASS


def _regression_check(
    *,
    metric_name: str,
    threshold_max_regression_pct: float,
    baseline_metric: dict[str, object] | None,
    candidate_metric: dict[str, object],
) -> str | None:
    if baseline_metric is None or baseline_metric.get("value") is None:
        return "baseline metric unavailable for regression-percent check"

    baseline_value = float(baseline_metric["value"])
    candidate_value = float(candidate_metric["value"])
    if baseline_value == 0.0:
        return "baseline metric is zero, so regression-percent check was skipped"

    higher_is_better = str(candidate_metric["unit"]) in {"ratio", "percent"}
    if higher_is_better:
        regression_pct = ((baseline_value - candidate_value) / baseline_value) * 100.0
    else:
        regression_pct = ((candidate_value - baseline_value) / baseline_value) * 100.0

    if regression_pct > threshold_max_regression_pct:
        return (
            f"regression {regression_pct:.3f}% exceeds max regression "
            f"{threshold_max_regression_pct:.3f}%"
        )
    return None


def _format_delta(delta: MetricDelta) -> str:
    if delta.percent_delta is None:
        return f"{delta.absolute_delta:.3f} {delta.unit.value}"
    return f"{delta.absolute_delta:.3f} {delta.unit.value} ({delta.percent_delta:.2f}%)"
