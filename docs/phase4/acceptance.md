# Repo Hyperindex Phase 4 Acceptance Contract

## Purpose

This document defines what Phase 4 must ship and what it must not ship.

If a required item below is missing, Phase 4 is not done.
If a prohibited item below ships, scope has widened past the approved phase.

## Phase 4 Must Ship

### Parser and extraction

- Incremental parsing for TS/JS source files resolved from Phase 2 snapshots.
- Deterministic extraction of declarations, imports, exports, containment, and identifier
  occurrences needed for symbol lookup, definitions, and references.
- Buffer-overlay-aware parsing behavior that respects the existing snapshot precedence rules.

### Persistence and graph

- A persistent symbol/fact store separate from the existing control-plane runtime store.
- Durable storage for indexed snapshot/file relationships.
- A deterministic graph covering containment, import, export, and reference edges for this phase.

### Query surface

- Protocol, daemon, and CLI support for symbol lookup.
- Protocol, daemon, and CLI support for definitions.
- Protocol, daemon, and CLI support for references.
- A basic explain or inspect path that exposes the evidence behind a symbol result.

### Incremental updates

- Incremental refresh from snapshot diffs.
- Incremental refresh for unsaved buffer-overlay snapshots.
- File-level reparsing rather than full-repo rebuilds for common small-edit cases.

### Harness integration

- A real engine path that remains compatible with the existing Phase 1 harness adapter seam.
- End-to-end execution of the checked-in synthetic symbol pack through `hyperbench`.
- Recorded latency and pass-rate behavior using the existing harness artifact contracts.

### Docs and reviewability

- A current execution plan.
- A current status document with commands run, risks, and the next recommended prompt.
- A decisions log capturing durable architecture choices.

## Phase 4 Must Preserve

### Phase 0 wedge

- local-first behavior
- TypeScript/JavaScript focus
- freshness from snapshots, working tree, and unsaved buffers

### Phase 1 benchmark contracts

- `hyperbench` public commands
- the existing adapter responsibilities
- typed query-pack, golden-set, run, report, and compare artifact contracts
- the checked-in symbol query assets and budgets

### Phase 2 runtime contracts

- repo registration and repo identity flow
- snapshot creation, read-file, and diff behavior
- buffer storage and overlay precedence
- daemon transport and request/response model
- watcher event normalization as an input seam

### Exact-search ownership boundary

- symbol indexing must not become the owner of exact-text indexing
- searchable-file handling must remain shareable and snapshot-derived, not symbol-store-owned

## Phase 4 Must Not Ship

- semantic retrieval, embeddings, or reranking
- impact analysis or blast-radius views
- call-graph or data-flow inference sold as precise behavior
- codemods, auto-refactors, or write-side editor actions
- a VS Code extension
- a browser UI or dashboard
- cloud sync, team sharing, or multi-user runtime behavior
- a required Node or TypeScript compiler dependency for the core symbol path
- schema-breaking changes to Phase 1 harness assets unless separately approved
- replacement of the Phase 2 snapshot and daemon contracts

## Acceptance Checklist

Phase 4 is acceptable only if every item below can be answered `yes`.

### Parser and facts

- Can the engine parse supported TS/JS files from a `ComposedSnapshot`?
- Does it preserve buffer overlay, then working-tree overlay, then base snapshot precedence?
- Are extracted declarations/imports/exports/occurrences deterministic?

### Persistence and graph

- Is there a durable symbol store separate from the Phase 2 control-plane store?
- Can indexed snapshot/file relationships survive daemon restart?
- Can the graph answer containment, import/export, and reference questions deterministically?

### Query behavior

- Can the public surface resolve symbol lookup by exact name?
- Can it return canonical definitions?
- Can it return exact resolved references?
- Can it explain the evidence behind a symbol result?

### Incremental updates

- Does a single-file edit avoid a full-repo rebuild in the normal case?
- Can buffer-only edits be queried without discarding snapshot correctness?
- Is snapshot diffing the trigger for symbol refresh behavior?

### Harness preservation

- Do existing `hyperbench` commands still work?
- Does the Phase 1 symbol benchmark run against the real engine through the current adapter seam?
- Are run/report/compare file names unchanged?

### Scope control

- Is semantic retrieval still unshipped?
- Is impact analysis still unshipped?
- Are UI, extension, codemod, cloud, and team-sharing features still unshipped?

## Minimum Evidence For Phase Completion

The final Phase 4 closeout should include:

- targeted Rust test results for parser, symbol, store, and daemon crates
- targeted Phase 1 harness validation for symbol queries
- benchmark results against the checked-in synthetic symbol pack
- documented latency and refresh measurements
- updated Phase 4 execution, status, and decisions docs
- explicit confirmation that all hard non-goals remain out of scope
