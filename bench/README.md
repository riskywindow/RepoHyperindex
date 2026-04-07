# Hyperbench Phase 1

This directory contains the Phase 1 benchmark harness for Repo Hyperindex.

Phase 1 scope in this repo:

- deterministic synthetic TypeScript corpora
- typed query packs and goldens
- corpus validation, bootstrap planning, and snapshot metadata
- a fixture-backed benchmark runner
- machine-readable run outputs
- report and compare commands
- smoke CI coverage

Still out of scope:

- the real Hyperindex daemon
- a real query engine or Rust engine implementation
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

Run the full local benchmark against the fixture adapter:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter fixture \
  --corpus-path /tmp/hyperbench-bundle \
  --output-dir /tmp/hyperbench-run-full \
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

## Scripts

Useful helper scripts:

- `bench/scripts/ci-smoke.sh`
  Runs the same fast deterministic path that CI uses. Invoke with
  `bash bench/scripts/ci-smoke.sh`.
- `bench/scripts/demo-phase1-smoke.sh`
  Generates a synthetic corpus, runs the fixture smoke benchmark, and prints a short Markdown report preview. Invoke with
  `bash bench/scripts/demo-phase1-smoke.sh`.
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
