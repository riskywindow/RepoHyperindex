# Repo Hyperindex Phase 2 Acceptance Contract

## Purpose

This document defines what Phase 2 must ship and what it must not ship.

If an implementation misses a required item below, Phase 2 is not done.
If an implementation ships a prohibited item below, Phase 2 has widened scope and must be
explicitly re-approved.

## Phase 2 Must Ship

### Runtime spine

- A Rust-stable workspace for product/runtime code outside `bench/`.
- A local daemon/runtime boundary, even if the earliest smoke path is CLI-driven.
- A CLI smoke path that proves the runtime can be configured and exercised locally.

### Protocol and config

- A versioned local protocol contract with typed request/response shapes.
- A versioned local config contract with documented defaults and version checks.
- Clear daemon/status error categories instead of raw subprocess or watcher failures.

### Persistence

- A persistent repo registry.
- A persistent manifest store for immutable snapshot manifests.
- Persistent scheduler/event state sufficient to recover basic daemon status after restart.

### Repo state ingestion

- Git working-tree introspection for repo root, branch or detached HEAD, commit, and normalized
  status.
- File watching with normalized event records.
- Snapshot records for:
  - immutable base snapshots
  - working-tree overlays
  - buffer overlays

### Scheduler and status

- A scheduler model with documented job states.
- A status API or command that reports repo/runtime state.
- A deterministic enough smoke path to validate repo registration, snapshot capture, and status.

### Docs and reviewability

- An execution plan that remains concrete and current.
- A status document that records completed work, commands run, remaining risks, and the next
  recommended prompt.
- A decisions log for durable architecture choices made during implementation.

## Phase 2 Must Preserve

### Phase 1 harness commands

These commands must continue to work with the same Phase 1 intent:

- `hyperbench status`
- `hyperbench corpora validate`
- `hyperbench corpora bootstrap`
- `hyperbench corpora snapshot`
- `hyperbench corpora generate-synth`
- `hyperbench run`
- `hyperbench report`
- `hyperbench compare`

### Phase 1 harness artifact contracts

These file names and responsibilities must remain intact:

- `summary.json`
- `events.jsonl`
- `metrics.jsonl`
- `query_results.csv`
- `refresh_results.csv`
- `metric_summaries.csv`
- `report.json`
- `report.md`
- `compare.json`
- `compare.md`

### Phase 1 adapter seam

Phase 2 integration must remain compatible with the existing adapter surface:

- `prepare_corpus`
- `execute_exact_query`
- `execute_symbol_query`
- `execute_semantic_query`
- `execute_impact_query`
- `run_incremental_refresh`

### Product wedge

Phase 2 must preserve the long-term wedge from Phase 0:

- local-first
- TypeScript impact engine
- freshness from branch, working tree, and buffers

Phase 2 may build only the runtime substrate for that wedge, not the intelligence layer itself.

## Phase 2 Must Not Ship

- parsing or parser infrastructure beyond raw file/snapshot metadata
- exact search execution or ranking
- symbol extraction or navigation
- semantic retrieval or embedding logic
- impact analysis or blast-radius logic
- a VS Code extension
- a browser UI, dashboard, or speculative frontend
- cloud sync
- multi-user service behavior
- remote daemon or remote control plane
- benchmark scoring logic moved out of the Phase 1 harness
- schema-breaking changes to Phase 1 corpora, query pack, golden, or compare contracts unless
  explicitly approved as a separate task

## Acceptance Checklist

Phase 2 is acceptable only if every item below can be answered `yes`.

### Runtime and protocol

- Is there a documented and implemented local runtime/daemon boundary?
- Is the protocol versioned and typed?
- Is the config versioned and typed?

### Persistence and repo state

- Can the runtime persist repo registration across process restarts?
- Can it persist immutable snapshot manifests?
- Can it reconstruct basic scheduler/status state after restart?
- Can it normalize file-watch events into a durable event representation?
- Can it inspect git state without relying on network access?

### Snapshots

- Are base snapshots immutable?
- Are working-tree and buffer overlays represented separately?
- Is overlay precedence documented and tested?
- Are snapshot paths normalized relative to repo root?

### Scheduler and status

- Is there a documented scheduler model with explicit job states?
- Can the CLI surface repo and runtime status cleanly?
- Is there a smoke path for repo register -> status -> snapshot capture?

### Phase 1 preservation

- Do the existing `hyperbench` public commands still work?
- Do the existing Phase 1 artifact names still exist with the same responsibilities?
- Does the harness remain the owner of report and compare logic?

### Scope control

- Does the implementation avoid parsing, exact search, symbol extraction, semantic retrieval,
  and impact analysis?
- Does the implementation avoid UI, extension, and cloud work?

## Minimum Evidence For Phase Completion

The final Phase 2 closeout should include:

- validation commands and results for runtime crates
- a CLI smoke transcript or summarized output for status and snapshot flow
- targeted Phase 1 preservation validation
- updated Phase 2 status and decisions docs
- explicit confirmation that all hard non-goals remain unshipped
