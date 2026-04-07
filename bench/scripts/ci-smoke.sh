#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

export UV_CACHE_DIR="${UV_CACHE_DIR:-/tmp/uv-cache}"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hyperbench-ci-smoke.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

BUNDLE_DIR="$TMP_DIR/bundle"
BASELINE_DIR="$TMP_DIR/baseline"
CANDIDATE_DIR="$TMP_DIR/candidate"
REPORT_DIR="$TMP_DIR/report"
COMPARE_DIR="$TMP_DIR/compare"

echo "[ci-smoke] validating configs"
uv run hyperbench corpora validate

echo "[ci-smoke] running unit tests"
uv run pytest

echo "[ci-smoke] generating synthetic corpus"
uv run hyperbench corpora generate-synth \
  --config-path bench/configs/synthetic-corpus.yaml \
  --output-dir "$BUNDLE_DIR"

echo "[ci-smoke] running baseline fixture smoke benchmark"
uv run hyperbench run \
  --adapter fixture \
  --corpus-path "$BUNDLE_DIR" \
  --output-dir "$BASELINE_DIR" \
  --mode smoke

echo "[ci-smoke] running candidate fixture smoke benchmark"
uv run hyperbench run \
  --adapter fixture \
  --corpus-path "$BUNDLE_DIR" \
  --output-dir "$CANDIDATE_DIR" \
  --mode smoke

echo "[ci-smoke] generating report"
uv run hyperbench report \
  --run-dir "$CANDIDATE_DIR" \
  --output-dir "$REPORT_DIR"

echo "[ci-smoke] generating compare artifacts"
uv run hyperbench compare \
  --baseline-run-dir "$BASELINE_DIR" \
  --candidate-run-dir "$CANDIDATE_DIR" \
  --budgets-path bench/configs/budgets.yaml \
  --output-dir "$COMPARE_DIR"

echo "[ci-smoke] completed successfully"
echo "[ci-smoke] report.md -> $REPORT_DIR/report.md"
echo "[ci-smoke] compare.md -> $COMPARE_DIR/compare.md"
