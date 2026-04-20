#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/hyperbench-impact-smoke.XXXXXX")"
bundle_dir="$tmp_root/bundle"
fixture_dir="$tmp_root/fixture-smoke"
daemon_dir="$tmp_root/daemon-impact-smoke"
report_dir="$tmp_root/report"
compare_dir="$tmp_root/compare"
hyperd_bin="$workspace_root/target/debug/hyperd"

printf 'Working directory: %s\n' "$tmp_root"
printf 'Building hyperd...\n'
cargo build -p hyperindex-daemon >/dev/null

printf 'Generating synthetic corpus bundle...\n'
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora generate-synth \
  --config-path "$workspace_root/bench/configs/synthetic-corpus.yaml" \
  --output-dir "$bundle_dir" >/dev/null

printf 'Running fixture impact smoke baseline...\n'
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter fixture \
  --corpus-path "$bundle_dir" \
  --query-pack-id synthetic-saas-medium-impact-pack \
  --output-dir "$fixture_dir" \
  --mode smoke

printf 'Running daemon-backed impact smoke candidate...\n'
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter daemon-impact \
  --engine-bin "$hyperd_bin" \
  --daemon-build-temperature cold \
  --corpus-path "$bundle_dir" \
  --query-pack-id synthetic-saas-medium-impact-pack \
  --output-dir "$daemon_dir" \
  --mode smoke

printf 'Rendering daemon report...\n'
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench report \
  --run-dir "$daemon_dir" \
  --output-dir "$report_dir"

printf 'Comparing fixture baseline vs daemon candidate...\n'
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench compare \
  --baseline-run-dir "$fixture_dir" \
  --candidate-run-dir "$daemon_dir" \
  --budgets-path "$workspace_root/bench/configs/budgets.yaml" \
  --output-dir "$compare_dir"

printf '\nArtifacts:\n'
printf '  fixture run: %s\n' "$fixture_dir"
printf '  daemon run:  %s\n' "$daemon_dir"
printf '  report:      %s\n' "$report_dir"
printf '  compare:     %s\n' "$compare_dir"
