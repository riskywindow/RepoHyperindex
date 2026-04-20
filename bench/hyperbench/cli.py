"""CLI entrypoint for the Phase 1 Hyperbench harness."""

from __future__ import annotations

import argparse
import sys
from collections.abc import Sequence

from hyperbench.adapter import AdapterError
from hyperbench.compare import CompareError, write_compare_artifacts
from hyperbench.corpora import (
    DEFAULT_CONFIG_DIR,
    DEFAULT_CORPORA_DIR,
    CorporaError,
    ManifestValidationError,
    bootstrap_repos,
    create_corpus_snapshot,
    validate_phase1_config_dir,
)
from hyperbench.report import RunReportError, write_run_report
from hyperbench.runner import (
    RunnerError,
    create_adapter,
    load_corpus_bundle,
    resolve_corpus_path,
    run_benchmark,
)
from hyperbench.schemas import SyntheticCorpusConfig
from hyperbench.synth import SyntheticGenerationError, generate_synthetic_corpus_bundle


def build_parser() -> argparse.ArgumentParser:
    """Build the top-level argument parser for the Phase 1 harness CLI."""
    parser = argparse.ArgumentParser(
        prog="hyperbench",
        description=(
            "Repo Hyperindex Phase 1 benchmark harness. "
            "Validate configs, generate corpora, run fixture or daemon-backed benchmarks, "
            "and render report/compare artifacts."
        ),
    )
    parser.add_argument(
        "--version",
        action="version",
        version="hyperbench 0.1.0",
    )

    subparsers = parser.add_subparsers(dest="command")

    status_parser = subparsers.add_parser(
        "status",
        help="Show harness status.",
    )
    status_parser.set_defaults(command_handler=_run_status)

    corpora_parser = subparsers.add_parser(
        "corpora",
        help="Validate, bootstrap, and snapshot benchmark corpora.",
    )
    corpora_subparsers = corpora_parser.add_subparsers(dest="corpora_command")

    corpora_validate = corpora_subparsers.add_parser(
        "validate",
        help="Validate corpus config documents and cross-document references.",
    )
    corpora_validate.add_argument(
        "--config-dir",
        default=str(DEFAULT_CONFIG_DIR),
        help="Directory containing Hyperbench config documents.",
    )
    corpora_validate.set_defaults(command_handler=_run_corpora_validate)

    corpora_bootstrap = corpora_subparsers.add_parser(
        "bootstrap",
        help="Clone or update selected repos into the managed corpora directory.",
    )
    corpora_bootstrap.add_argument(
        "--config-dir",
        default=str(DEFAULT_CONFIG_DIR),
        help="Directory containing repos.yaml.",
    )
    corpora_bootstrap.add_argument(
        "--corpora-dir",
        default=str(DEFAULT_CORPORA_DIR),
        help="Managed corpora directory for cloned repos.",
    )
    corpora_bootstrap.add_argument(
        "--repo-id",
        action="append",
        default=None,
        help="Limit bootstrap to one or more specific repo ids.",
    )
    corpora_bootstrap.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the bootstrap plan without cloning or fetching.",
    )
    corpora_bootstrap.set_defaults(command_handler=_run_corpora_bootstrap)

    corpora_snapshot = corpora_subparsers.add_parser(
        "snapshot",
        help="Record snapshot metadata for a local corpus path.",
    )
    corpora_snapshot.add_argument(
        "--path",
        required=True,
        help="Local corpus directory to scan.",
    )
    corpora_snapshot.add_argument(
        "--manifest-path",
        help="Optional manifest file to hash for snapshot metadata.",
    )
    corpora_snapshot.add_argument(
        "--repo-id",
        help="Optional repo id from repos.yaml to hash as the manifest source.",
    )
    corpora_snapshot.add_argument(
        "--config-dir",
        default=str(DEFAULT_CONFIG_DIR),
        help="Directory containing repos.yaml when --repo-id is used.",
    )
    corpora_snapshot.set_defaults(command_handler=_run_corpora_snapshot)

    corpora_generate = corpora_subparsers.add_parser(
        "generate-synth",
        help="Generate a deterministic synthetic TypeScript monorepo corpus bundle.",
    )
    corpora_generate.add_argument(
        "--config-path",
        default="bench/configs/synthetic-corpus.yaml",
        help="Synthetic corpus config file to load.",
    )
    corpora_generate.add_argument(
        "--output-dir",
        help=(
            "Output directory for the generated corpus bundle. "
            "Defaults to bench/corpora/synthetic/<corpus_id>."
        ),
    )
    corpora_generate.add_argument(
        "--force",
        action="store_true",
        help="Replace an existing output directory if it already exists.",
    )
    corpora_generate.set_defaults(command_handler=_run_corpora_generate_synth)

    run_parser = subparsers.add_parser(
        "run",
        help="Execute the Phase 1 harness against a selected adapter and corpus bundle.",
    )
    run_parser.add_argument(
        "--adapter",
        choices=["fixture", "daemon", "daemon-impact", "shell"],
        default="fixture",
        help="Adapter boundary to use for this benchmark run.",
    )
    run_parser.add_argument(
        "--corpus-path",
        help="Path to a generated corpus bundle directory.",
    )
    run_parser.add_argument(
        "--corpus-id",
        help="Synthetic corpus id under bench/corpora/synthetic when --corpus-path is omitted.",
    )
    run_parser.add_argument(
        "--corpora-dir",
        default="bench/corpora/synthetic",
        help="Base directory used with --corpus-id.",
    )
    run_parser.add_argument(
        "--query-pack-id",
        action="append",
        default=None,
        help="Limit the run to one or more specific query_pack_id values.",
    )
    run_parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where summary, JSONL, and CSV outputs should be written.",
    )
    run_parser.add_argument(
        "--mode",
        choices=["smoke", "full"],
        default="full",
        help="Run a small smoke selection or the full selected query set.",
    )
    run_parser.add_argument(
        "--engine-bin",
        help=(
            "Optional path to the engine binary. "
            "For --adapter daemon this should point to hyperd; otherwise the workspace "
            "target/debug/hyperd binary or cargo run fallback is used."
        ),
    )
    run_parser.add_argument(
        "--daemon-build-temperature",
        choices=["cold", "warm"],
        default="cold",
        help=(
            "For --adapter daemon, measure either the first clean parser/symbol build "
            "or a warmed repeat build on the same clean snapshot."
        ),
    )
    run_parser.add_argument(
        "--daemon-workspace-root",
        help=(
            "Optional workspace directory for the daemon adapter. "
            "Defaults to a temporary directory and is mainly useful for debugging."
        ),
    )
    run_parser.set_defaults(command_handler=_run_harness)

    report_parser = subparsers.add_parser(
        "report",
        help="Render JSON and Markdown summaries for a completed benchmark run.",
    )
    report_parser.add_argument(
        "--run-dir",
        required=True,
        help="Directory containing a completed benchmark run.",
    )
    report_parser.add_argument(
        "--output-dir",
        help="Optional output directory for report.json and report.md. Defaults to the run dir.",
    )
    report_parser.set_defaults(command_handler=_run_report)

    compare_parser = subparsers.add_parser(
        "compare",
        help="Compare two completed benchmark runs against a budget config.",
    )
    compare_parser.add_argument(
        "--baseline-run-dir",
        required=True,
        help="Directory containing the baseline run outputs.",
    )
    compare_parser.add_argument(
        "--candidate-run-dir",
        required=True,
        help="Directory containing the candidate run outputs.",
    )
    compare_parser.add_argument(
        "--budgets-path",
        default="bench/configs/budgets.yaml",
        help="Budget config used for regression checks.",
    )
    compare_parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where compare.json and compare.md should be written.",
    )
    compare_parser.set_defaults(command_handler=_run_compare)
    return parser


