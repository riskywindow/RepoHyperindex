"""Output writers and report helpers for Hyperbench benchmark runs."""

from __future__ import annotations

import csv
import json
from pathlib import Path


class RunReportError(RuntimeError):
    """Raised when report artifacts cannot be loaded or rendered."""


def write_json(path: Path, payload: object) -> None:
    """Write a JSON document with deterministic formatting."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, records: list[dict[str, object]]) -> None:
    """Write a JSONL document with one record per line."""
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = [json.dumps(record, sort_keys=True) for record in records]
    path.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")


def write_csv(path: Path, rows: list[dict[str, object]], fieldnames: list[str]) -> None:
    """Write a CSV file with the provided field order."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in fieldnames})


def load_run_summary(run_dir: str | Path) -> dict[str, object]:
    """Load a run summary JSON document from a completed run directory."""
    summary_path = Path(run_dir) / "summary.json"
    try:
        return json.loads(summary_path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise RunReportError(f"Run summary does not exist: {summary_path}") from exc
    except json.JSONDecodeError as exc:
        raise RunReportError(f"Run summary is not valid JSON: {summary_path}") from exc


def build_run_report(summary: dict[str, object]) -> dict[str, object]:
    """Build a compact, report-friendly JSON document from a run summary."""
    instrumentation = dict(summary.get("instrumentation", {}))
    run_metadata = dict(summary.get("run_metadata", {}))
    corpus = dict(summary.get("corpus", {}))
    query_counts_by_type = dict(summary.get("query_counts_by_type", {}))

    return {
        "run_id": summary.get("run_id"),
        "adapter": summary.get("adapter"),
        "mode": summary.get("mode"),
        "corpus": {
            "corpus_id": corpus.get("corpus_id"),
            "display_name": corpus.get("display_name"),
            "bundle_path": corpus.get("bundle_path"),
        },
        "query_count": summary.get("query_count"),
        "query_pass_count": summary.get("query_pass_count"),
        "query_pass_rate": summary.get("query_pass_rate"),
        "refresh_scenario_count": summary.get("refresh_scenario_count"),
        "query_counts_by_type": query_counts_by_type,
        "instrumentation": instrumentation,
        "host": {
            "os": run_metadata.get("os"),
            "cpu": run_metadata.get("cpu"),
            "ram_bytes": run_metadata.get("ram_bytes"),
            "tool_versions": run_metadata.get("tool_versions"),
            "git_sha": run_metadata.get("git_sha"),
        },
    }


def render_run_report_markdown(report: dict[str, object]) -> str:
    """Render a review-friendly Markdown summary for a completed run."""
    instrumentation = dict(report.get("instrumentation", {}))
    corpus = dict(report.get("corpus", {}))
    host = dict(report.get("host", {}))
    query_counts_by_type = dict(report.get("query_counts_by_type", {}))
    lines = [
        f"# Hyperbench Run Report: {report.get('run_id')}",
        "",
        "## Overview",
        "",
        f"- Adapter: `{report.get('adapter')}`",
        f"- Mode: `{report.get('mode')}`",
        f"- Corpus: `{corpus.get('corpus_id')}`",
        f"- Query count: `{report.get('query_count')}`",
        f"- Query pass count: `{report.get('query_pass_count')}`",
        f"- Query pass rate: `{_format_ratio(report.get('query_pass_rate'))}`",
        f"- Refresh scenarios: `{report.get('refresh_scenario_count')}`",
        "",
        "## Instrumentation",
        "",
        f"- Wall clock: `{_format_ms(instrumentation.get('wall_clock_ms'))}`",
        f"- Query latency p50: `{_format_ms(instrumentation.get('query_latency_p50_ms'))}`",
        f"- Query latency p95: `{_format_ms(instrumentation.get('query_latency_p95_ms'))}`",
        f"- Refresh latency p50: `{_format_ms(instrumentation.get('refresh_latency_p50_ms'))}`",
        f"- Refresh latency p95: `{_format_ms(instrumentation.get('refresh_latency_p95_ms'))}`",
        f"- Peak RSS: `{_format_bytes(instrumentation.get('peak_rss_bytes'))}`",
        f"- Output disk usage: `{_format_bytes(instrumentation.get('output_disk_usage_bytes'))}`",
        "",
        "## Query Mix",
        "",
    ]
    for query_type, count in sorted(query_counts_by_type.items()):
        lines.append(f"- `{query_type}`: `{count}`")

    lines.extend(
        [
            "",
            "## Host",
            "",
            f"- OS: `{_nested(host, 'os', 'platform')}`",
            f"- CPU: `{_nested(host, 'cpu', 'processor')}`",
            f"- RAM: `{_format_bytes(host.get('ram_bytes'))}`",
            f"- Python: `{_nested(host, 'tool_versions', 'python')}`",
            f"- uv: `{_nested(host, 'tool_versions', 'uv')}`",
            f"- git: `{_nested(host, 'tool_versions', 'git')}`",
            f"- Git SHA: `{host.get('git_sha') or 'unavailable'}`",
        ]
    )
    return "\n".join(lines) + "\n"


def write_run_report(
    run_dir: str | Path,
    *,
    output_dir: str | Path | None = None,
) -> tuple[Path, Path]:
    """Write JSON and Markdown reports for a completed run directory."""
    run_root = Path(run_dir)
    target_dir = Path(output_dir) if output_dir is not None else run_root
    target_dir.mkdir(parents=True, exist_ok=True)
    summary = load_run_summary(run_root)
    report = build_run_report(summary)
    report_json_path = target_dir / "report.json"
    report_markdown_path = target_dir / "report.md"
    write_json(report_json_path, report)
    report_markdown_path.write_text(render_run_report_markdown(report), encoding="utf-8")
    return report_json_path, report_markdown_path


def _format_ms(value: object) -> str:
    if value is None:
        return "unavailable"
    return f"{float(value):.2f} ms"


def _format_ratio(value: object) -> str:
    if value is None:
        return "unavailable"
    return f"{float(value):.3f}"


def _format_bytes(value: object) -> str:
    if value is None:
        return "unavailable"
    amount = float(value)
    for suffix in ("B", "KiB", "MiB", "GiB"):
        if abs(amount) < 1024.0 or suffix == "GiB":
            return f"{amount:.2f} {suffix}"
        amount /= 1024.0
    return f"{amount:.2f} GiB"


def _nested(root: dict[str, object], key: str, nested_key: str) -> str:
    value = root.get(key)
    if not isinstance(value, dict):
        return "unavailable"
    return str(value.get(nested_key) or "unavailable")
