# Hyperbench Phase 1

This directory contains the Phase 1 benchmark harness for Repo Hyperindex.

Phase 1 scope in this repo:

- deterministic synthetic TypeScript corpora
- typed query packs and goldens
- corpus validation, bootstrap planning, and snapshot metadata
- a fixture-backed benchmark runner plus real daemon-backed symbol and impact adapters
- machine-readable run outputs
- report and compare commands
- smoke CI coverage

Still out of scope:

- exact and semantic real-engine adapters
- VS Code extension, UI, or cloud service

## Quick Start

Fresh clone:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv sync
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench --help
```

Repo health checks:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run ruff check .
UV_CACHE_DIR=/tmp/uv-cache uv run pytest
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora validate
```

## Common Commands

Generate the canonical synthetic corpus bundle:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora generate-synth \
  --config-path bench/configs/synthetic-corpus.yaml \
  --output-dir /tmp/hyperbench-bundle
```

Run the fast local smoke benchmark:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter fixture \
  --corpus-path /tmp/hyperbench-bundle \
  --output-dir /tmp/hyperbench-run-smoke \
  --mode smoke
```

Run the real symbol-engine smoke benchmark against the Phase 4 daemon path:

```bash
cargo build -p hyperindex-daemon

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter daemon \
  --engine-bin "$(pwd)/target/debug/hyperd" \
  --daemon-build-temperature cold \
  --corpus-path /tmp/hyperbench-bundle \
  --query-pack-id synthetic-saas-medium-symbol-pack \
  --output-dir /tmp/hyperbench-run-symbol-smoke \
  --mode smoke
```

Run the real impact-engine smoke benchmark against the Phase 5 daemon path:

```bash
cargo build -p hyperindex-daemon

UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter daemon-impact \
  --engine-bin "$(pwd)/target/debug/hyperd" \
  --daemon-build-temperature cold \
  --corpus-path /tmp/hyperbench-bundle \
  --query-pack-id synthetic-saas-medium-impact-pack \
  --output-dir /tmp/hyperbench-run-impact-smoke \
  --mode smoke
```

Run the full local benchmark against the fixture adapter:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter fixture \
  --corpus-path /tmp/hyperbench-bundle \
  --output-dir /tmp/hyperbench-run-full \
  --mode full
```

Run the full real symbol benchmark, including incremental refresh scenarios:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter daemon \
  --engine-bin "$(pwd)/target/debug/hyperd" \
  --daemon-build-temperature cold \
  --corpus-path /tmp/hyperbench-bundle \
  --query-pack-id synthetic-saas-medium-symbol-pack \
  --output-dir /tmp/hyperbench-run-symbol-full \
  --mode full
```

Run the full real impact benchmark, including incremental impact refresh scenarios:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter daemon-impact \
  --engine-bin "$(pwd)/target/debug/hyperd" \
  --daemon-build-temperature cold \
  --corpus-path /tmp/hyperbench-bundle \
  --query-pack-id synthetic-saas-medium-impact-pack \
  --output-dir /tmp/hyperbench-run-impact-full \
  --mode full
```

Generate a report for a completed run:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench report \
  --run-dir /tmp/hyperbench-run-smoke \
  --output-dir /tmp/hyperbench-report
```

Compare a baseline run against a candidate run:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench compare \
  --baseline-run-dir /tmp/hyperbench-run-smoke-baseline \
  --candidate-run-dir /tmp/hyperbench-run-smoke-candidate \
  --budgets-path bench/configs/budgets.yaml \
  --output-dir /tmp/hyperbench-compare
```

Useful symbol-engine compare pairs:

- fixture baseline vs daemon candidate:
  compare `/tmp/hyperbench-run-smoke` or `/tmp/hyperbench-run-symbol-smoke`
- daemon cold vs daemon warm:
  rerun the daemon command with `--daemon-build-temperature warm` and compare the two run dirs
- full build vs incremental behavior:
  inspect `summary.json` `prepare`/`refresh_summary`, `metrics.jsonl`, and `refresh_results.csv`
  from the full daemon run; those now record cold/warm build metadata plus incremental/full-rebuild
  refresh modes and latencies

Useful impact-engine compare pairs:

- fixture baseline vs daemon candidate:
  compare a fixture impact smoke or full run against the matching `daemon-impact` run to surface
  accuracy and latency deltas in one machine-readable compare artifact
- daemon cold vs daemon warm:
  rerun the same `daemon-impact` command with `--daemon-build-temperature warm` and compare the
  two run dirs to isolate prerequisite build/materialization reuse
- full compute vs incremental update:
  inspect `refresh_results.csv`, `metrics.jsonl`, and `summary.json` from a full `daemon-impact`
  run; the impact adapter now records `impact_refresh_mode`, `impact_analyze_latency_ms`, and
  additive persisted-build refresh stats

When the current daemon contract cannot resolve a checked-in impact target yet, the harness records
that as an empty impact query result with diagnostic notes instead of aborting the run. This keeps
full-pack benchmarking and compare/report generation usable during engine bring-up.

## Scripts

Useful helper scripts:

- `bench/scripts/ci-smoke.sh`
  Runs the same fast deterministic path that CI uses. Invoke with
  `bash bench/scripts/ci-smoke.sh`.
- `bench/scripts/demo-phase1-smoke.sh`
  Generates a synthetic corpus, runs the fixture smoke benchmark, and prints a short Markdown report preview. Invoke with
  `bash bench/scripts/demo-phase1-smoke.sh`.
- `bench/scripts/symbol-query-smoke.sh`
  Builds `hyperd`, generates a synthetic bundle, runs fixture and daemon-backed symbol smoke
  benchmarks, and writes report/compare artifacts without manual setup.
- `bench/scripts/impact-query-smoke.sh`
  Builds `hyperd`, generates a synthetic bundle, runs fixture and daemon-backed impact smoke
  benchmarks, and writes report/compare artifacts without manual setup.
- `bench/scripts/profile-harness.sh`
  Profiles the Python harness with `cProfile`.
- `bench/scripts/profile-rust-engine-placeholder.sh`
  Placeholder guidance for future Rust engine profiling.

## Output Shape

`hyperbench run` writes:

- `summary.json`
- `events.jsonl`
- `metrics.jsonl`
- `query_results.csv`
- `refresh_results.csv`
- `metric_summaries.csv`

For daemon-backed symbol or impact runs, those artifacts now also include:

- benchmark dimensions such as transport and cold/warm build temperature
- prepare/build metadata for parser and symbol-index bring-up
- adapter-specific refresh mode and fallback details for incremental daemon refresh scenarios
- additive impact metrics such as `prepare-impact-analyze-latency` and
  `refresh-impact-analyze-latency` when `--adapter daemon-impact` is used

`hyperbench report` writes:

- `report.json`
- `report.md`

`hyperbench compare` writes:

- `compare.json`
- `compare.md`

## More Docs

- [execution-plan.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/execution-plan.md)
- [benchmark-spec.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/benchmark-spec.md)
- [how-to-run-phase1.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/how-to-run-phase1.md)
- [how-to-add-a-corpus.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/how-to-add-a-corpus.md)
- [phase1-handoff.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/phase1-handoff.md)
