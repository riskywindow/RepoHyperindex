#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

export UV_CACHE_DIR="${UV_CACHE_DIR:-/tmp/uv-cache}"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hyperbench-demo.XXXXXX")"
trap 'echo "demo artifacts kept at $TMP_DIR"' EXIT

BUNDLE_DIR="$TMP_DIR/bundle"
RUN_DIR="$TMP_DIR/run"
REPORT_DIR="$TMP_DIR/report"

uv run hyperbench corpora generate-synth \
  --config-path bench/configs/synthetic-corpus.yaml \
  --output-dir "$BUNDLE_DIR"

uv run hyperbench run \
  --adapter fixture \
  --corpus-path "$BUNDLE_DIR" \
  --output-dir "$RUN_DIR" \
  --mode smoke

uv run hyperbench report \
  --run-dir "$RUN_DIR" \
  --output-dir "$REPORT_DIR"

echo
echo "Report preview:"
echo "--------------"
sed -n '1,40p' "$REPORT_DIR/report.md"
