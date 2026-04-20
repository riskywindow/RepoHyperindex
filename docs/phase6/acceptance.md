# Repo Hyperindex Phase 6 Acceptance Contract

## Purpose

This document defines what Phase 6 must ship and what it must not ship.

If a required item below is missing, Phase 6 is not done.
If a prohibited item below ships, Phase 6 has widened past the approved slice.

## Phase 6 Must Ship

### Semantic retrieval engine

- A real semantic retrieval layer over snapshot-resolved TypeScript and JavaScript files.
- Deterministic symbol-first chunking with file fallback windows.
- One local-first embedding provider implementation.
- Snapshot-scoped persisted semantic build metadata.

### Runtime integration

- Compatible daemon protocol support for semantic status and semantic search.
- CLI support for semantic querying and semantic status.
- Incremental semantic refresh from `SnapshotDiffResponse`, including buffer-only changes.

### Harness integration

- A real daemon-backed semantic adapter through the existing Phase 1 harness.
- End-to-end execution of the checked-in synthetic semantic pack through `hyperbench`.
- Existing run/report/compare artifact contracts preserved.

### Determinism and observability

- Stable chunk identities and deterministic ranking tie-breakers.
- Machine-readable semantic build metadata.
- Diagnostics for disabled, missing, stale, or corrupt semantic state.

### Docs

- A current execution plan.
- A current status document with completed work and the next recommended prompt.
- Durable documentation of preserved interfaces and Phase 6 boundaries.

## Phase 6 Must Preserve

### Phase 1 harness contracts

- `SemanticQuery { text, path_globs, rerank_mode, limit }`
- normalized `QueryHit`
- `hyperbench` run/report/compare flows
- checked-in semantic packs and goldens

### Phase 2 runtime contracts

- `ComposedSnapshot`
- `SnapshotAssembler::resolve_file(...)`
- `SnapshotAssembler::diff(...)`
- repo registration, snapshot creation, and daemon lifecycle behavior

### Phase 3 exact-search ownership boundary

- semantic retrieval must not become exact file search
- semantic ranking may consume lexical features additively only

### Phase 4 symbol contracts

- `SymbolId`
- `SymbolRecord`
- existing `GraphEdgeKind` meanings
- parser/symbol build identity and metadata

### Phase 5 impact contracts

- `ImpactAnalyzeResponse` remains the blast-radius source of truth
- `ImpactEnrichmentPlan` is additive context only
- semantic search must not invoke `impact_analyze` in its hot path

## Phase 6 Must Not Ship

- answer generation, summaries, or chat responses
- LLM or cross-encoder reranking
- semantic edits, codemods, or write-side actions
- UI, browser, or VS Code product work
- cloud or multi-user runtime behavior
- a replacement exact-search engine
- a replacement parser/symbol/impact engine
- a mandatory hosted embedding dependency
- semantic ranking that probes the repo root outside the snapshot model

## Acceptance Checklist

Phase 6 is acceptable only if every item below can be answered `yes`.

### Chunking and retrieval

- Does the engine build symbol-first chunks with file fallbacks deterministically?
- Does every semantic hit anchor to a snapshot path and stable chunk identity?
- Can the engine return semantic hits without generating prose answers?

### Runtime behavior

- Can the daemon report semantic readiness through a status method?
- Can the daemon serve semantic search for a specific repo and snapshot?
- Does semantic indexing refresh incrementally from snapshot diffs, including buffer-only edits?

### Harness behavior

- Does the checked-in synthetic semantic pack run through the real engine via the existing harness
  contract?
- Do run/report/compare artifacts remain unchanged in shape?
- Does the hero query `semantic-hero-session-invalidation` pass at top-1?

### Scope control

- Is the default embedding path local-first?
- Is exact search still a separate ownership boundary?
- Is impact analysis still a separate ownership boundary?
- Are answer generation, UI, extension, cloud, and multi-user features still unshipped?

## Minimum Evidence For Phase Completion

The final Phase 6 closeout should include:

- targeted Rust tests for semantic chunking, search, store, and daemon transport
- targeted compatibility tests proving Phase 2, Phase 4, and Phase 5 flows still work
- daemon-backed semantic harness validation with smoke and full synthetic runs
- machine-readable latency and pass-rate results from `hyperbench`
- updated Phase 6 docs with command results, risks, and remaining boundaries