def _run_status(_: argparse.Namespace) -> int:
    print(
        "hyperbench Phase 1 harness is installed and ready. "
        "Use corpora validate/generate-synth, run, report, and compare."
    )
    return 0


def _run_corpora_validate(args: argparse.Namespace) -> int:
    report = validate_phase1_config_dir(args.config_dir)
    if report.errors:
        raise ManifestValidationError("\n".join(report.errors))
    print("Validation succeeded.")
    if report.warnings:
        print("Warnings:")
        for warning in report.warnings:
            print(f"- {warning}")
    return 0


def _run_corpora_bootstrap(args: argparse.Namespace) -> int:
    plan = bootstrap_repos(
        config_dir=args.config_dir,
        corpora_dir=args.corpora_dir,
        dry_run=args.dry_run,
        repo_ids=args.repo_id,
    )
    if args.dry_run:
        print("Bootstrap dry-run plan:")
    else:
        print("Bootstrap completed:")
    for entry in plan:
        notes = f" [{'; '.join(entry.notes)}]" if entry.notes else ""
        pin = entry.pinned_ref if entry.pinned_ref is not None else "unset"
        print(f"- {entry.repo_id}: {entry.action} -> {entry.destination} @ {pin}{notes}")
    return 0


def _run_corpora_snapshot(args: argparse.Namespace) -> int:
    snapshot = create_corpus_snapshot(
        args.path,
        manifest_path=args.manifest_path,
        repo_id=args.repo_id,
        config_dir=args.config_dir,
    )
    print(snapshot.to_json_text())
    return 0


