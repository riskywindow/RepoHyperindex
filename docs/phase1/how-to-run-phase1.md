# How To Run Phase 1

This guide is the operator-facing walkthrough for the Phase 1 Hyperbench harness.

## Prerequisites

- Python `3.12+`
- [`uv`](https://docs.astral.sh/uv/) available locally
- a fresh clone of the repo

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

## Full Local Benchmark Run

The full run still uses the `FixtureAdapter`, but it executes the full synthetic query surface instead of the smaller smoke selection.

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
