# Repo Hyperindex Phase 5 Acceptance Contract

## Purpose

This document defines what Phase 5 must ship and what it must not ship.

If a required item below is missing, Phase 5 is not done.
If a prohibited item below ships, scope has widened past the approved phase.

## Current Closeout Note

As of 2026-04-13, Phase 5 closes with shipped first-class input support for:

- `symbol`
- `file`

`config_key` remains intentionally deferred to Phase 6. The checked-in harness keeps
config-backed benchmark compatibility by degrading those queries to backing-file impact requests at
the adapter boundary.

## Phase 5 Must Ship

### Impact engine

- A real impact-analysis layer over the checked-in Phase 4 symbol graph.
- Deterministic direct and transitive impact traversal.
- Deterministic target resolution for `symbol` and `file`.
- Deterministic certainty tiers with visible reason paths.

### Ranking and outputs

- Ranked impact outputs for `symbol`, `file`, `package`, and `test`.
- Stable ordering rules for ties.
- No hidden semantic guessing behind one opaque score.

### Incremental updates

- Incremental impact refresh from snapshot diffs.
- Incremental impact refresh for buffer-overlay snapshots.
- Full-rebuild fallback rules that are explicit and testable.

### Runtime integration

- Compatible Phase 5 protocol extensions on the existing local daemon contract.
- Daemon integration that remains snapshot-scoped.
- CLI integration for impact analysis.

### Harness integration

- A real daemon-backed impact path through the existing Phase 1 harness adapter seam.
- End-to-end execution of the checked-in synthetic impact pack through `hyperbench`.
- Preserved harness artifact names and result normalization.

### Docs and reviewability

- A current execution plan.
- A current status document with commands run, risks, and the next recommended prompt.
- A decisions log capturing durable architecture choices.

## Phase 5 Must Preserve

### Phase 0 wedge

- local-first behavior
- TypeScript/JavaScript focus
- freshness from snapshots, working tree, and unsaved buffers

### Phase 1 benchmark contracts

- `hyperbench` public commands
- the existing adapter responsibilities
- typed query-pack, golden-set, run, report, and compare artifact contracts
- the checked-in impact query packs, goldens, and compare-budget files

### Phase 2 runtime contracts

- repo registration and repo identity flow
- snapshot creation, read-file, and diff behavior
- buffer storage and overlay precedence
- daemon transport and request/response model
- watcher event normalization as an input seam

### Phase 3 exact-search ownership boundary

- impact must not become an exact-search implementation
- exact search must remain optional for core impact results
- shared snapshot-derived file eligibility remains compatible and separable

### Phase 4 symbol contracts

- existing symbol identities
- existing symbol query methods
- existing `GraphEdgeKind` meanings
- existing symbol build metadata needed by compare/report flows

## Phase 5 Must Not Ship

- semantic retrieval, embeddings, reranking, or answer generation
- a checked-in exact-search engine
- compiler-grade semantic binding
- call-graph, inheritance, or data-flow reasoning sold as precise impact
- codemods, auto-refactors, or write-side editor actions
- a VS Code extension
- a browser UI or dashboard
- cloud sync, team sharing, or multi-user runtime behavior
- framework-specific route indexing beyond file-backed route evidence
- schema-breaking changes to Phase 1 harness assets unless separately approved
- replacement of the Phase 2 snapshot and daemon contracts
- replacement of the Phase 4 symbol graph as the source of syntax-derived truth

## Acceptance Checklist

Phase 5 is acceptable only if every item below can be answered `yes`.

### Target resolution and traversal

- Can the engine resolve `symbol` and `file` targets deterministically?
- Can it produce both direct and transitive impact answers?
- Does every non-self hit carry a certainty tier and at least one reason path?

### Ranking and output kinds

- Can the engine rank `symbol`, `file`, `package`, and `test` outputs?
- Are tie-breakers deterministic?
- Is the top hit stable for the checked-in synthetic hero query?

### Incremental behavior

- Does a one-file working-tree edit avoid recomputing the entire impact projection in the normal
  case?
- Can buffer-only edits refresh impact without discarding snapshot correctness?
- Are full-rebuild fallbacks explicit when invariants are violated?

### Runtime and public surface

- Does the daemon expose a compatible impact-analysis method?
- Does the CLI expose a usable impact command?
- Are requests and responses still snapshot-scoped and local-only?

### Harness preservation

- Do existing `hyperbench` commands still work?
- Does the checked-in synthetic impact pack run through the real engine via the existing adapter
  seam?
- Are run/report/compare artifact names unchanged?

### Scope control

- Is semantic retrieval still unshipped?
- Is an exact-search engine still unshipped?
- Are UI, extension, codemod, cloud, and team-sharing features still unshipped?

## Minimum Evidence For Phase Completion

The final Phase 5 closeout should include:

- targeted Rust test results for impact, impact-store, daemon, and CLI crates
- targeted Python harness validation proving impact adapter compatibility
- benchmark results against the checked-in synthetic impact pack
- incremental refresh measurements for working-tree and buffer-only scenarios
- updated Phase 5 execution, status, and decisions docs
- a Phase 5 handoff that gives Phase 6 the exact plug-in seams
- explicit confirmation that all hard non-goals remain out of scope