def _run_corpora_generate_synth(args: argparse.Namespace) -> int:
    config = SyntheticCorpusConfig.from_path(args.config_path)
    output_dir = args.output_dir or f"bench/corpora/synthetic/{config.corpus_id}"
    result = generate_synthetic_corpus_bundle(config, output_dir, force=args.force)
    print("Synthetic corpus generated:")
    print(f"- output_dir: {result.output_dir}")
    print(f"- repo_dir: {result.repo_dir}")
    print(f"- manifest: {result.manifest_path}")
    print(f"- ground_truth: {result.ground_truth_path}")
    print(f"- query_pack: {result.query_pack_path}")
    print(f"- golden_set: {result.golden_set_path}")
    print(f"- edit_scenarios: {result.edit_scenarios_path}")
    print(f"- repo_file_count: {result.repo_file_count}")
    return 0


def _run_harness(args: argparse.Namespace) -> int:
    corpus_path = resolve_corpus_path(
        corpus_path=args.corpus_path,
        corpus_id=args.corpus_id,
        corpora_dir=args.corpora_dir,
    )
    adapter = create_adapter(
        args.adapter,
        engine_bin=args.engine_bin,
        daemon_build_temperature=args.daemon_build_temperature,
        daemon_workspace_root=args.daemon_workspace_root,
    )
    bundle = load_corpus_bundle(corpus_path)
    result = run_benchmark(
        adapter=adapter,
        corpus_bundle=bundle,
        output_dir=args.output_dir,
        mode=args.mode,
        query_pack_ids=args.query_pack_id,
    )
    print("Benchmark run completed:")
    print(f"- run_id: {result.run_id}")
    print(f"- adapter: {result.adapter_name}")
    print(f"- corpus_id: {result.corpus_id}")
    print(f"- mode: {result.mode}")
    print(f"- query_packs: {', '.join(result.query_pack_ids)}")
    print(f"- query_count: {result.query_count}")
    print(f"- refresh_scenario_count: {result.refresh_scenario_count}")
    print(f"- summary: {result.artifacts.summary_path}")
    print(f"- events: {result.artifacts.events_path}")
    print(f"- metrics: {result.artifacts.metrics_path}")
    print(f"- query_results_csv: {result.artifacts.query_results_csv_path}")
    print(f"- refresh_results_csv: {result.artifacts.refresh_results_csv_path}")
    print(f"- metric_summaries_csv: {result.artifacts.metric_summaries_csv_path}")
    return 0


def _run_report(args: argparse.Namespace) -> int:
    report_json_path, report_markdown_path = write_run_report(
        args.run_dir,
        output_dir=args.output_dir,
    )
    print("Run report written:")
    print(f"- report_json: {report_json_path}")
    print(f"- report_markdown: {report_markdown_path}")
    return 0


def _run_compare(args: argparse.Namespace) -> int:
    compare_json_path, compare_markdown_path = write_compare_artifacts(
        baseline_run_dir=args.baseline_run_dir,
        candidate_run_dir=args.candidate_run_dir,
        budgets_path=args.budgets_path,
        output_dir=args.output_dir,
    )
    print("Run comparison written:")
    print(f"- compare_json: {compare_json_path}")
    print(f"- compare_markdown: {compare_markdown_path}")
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    """Run the CLI and return a process exit code."""
    parser = build_parser()
    args = parser.parse_args(list(argv) if argv is not None else None)
    command_handler = getattr(args, "command_handler", None)
    if command_handler is None:
        parser.print_help()
        return 0
    try:
        return int(command_handler(args))
    except (
        AdapterError,
        CompareError,
        CorporaError,
        RunReportError,
        RunnerError,
        SyntheticGenerationError,
    ) as exc:
        print(str(exc), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
