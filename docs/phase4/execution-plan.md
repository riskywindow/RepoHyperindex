# Repo Hyperindex Phase 4 Execution Plan

## Purpose

Phase 4 turns the existing local runtime spine into the first real symbol engine for Repo
Hyperindex.

The goal is to add incremental TS/JS parsing, deterministic file-fact extraction, persistent
symbol storage, symbol/reference/import/export/containment graph construction, and snapshot-aware
symbol query APIs while preserving:

- the Phase 0 wedge from [repo_hyperindex_phase0.md](/Users/rishivinodkumar/RepoHyperindex/repo_hyperindex_phase0.md):
  local-first TypeScript impact engine
- the Phase 1 benchmark harness and public symbol-evaluation contracts under
  [bench/](/Users/rishivinodkumar/RepoHyperindex/bench)
- the Phase 2 runtime as the source of truth for repo registration, snapshots, buffers, watcher
  events, daemon lifecycle, and CLI transport

This phase still does not ship semantic retrieval, impact analysis, codemods, editor UI, cloud
sync, or any Phase 5 work.

## Final Phase 4 Scope

Phase 4 includes only the following deliverables:

1. Incremental parsing for `.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, and `.cts` files resolved from a
   [ComposedSnapshot](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs).
2. Deterministic file-fact extraction for declarations, imports, exports, containment, and
   identifier occurrences needed for symbol lookup, definitions, and references.
3. A persistent symbol/fact store that is durable across daemon restarts and keyed to repo and
   snapshot state.
4. A deterministic graph layer covering symbol/reference/import/export/containment edges for this
   phase.
5. Symbol query APIs and CLI commands for:
   - symbol lookup
   - definitions
   - references
   - basic symbol inspection / explain output
6. Incremental update machinery driven by snapshot diffs and buffer overlays.
7. Daemon and CLI integration that keeps Phase 2 transport, repo, snapshot, and watcher seams
   intact.
8. Harness integration so the Phase 1 symbol benchmark path can execute against the real symbol
   engine without redesigning `hyperbench`.
9. Durable docs, acceptance criteria, and validation targets that keep the rest of Phase 4
   incremental and reviewable.

## Explicit Non-Goals

Phase 4 must not implement any of the following:

- semantic retrieval, embeddings, reranking, or vector storage
- impact analysis, blast-radius inference, or test recommendation
- codemods, automated refactors, or write-side IDE actions
- a VS Code extension, web UI, or operator dashboard
- cloud sync, team sharing, or multi-user runtime behavior
- a production TypeScript type-checker or full compiler-grade semantic model
- inferred call graphs, inheritance graphs, or dynamic dispatch reasoning
- Phase 1 harness schema changes unless separately approved
- replacement of the Phase 2 snapshot, repo, daemon, or CLI contracts
- replacement of any future exact-search engine with symbol-driven file discovery

## Preservation Audit

Phase 4 planning is constrained by the current checked-in contracts below.

### Phase 1 harness contracts to preserve

The Phase 1 harness remains the benchmark source of truth for symbol behavior.

Key adapter seam:

- [bench/hyperbench/adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  - `prepare_corpus`
  - `execute_symbol_query`
  - `run_incremental_refresh`
  - normalized `QueryHit { path, symbol, rank, reason, score }`

Key run/report/compare flow:

- [bench/hyperbench/runner.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/runner.py)
- [bench/README.md](/Users/rishivinodkumar/RepoHyperindex/bench/README.md)
- [docs/phase1/benchmark-spec.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/benchmark-spec.md)

Phase 1 symbol-query source artifacts present in this repo:

- [synthetic-saas-medium-symbol-pack.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/synthetic-saas-medium-symbol-pack.json)
- [synthetic-saas-medium-symbol-goldens.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens/synthetic-saas-medium-symbol-goldens.json)
- [query-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-pack.yaml)
- [golden-set.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/golden-set.yaml)

Phase 1 benchmark budgets present in this repo:

- [budgets.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/budgets.yaml)
- [compare-budget.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/compare-budget.yaml)

Planning implication:

- Phase 4 must adapt its symbol engine output to the existing harness result shape.
- Query behavior should be benchmarked against the checked-in symbol pack before any wider symbol
  API ambitions.

### Phase 2 runtime contracts to preserve

The checked-in Phase 2 runtime is the source of truth for snapshots and daemon lifecycle.

Transport and request flow:

- [crates/hyperindex-protocol/src/api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
- [crates/hyperindex-cli/src/client.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/client.rs)
- [crates/hyperindex-daemon/src/handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)

Repo and snapshot state:

- [crates/hyperindex-protocol/src/repo.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/repo.rs)
- [crates/hyperindex-protocol/src/snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
- [crates/hyperindex-snapshot/src/manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
- [crates/hyperindex-daemon/src/state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
- [crates/hyperindex-repo-store/src/manifests.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-repo-store/src/manifests.rs)

Watcher and normalized file-event inputs:

- [crates/hyperindex-protocol/src/watch.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/watch.rs)
- [crates/hyperindex-watcher/src/watcher.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-watcher/src/watcher.rs)
- [crates/hyperindex-cli/src/commands/watch.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/watch.rs)

Planning implication:

- Phase 4 parser/index code must consume files via `ComposedSnapshot` and `SnapshotAssembler`.
- Repo identity continues to come from `RepoRecord` plus `GitRepoState`.
- Buffer overlays remain first-class inputs and preserve the current precedence:
  buffer overlay, then working-tree overlay, then base snapshot.

### Phase 3 exact-search preservation boundary

There is no checked-in `docs/phase3/phase3-handoff.md`,
`docs/phase3/status.md`, `docs/phase4/*`, or current exact-search crate in this repository as of
April 7, 2026.

That means the practical preservation boundary for Phase 4 is:

- do not redefine the current Phase 2 repo/snapshot/watch lifecycle
- do not let symbol indexing become the source of truth for searchable-file discovery
- if exact search later lands, share only a compatible file-catalog abstraction derived from the
  same snapshot inputs

Final planning rule:

- searchable-file eligibility should be a shared abstraction over snapshot-resolved files and repo
  ignore rules
- exact text indexing and symbol indexing must remain separable consumers of that abstraction
- Phase 4 must not require exact search to route through the symbol store

## Proposed Runtime / Parser / Symbol File Tree

This is the target Phase 4 shape. Not every file must land in the first implementation slice.

```text
docs/
  phase4/
    execution-plan.md
    acceptance.md
    status.md
    decisions.md

crates/
  hyperindex-parser/
    src/
      lib.rs
      language.rs
      parse.rs
      incremental.rs
      tree_cache.rs
      ts_queries.rs
  hyperindex-symbols/
    src/
      lib.rs
      facts.rs
      extract.rs
      identities.rs
      graph.rs
      query.rs
      module_resolution.rs
      overlays.rs
  hyperindex-symbol-store/
    src/
      lib.rs
      db.rs
      migrations.rs
      snapshots.rs
      files.rs
      facts.rs
      graph.rs
      query.rs
  hyperindex-daemon/
    src/
      handlers.rs
      state.rs
      runtime.rs
  hyperindex-cli/
    src/
      commands/
        symbol.rs
  hyperindex-protocol/
    src/
      symbols.rs
      api.rs
```

Recommended responsibility split:

- `hyperindex-parser`
  owns syntax trees and incremental per-file parse reuse
- `hyperindex-symbols`
  owns language-specific file-fact extraction, symbol identity, graph construction, and overlay
  merge logic
- `hyperindex-symbol-store`
  owns durable persistence, snapshot/file-content dedupe, and query execution support
- existing daemon, CLI, and protocol crates remain the public surface

## Parser Candidates And Final Recommendation

### Candidate A: tree-sitter with TypeScript/TSX grammars

Pros:

- designed for incremental parsing
- strong fit with unsaved-buffer workflows
- mature Rust bindings
- syntax-tree performance aligns with local daemon use
- config-free enough for the local-first wedge

Cons:

- syntax only, not a compiler semantic model
- import/reference resolution must be built separately

### Candidate B: SWC parser

Pros:

- fast Rust parser
- good JS/TS AST coverage

Cons:

- not built around incremental tree reuse
- heavier AST ownership for live-edit update flows

### Candidate C: Oxc parser

Pros:

- modern Rust JS/TS tooling
- strong performance potential

Cons:

- incremental parsing story is weaker for this phase than tree-sitter
- would still require custom layering for live overlay updates

### Candidate D: TypeScript compiler via Node subprocess

Pros:

- highest semantic fidelity

Cons:

- breaks the Rust-stable direction
- introduces toolchain coupling and local environment fragility
- too wide for Phase 4

### Final recommendation

Use `tree-sitter` plus `tree-sitter-typescript` as the Phase 4 parser.

Reason:

- the phase is explicitly incremental and local-first
- Phase 4 needs fast structural updates more than full type-checker semantics
- tree-sitter matches the Phase 0 wedge and Phase 2 snapshot/buffer model best

## Symbol Identity Model Candidates And Final Recommendation

### Candidate A: path + byte-range only

Pros:

- simple

Cons:

- unstable across unrelated edits above a declaration
- poor fit for incremental persistence

### Candidate B: symbol name only

Pros:

- easy lookup

Cons:

- collisions are guaranteed in monorepos
- unusable for precise references

### Candidate C: USR-like declaration fingerprint

Shape:

- repo id
- declaration file path
- symbol kind
- container chain
- declared name
- normalized signature digest

Pros:

- stable across many whitespace and nearby edits
- deterministic
- human-debuggable
- good fit for persistent storage and harness normalization

Cons:

- still changes on real rename/move/signature changes

### Final recommendation

Adopt a two-level identity model:

1. `symbol_id`
   - hash of repo id, path, kind, container chain, declared name, and normalized declaration
     signature digest
   - persistent, declaration-oriented, and queryable
2. `occurrence_id`
   - snapshot-scoped identity for one definition/reference/import/export occurrence
   - includes snapshot id, path, and byte range

Planning implication:

- definitions and references should resolve through `symbol_id`
- snapshot-local explain/debug views may expose `occurrence_id`
- Phase 4 does not need cross-rename identity continuity beyond deterministic rebuilds

## Graph Edge Scope For This Phase

Phase 4 graph edges are intentionally limited to what can be derived deterministically from syntax
plus module-resolution rules.

In scope:

- `file_contains_symbol`
- `symbol_contains_symbol`
- `file_imports_module`
- `symbol_imports_symbol` when a binding resolves exactly
- `file_exports_symbol`
- `symbol_exports_symbol` for re-export aliases
- `occurrence_references_symbol`

Optional only if exact and deterministic in the implementation slice:

- `symbol_aliases_symbol`

Out of scope for Phase 4:

- call graphs
- inheritance graphs
- inferred data-flow edges
- type-driven override edges
- probable or fuzzy references
- impact edges

## Persistence Strategy Candidates And Final Recommendation

### Candidate A: extend the existing runtime SQLite database

Pros:

- fewer moving parts

Cons:

- mixes control-plane state with potentially large fact/graph tables
- harder to tune and migrate independently

### Candidate B: JSON or binary files only

Pros:

- simple to inspect

Cons:

- poor query ergonomics
- awkward incremental upserts
- weak concurrency story

### Candidate C: dedicated per-repo SQLite symbol store

Pros:

- consistent with current repo-local runtime design
- supports indexing, joins, and deterministic queries
- separate migrations and vacuum strategy
- easier to keep control plane and symbol plane isolated

Cons:

- requires another persistence crate and migration surface

### Final recommendation

Use a dedicated per-repo SQLite symbol store under the runtime data root, separate from the
existing control-plane SQLite database.

Recommended location shape:

- `.hyperindex/data/symbols/<repo_id>/symbols.sqlite3`

Recommended data layout:

- `indexed_snapshots`
- `snapshot_files`
- `file_facts`
- `symbols`
- `occurrences`
- `graph_edges`
- `module_resolution`
- `query_cache` only if deterministic and easy to invalidate

Recommended operating mode:

- SQLite WAL mode
- transactional file-level upserts
- content-addressed reuse keyed by file content digest where practical
- no symbol facts embedded into snapshot manifests

## Incremental Update Model

Phase 4 incremental updates should be snapshot-driven, not repo-root-driven.

### Canonical input model

1. Phase 2 creates or resolves a `ComposedSnapshot`.
2. Phase 4 reads the snapshot manifest and, when needed, `SnapshotDiffResponse`.
3. TS/JS eligible files are selected from snapshot-resolved paths.
4. Changed files are reparsed; unchanged files reuse persisted facts by digest or prior indexed
   snapshot links.

### File eligibility

Index only:

- `.ts`
- `.tsx`
- `.js`
- `.jsx`
- `.mts`
- `.cts`

Ignore:

- files excluded by repo ignore rules already honored by the runtime
- non-UTF-8 or clearly non-source files
- generated/vendor directories only when already filtered by repo ignore settings or explicit
  future config

### Persistent snapshot indexing

For non-buffer snapshots:

- index the snapshot durably
- store file-fact rows linked to that snapshot
- compute reverse import dependents for targeted edge refresh

### Buffer overlay handling

Final recommendation:

- persist durable symbol state for indexed snapshots
- handle unsaved buffer overlays as snapshot-scoped overlay facts that can be persisted under the
  overlay snapshot id without mutating the last committed snapshot’s facts

Reason:

- the current product wedge requires buffer freshness
- the Phase 2 snapshot model already treats buffer-inclusive snapshots as immutable query inputs

### Update algorithm

1. Compare the target snapshot against the last indexed snapshot for the repo.
2. Use `SnapshotAssembler::diff(...)` to identify changed, added, deleted, and buffer-only paths.
3. Re-resolve changed files through snapshot precedence rules.
4. Reparse only changed files, reusing in-memory tree caches when the daemon still has a prior
   tree for the same file lineage.
5. Replace facts/occurrences/edges for changed files inside one transaction.
6. Re-resolve imports/exports/references for:
   - changed files
   - files importing changed modules when export surfaces changed
7. Mark the snapshot as indexed and queryable.

## Integration Points With The Phase 2 Daemon And Snapshot System

Phase 4 must extend, not replace, the Phase 2 daemon flow.

### State and orchestration

- add symbol services behind
  [DaemonStateManager](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
- keep repo registration and snapshot creation owned by the existing daemon state/store layer
- reuse `JobKind` with new symbol-oriented jobs only if the additions are compatible and explicit

Recommended new internal jobs:

- `SymbolIndex`
- `SymbolRefresh`

### Protocol extension pattern

Follow the existing Phase 2 extension rule:

1. add types in
   [hyperindex-protocol](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol)
2. extend
   [RequestBody](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
   and `SuccessPayload`
3. add daemon handlers in
   [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
4. add `hyperctl` command wiring

Recommended Phase 4 public methods:

- `parse_build`
- `parse_status`
- `parse_inspect_file`
- `symbol_index_build`
- `symbol_index_status`
- `symbol_search`
- `symbol_show`
- `definition_lookup`
- `reference_lookup`
- `symbol_resolve`

### Snapshot integration rule

Parser and symbol services must never read the working tree directly when answering symbol queries.

They must operate on:

- `ComposedSnapshot`
- `SnapshotAssembler::resolve_file(...)`
- `SnapshotAssembler::diff(...)`

### Watcher integration rule

Watcher batches remain producers of normalized file-change hints only.

Phase 4 may use watcher batches to decide when to build a new snapshot or schedule a symbol
refresh, but must not bypass snapshot composition.

## Integration Points With The Future Exact-Search File Discovery / Index Lifecycle

Because no checked-in Phase 3 exact-search crate exists yet, Phase 4 should define only the
compatible abstraction it expects exact search to share later.

### Shared abstraction to introduce during implementation

One snapshot-derived file catalog with:

- repo id
- snapshot id
- repo-relative path
- file content digest
- language classification
- index eligibility flag

Rules:

- exact search and symbol indexing can both consume this catalog
- exact search remains the owner of text indexing and searchable-file policy
- symbol indexing remains the owner of parser/symbol facts

### What Phase 4 must not do

- invent a separate repo crawl pipeline
- make exact search depend on the symbol store
- redefine repo ignore handling independently from the runtime

## Integration Points With The Phase 1 Symbol Benchmark Harness

Phase 4 must make the existing Phase 1 harness runnable against the real symbol engine.

### Required adapter compatibility

The existing shell-facing harness seam in
[bench/hyperbench/adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
must remain logically compatible.

Phase 4 must provide a shell or daemon-backed adapter path that can:

- prepare the corpus
- execute symbol queries
- run incremental refresh scenarios

### Required result normalization

The engine output must still normalize to:

- `path`
- optional `symbol`
- `rank`
- `reason`
- `score`

### Phase 4 benchmark focus

The minimum benchmarkable Phase 4 behavior is:

- the synthetic symbol pack in
  [synthetic-saas-medium-symbol-pack.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/synthetic-saas-medium-symbol-pack.json)
- the matching goldens in
  [synthetic-saas-medium-symbol-goldens.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens/synthetic-saas-medium-symbol-goldens.json)
- incremental refresh for the hero path around `invalidateSession`

Semantic and impact packs remain out of scope for Phase 4 implementation slices unless a prompt
explicitly asks for compatible non-semantic stubs.

## Query Semantics For This Phase

Phase 4 query semantics should be exact and evidence-first.

### Symbol lookup

Input:

- `snapshot_id`
- either `symbol` string or `symbol_id`
- optional `scope`: `repo | package | file`
- optional `path_globs` later only if needed

Behavior:

- exact symbol-name lookup only in this phase
- return deterministic ranked declarations
- no fuzzy search, typo tolerance, or semantic expansion

Ranking:

1. exact declaration-name matches
2. exported declarations before local-only declarations
3. shorter container chain before deeper nested declarations
4. repo-relative path lexical order
5. byte offset ascending

### Definitions

Input:

- `snapshot_id`
- source anchor or `symbol_id`

Behavior:

- return the canonical declaration occurrence plus related export/module evidence
- when a local identifier resolves through an import, return the imported declaration target

### References

Input:

- `snapshot_id`
- `symbol_id`

Behavior:

- return exact resolved identifier occurrences linked to the declaration
- include import/export reference sites when they bind to the symbol
- do not include fuzzy text matches
- do not invent references from dynamic property access, string keys, or runtime reflection

### Explain / inspect

Return:

- declaration metadata
- file path
- byte / line span
- container chain
- import/export relationships
- reference count
- whether the result came from durable facts or overlay facts

## Validation Matrix

### Parser and extraction

- unit tests for supported file extensions
- unit tests for TS/TSX/JS/JSX declaration extraction
- unit tests for import/export extraction
- unit tests for deterministic symbol identities
- unit tests for reference resolution within one file and across direct imports

### Persistence and graph

- migration tests for the symbol store
- roundtrip tests for fact upserts and deletes
- snapshot-to-store dedupe tests by file digest
- graph query tests for lookup, definitions, and references

### Runtime integration

- daemon tests for symbol indexing from `snapshots_create`
- daemon tests for buffer-overlay symbol queries
- CLI tests for `hyperctl symbol ...`
- watcher-triggered refresh scheduling tests once watch ingestion is durable

### Harness integration

- Phase 1 shell or daemon adapter smoke for symbol queries
- synthetic symbol-pack benchmark pass/fail evaluation
- incremental refresh checks for the `invalidateSession` hero path

## Performance Targets And Measurement

These are Phase 4 engineering targets, not release guarantees.

### Targets

- warm symbol lookup p95: `<= 75 ms`
- warm definitions p95: `<= 100 ms`
- warm references p95: `<= 150 ms`
- single-file reparse + fact refresh p95: `<= 200 ms`
- buffer-overlay refresh for one changed file p95: `<= 250 ms`
- initial synthetic medium corpus symbol index build: `<= 10 s`

### Measurement method

- use the Phase 1 harness for symbol query latency and pass-rate tracking
- add Rust integration timing around:
  - parse
  - extract
  - store transaction
  - graph resolution
  - query execution
- record cold and warm timings separately
- compare candidate runs with the existing compare-budget flow without changing Phase 1 artifact
  names

## Risks And Mitigations

### Risk: syntax-only parsing misses TypeScript semantic edges

Mitigation:

- keep Phase 4 graph scope limited to deterministic syntax-plus-module-resolution edges
- defer type-driven behaviors to a later explicitly approved phase

### Risk: buffer-overlay support causes index churn

Mitigation:

- treat buffer-inclusive snapshots as immutable overlay snapshots
- reuse unchanged file facts by digest
- reparse only changed files

### Risk: symbol identity becomes unstable and breaks references

Mitigation:

- use the two-level `symbol_id` / `occurrence_id` model
- test rename, move, whitespace-only, and nested-container scenarios explicitly

### Risk: the control-plane runtime store becomes overloaded

Mitigation:

- keep symbol persistence in a separate per-repo SQLite database
- leave repo registry, buffers, and snapshot-manifest metadata in the existing Phase 2 store

### Risk: Phase 4 silently absorbs exact-search responsibility

Mitigation:

- keep searchable-file discovery as a shared snapshot-derived abstraction only
- keep text indexing out of the symbol store

### Risk: Phase 4 silently absorbs impact-analysis work

Mitigation:

- keep graph edges descriptive, not inferential
- block any impact surface unless a later prompt explicitly authorizes it

## Validation Commands

The intended validation surface for implementation slices is:

```bash
CARGO_TARGET_DIR=/tmp/repohyperindex-target cargo test -p hyperindex-parser
CARGO_TARGET_DIR=/tmp/repohyperindex-target cargo test -p hyperindex-symbols
CARGO_TARGET_DIR=/tmp/repohyperindex-target cargo test -p hyperindex-symbol-store
CARGO_TARGET_DIR=/tmp/repohyperindex-target cargo test -p hyperindex-daemon
UV_CACHE_DIR=/tmp/uv-cache uv run pytest tests/test_query_packs.py tests/test_compare.py
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run --adapter shell --corpus-path <bundle> --output-dir <run-dir> --mode smoke --query-pack-id synthetic-saas-medium-symbol-pack
```

For this docs-only planning slice, only the smallest relevant existing validations should run.

## Definition Of Done

Phase 4 is done only when all of the following are true:

1. TS/JS parser and symbol extraction work on snapshot-resolved files, including buffer overlays.
2. Symbol facts are persisted durably outside the Phase 2 control-plane store.
3. The graph supports deterministic symbol lookup, definitions, and references.
4. Incremental updates reparse only changed files and preserve snapshot precedence.
5. The daemon and CLI expose symbol methods without breaking existing Phase 2 flows.
6. The Phase 1 harness can execute the real symbol engine through the existing adapter seam.
7. The synthetic symbol pack and matching goldens run end to end through `hyperbench`.
8. No semantic retrieval, impact analysis, codemods, UI, or cloud behavior has shipped.

## Assumptions That Do Not Require User Input Right Now

- Rust stable remains the preferred language for the Phase 4 parser/runtime/symbol stack because
  the repo already established a Rust workspace in Phase 2.
- The current Phase 2 snapshot model remains the only supported source of file truth for parser and
  symbol work.
- The existing Phase 1 symbol harness artifacts are sufficient to define the first benchmarkable
  Phase 4 behavior.
- It is acceptable to create new Phase 4 crates instead of overloading the existing repo-store
  crate with large symbol tables.
- There is no checked-in Phase 3 public exact-search contract to preserve beyond the current
  runtime file-discovery lifecycle; any future exact-search integration should use a shared
  snapshot-derived file catalog rather than a coupled index implementation.
