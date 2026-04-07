#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: bench/scripts/profile-harness.sh <corpus-bundle-dir> <output-dir> [extra hyperbench run args...]" >&2
  exit 1
fi

CORPUS_PATH="$1"
OUTPUT_DIR="$2"
shift 2

mkdir -p "$OUTPUT_DIR"

UV_CACHE_DIR="${UV_CACHE_DIR:-/tmp/uv-cache}" \
uv run python -m cProfile -o "$OUTPUT_DIR/hyperbench-run.prof" -m hyperbench.cli run \
  --adapter fixture \
  --corpus-path "$CORPUS_PATH" \
  --output-dir "$OUTPUT_DIR/run" \
  "$@"

echo "Harness profile written to $OUTPUT_DIR/hyperbench-run.prof"
