# How To Run Phase 1

This guide is the operator-facing walkthrough for the Phase 1 Hyperbench harness.

## Prerequisites

- Python `3.12+`
- [`uv`](https://docs.astral.sh/uv/) available locally
- a fresh clone of the repo
- Rust toolchain with `cargo` for the real symbol and impact engine paths

## First-Time Setup

From the repo root:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv sync
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench --help
```

Recommended sanity checks:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run ruff check .
UV_CACHE_DIR=/tmp/uv-cache uv run pytest
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora validate
```

## Local Smoke Run

This is the fastest end-to-end path and matches the CI smoke flow.

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora generate-synth \
  --config-path bench/configs/synthetic-corpus.yaml \
  --output-dir /tmp/hyperbench-bundle

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter fixture \
  --corpus-path /tmp/hyperbench-bundle \
  --output-dir /tmp/hyperbench-run-smoke \
  --mode smoke

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench report \
  --run-dir /tmp/hyperbench-run-smoke \
  --output-dir /tmp/hyperbench-report-smoke
```

Expected smoke outputs:

- `/tmp/hyperbench-run-smoke/summary.json`
- `/tmp/hyperbench-run-smoke/events.jsonl`
- `/tmp/hyperbench-run-smoke/metrics.jsonl`
- `/tmp/hyperbench-run-smoke/query_results.csv`
- `/tmp/hyperbench-run-smoke/refresh_results.csv`
- `/tmp/hyperbench-run-smoke/metric_summaries.csv`
- `/tmp/hyperbench-report-smoke/report.json`
- `/tmp/hyperbench-report-smoke/report.md`

## Real Symbol Smoke Run

This runs the Phase 1 symbol query pack through the real Phase 4 parser/symbol engine.

```bash
cargo build -p hyperindex-daemon

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora generate-synth \
  --config-path bench/configs/synthetic-corpus.yaml \
  --output-dir /tmp/hyperbench-bundle-symbol

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter daemon \
  --engine-bin "$(pwd)/target/debug/hyperd" \
  --daemon-build-temperature cold \
  --corpus-path /tmp/hyperbench-bundle-symbol \
  --query-pack-id synthetic-saas-medium-symbol-pack \
  --output-dir /tmp/hyperbench-run-symbol-smoke \
  --mode smoke

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench report \
  --run-dir /tmp/hyperbench-run-symbol-smoke \
  --output-dir /tmp/hyperbench-report-symbol-smoke
```

The daemon adapter starts its own temporary runtime workspace automatically. No manual daemon start
or repo registration step is required.

If Unix-domain sockets are permitted, the adapter uses the long-lived daemon transport. In more
restricted environments it falls back to the same daemon protocol over stdio and keeps the runtime
state on disk so the benchmark still runs end to end.

## Real Impact Smoke Run

This runs the Phase 1 impact query pack through the real Phase 5 impact engine over the daemon
protocol.

```bash
cargo build -p hyperindex-daemon

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora generate-synth \
  --config-path bench/configs/synthetic-corpus.yaml \
  --output-dir /tmp/hyperbench-bundle-impact

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter daemon-impact \
  --engine-bin "$(pwd)/target/debug/hyperd" \
  --daemon-build-temperature cold \
  --corpus-path /tmp/hyperbench-bundle-impact \
  --query-pack-id synthetic-saas-medium-impact-pack \
  --output-dir /tmp/hyperbench-run-impact-smoke \
  --mode smoke

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench report \
  --run-dir /tmp/hyperbench-run-impact-smoke \
  --output-dir /tmp/hyperbench-report-impact-smoke
```

The impact adapter bootstraps its own temporary runtime workspace and does not require a manually
started daemon or repo registration step.

The current compare/golden flow remains useful even when the real impact engine does not yet match
the fixture baseline perfectly. `query_results.csv`, `summary.json`, and `compare.json` still
capture the machine-readable deltas needed for engine bring-up.

If the current daemon contract cannot resolve a checked-in impact target yet, the harness now keeps
the run alive and records that gap as an empty query result with diagnostic notes in
`query_results.csv` and `events.jsonl` instead of aborting the whole benchmark.

## Full Local Benchmark Run

The full run can still use the `FixtureAdapter`, but symbol benchmarking now also supports the real
daemon-backed engine.

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora generate-synth \
  --config-path bench/configs/synthetic-corpus.yaml \
  --output-dir /tmp/hyperbench-bundle-full

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter fixture \
  --corpus-path /tmp/hyperbench-bundle-full \
  --output-dir /tmp/hyperbench-run-full \
  --mode full
```

Real symbol-engine full run:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter daemon \
  --engine-bin "$(pwd)/target/debug/hyperd" \
  --daemon-build-temperature cold \
  --corpus-path /tmp/hyperbench-bundle-full \
  --query-pack-id synthetic-saas-medium-symbol-pack \
  --output-dir /tmp/hyperbench-run-symbol-full \
  --mode full
```

Real impact-engine full run:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter daemon-impact \
  --engine-bin "$(pwd)/target/debug/hyperd" \
  --daemon-build-temperature cold \
  --corpus-path /tmp/hyperbench-bundle-full \
  --query-pack-id synthetic-saas-medium-impact-pack \
  --output-dir /tmp/hyperbench-run-impact-full \
  --mode full
```

The full symbol run emits:

- symbol query results for the whole checked-in symbol pack
- parser and symbol build metadata for the clean baseline build
- incremental refresh rows for the checked-in edit scenarios
- refresh mode and fallback fields so full-rebuild vs incremental behavior is comparable

The full impact run emits:

- impact query results for the whole checked-in impact pack
- clean-snapshot prerequisite build metadata plus impact analyze/materialization metadata
- refresh rows with additive impact fields such as:
  `impact_refresh_mode`, `impact_analyze_latency_ms`,
  `impact_refresh_elapsed_ms`, `impact_refresh_files_touched`,
  `impact_refresh_entities_recomputed`, and `impact_refresh_edges_refreshed`
- query-type metric summaries that include `impact-latency`

## Baseline vs Candidate Compare

Use two completed run directories and the default Phase 1 budgets:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench compare \
  --baseline-run-dir /tmp/hyperbench-run-baseline \
  --candidate-run-dir /tmp/hyperbench-run-candidate \
  --budgets-path bench/configs/budgets.yaml \
  --output-dir /tmp/hyperbench-compare
```

This writes:

- `/tmp/hyperbench-compare/compare.json`
- `/tmp/hyperbench-compare/compare.md`

The Markdown file is intended to be pasted into a PR description or issue comment.

Recommended symbol-engine compare pairs:

- fixture baseline vs daemon candidate
  Use the fixture symbol smoke run as the baseline and the daemon-backed symbol smoke run as the
  candidate.
- daemon cold vs daemon warm
  Re-run the daemon command with `--daemon-build-temperature warm` and compare the two result
  directories. The compare output now includes prepare/build latency metrics for this case.
- baseline vs candidate real-engine changes
  Capture one daemon-backed symbol run before a change and one after it, then compare those two run
  directories with the same command above.

For full-build vs incremental behavior, use the full daemon run and inspect:

- `summary.json`
- `metrics.jsonl`
- `refresh_results.csv`
- `report.json`

Those artifacts now record clean-build prepare metadata plus incremental refresh mode/latency data
in a machine-readable form.

Recommended impact-engine compare pairs:

- fixture baseline vs daemon candidate
  Use the fixture impact smoke or full run as the baseline and the daemon-backed impact run as the
  candidate. This is the fastest way to compare the fixture adapter against the real impact
  engine.
- daemon cold vs daemon warm
  Re-run the same `daemon-impact` command with `--daemon-build-temperature warm` and compare the
  two output directories. The compare artifact now includes `impact-latency` and
  `prepare-impact-analyze-latency` when those metrics are present.
- full compute vs incremental update behavior
  Use a full `daemon-impact` run and inspect:
  `summary.json`, `metrics.jsonl`, `refresh_results.csv`, and `report.json`.
  Those artifacts now expose whether the impact build reused a persisted baseline or performed a
  fresh full/incremental compute for each refresh scenario.

## Fixture Smoke Demo

If you want a single scripted demo:

```bash
bash bench/scripts/demo-phase1-smoke.sh
```

It generates a temporary synthetic corpus, runs the fixture smoke benchmark, renders a report, and prints the first part of the Markdown report.

Example report shape:

```md
# Hyperbench Run Report: fixture-synthetic-saas-medium-smoke-...

## Overview

- Adapter: `fixture`
- Mode: `smoke`
- Corpus: `synthetic-saas-medium`
- Query count: `4`
```

## Real Symbol Smoke Script

For a no-manual-steps symbol benchmark demo:

```bash
bash bench/scripts/symbol-query-smoke.sh
```

The script:

1. builds `hyperd`
2. generates a synthetic corpus bundle
3. runs a fixture symbol smoke baseline
4. runs a daemon-backed symbol smoke candidate
5. renders a daemon report
6. writes fixture-vs-daemon compare artifacts

## Real Impact Smoke Script

For a no-manual-steps impact benchmark demo:

```bash
bash bench/scripts/impact-query-smoke.sh
```

The script:

1. builds `hyperd`
2. generates a synthetic corpus bundle
3. runs a fixture impact smoke baseline
4. runs a daemon-backed impact smoke candidate
5. renders a daemon report
6. writes fixture-vs-daemon compare artifacts

## CI Smoke Path

Local equivalent of CI:

```bash
bash bench/scripts/ci-smoke.sh
```

The GitHub Actions workflow at [.github/workflows/phase1-smoke.yml](/Users/rishivinodkumar/RepoHyperindex/.github/workflows/phase1-smoke.yml) runs the same fast path:

- schema/config validation
- unit tests
- synthetic corpus generation
- fixture smoke benchmark
- report generation
- compare generation

## Real Corpora

Real-repo bootstrap remains optional and manual in Phase 1 because pinned refs and network access may not always be available.

Start with:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora bootstrap --dry-run
```

Then follow the corpus-specific process in [how-to-add-a-corpus.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/how-to-add-a-corpus.md).
