#!/usr/bin/env bash
set -euo pipefail

echo "This is a Phase 1 placeholder for future Rust engine profiling."
echo "Expected future usage:"
echo "  1. build the Rust engine binary with profiling symbols"
echo "  2. run hyperbench with --adapter shell --engine-bin <path>"
echo "  3. wrap the engine invocation with the profiler used by your platform"
echo
echo "Suggested placeholders:"
echo "  macOS:  xctrace / Instruments"
echo "  Linux:  perf, heaptrack, or valgrind massif"
echo "  CI:     collect benchmark outputs first, then attach profiler artifacts separately"
