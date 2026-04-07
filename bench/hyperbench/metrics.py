"""Metric helpers for Hyperbench benchmark runs."""

from __future__ import annotations

from collections import defaultdict
from statistics import mean

from hyperbench.schemas import MetricKind, MetricSample, MetricSummary, MetricUnit


def build_metric_samples(
    *,
    query_rows: list[dict[str, object]],
    refresh_rows: list[dict[str, object]],
    run_metric_rows: list[dict[str, object]] | None = None,
) -> list[MetricSample]:
    """Build raw metric samples from normalized query and refresh rows."""
    samples: list[MetricSample] = []
    for row in query_rows:
        query_type = str(row["query_type"])
        query_id = str(row["query_id"])
        passed_value = 1.0 if bool(row["passed"]) else 0.0
        samples.append(
            MetricSample(
                metric_name="query-latency",
                metric_kind=MetricKind.LATENCY,
                unit=MetricUnit.MILLISECONDS,
                value=float(row["latency_ms"]),
                tags={"query_type": query_type, "query_id": query_id},
            )
        )
        samples.append(
            MetricSample(
                metric_name=f"{query_type}-latency",
                metric_kind=MetricKind.LATENCY,
                unit=MetricUnit.MILLISECONDS,
                value=float(row["latency_ms"]),
                tags={"query_type": query_type, "query_id": query_id},
            )
        )
        samples.append(
            MetricSample(
                metric_name="query-pass-rate",
                metric_kind=MetricKind.ACCURACY,
                unit=MetricUnit.RATIO,
                value=passed_value,
                tags={"query_type": query_type, "query_id": query_id},
            )
        )
        samples.append(
            MetricSample(
                metric_name=f"{query_type}-pass-rate",
                metric_kind=MetricKind.ACCURACY,
                unit=MetricUnit.RATIO,
                value=passed_value,
                tags={"query_type": query_type, "query_id": query_id},
            )
        )
        samples.append(
            MetricSample(
                metric_name=f"{query_type}-hit-count",
                metric_kind=MetricKind.ACCURACY,
                unit=MetricUnit.COUNT,
                value=float(row["actual_hit_count"]),
                tags={"query_type": query_type, "query_id": query_id},
            )
        )

    for row in refresh_rows:
        scenario_id = str(row["scenario_id"])
        samples.append(
            MetricSample(
                metric_name="refresh-latency",
                metric_kind=MetricKind.LATENCY,
                unit=MetricUnit.MILLISECONDS,
                value=float(row["latency_ms"]),
                tags={"scenario_id": scenario_id},
            )
        )
        samples.append(
            MetricSample(
                metric_name="refresh-changed-query-count",
                metric_kind=MetricKind.CUSTOM,
                unit=MetricUnit.COUNT,
                value=float(row["changed_query_count"]),
                tags={"scenario_id": scenario_id},
            )
        )

    for row in run_metric_rows or []:
        tags = row.get("tags") or {}
        samples.append(
            MetricSample(
                metric_name=str(row["metric_name"]),
                metric_kind=MetricKind(str(row["metric_kind"])),
                unit=MetricUnit(str(row["unit"])),
                value=float(row["value"]),
                tags={str(key): str(value) for key, value in dict(tags).items()},
            )
        )

    return samples


def summarize_metric_samples(samples: list[MetricSample]) -> list[MetricSummary]:
    """Collapse raw metric samples into metric summaries."""
    grouped: dict[tuple[str, MetricKind, MetricUnit], list[float]] = defaultdict(list)
    for sample in samples:
        grouped[(sample.metric_name, sample.metric_kind, sample.unit)].append(sample.value)

    summaries: list[MetricSummary] = []
    for (metric_name, metric_kind, unit), values in sorted(grouped.items()):
        ordered = sorted(values)
        summaries.append(
            MetricSummary(
                metric_name=metric_name,
                metric_kind=metric_kind,
                unit=unit,
                sample_count=len(ordered),
                minimum=ordered[0],
                maximum=ordered[-1],
                mean=mean(ordered),
                p50=_percentile(ordered, 0.50),
                p95=_percentile(ordered, 0.95),
                p99=_percentile(ordered, 0.99),
            )
        )
    return summaries


def _percentile(values: list[float], percentile: float) -> float | None:
    if not values:
        return None
    if len(values) == 1:
        return values[0]
    index = round((len(values) - 1) * percentile)
    return values[index]
