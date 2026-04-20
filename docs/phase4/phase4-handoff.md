# Repo Hyperindex Phase 4 Handoff

## What Phase 4 Built

Phase 4 turned the Phase 2 runtime spine into the first real local symbol engine for Repo
Hyperindex.

The checked-in implementation now includes:

- incremental TS/JS parsing for `.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, and `.cts` files resolved
  from `ComposedSnapshot`
- deterministic fact extraction for declarations, imports, exports, containment, and identifier
  occurrences
- a durable per-repo SQLite symbol store
- deterministic graph construction for containment, imports, exports, and references
- a library query engine for:
  - exact/prefix/substring symbol search
  - show/explain
  - definition lookup
  - reference lookup
  - location/offset resolve
- daemon and CLI integration for symbol build/status/search/show/definitions/references/resolve
- incremental refresh from `SnapshotDiffResponse`, including buffer-only snapshots
- operator/debug commands for symbol rebuild, doctor, and stats
- Phase 1 harness integration through the daemon-backed symbol adapter

The runtime is now coherent enough that Phase 5 can build on the current parser/symbol stack
without redesigning the public seams first.

## Intentionally Still Out Of Scope

Phase 4 still does not implement:

- impact analysis
- semantic retrieval, embeddings, reranking, or vector storage
- compiler-grade TypeScript semantic resolution
- call graphs, inheritance graphs, or data-flow reasoning presented as precise behavior
- codemods or write-side refactors
- a VS Code extension or browser UI
- cloud, team-sharing, or multi-user runtime behavior
- a checked-in Phase 3 exact-search engine

Phase 5 should keep those boundaries explicit unless the scope changes.

## Phase 5 Plug-In Interfaces

### Symbol graph access

The current graph substrate is:

- [symbol_graph.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_graph.rs)
  - `SymbolGraph`
  - `SymbolGraphBuilder::build_with_snapshot(facts, snapshot) -> SymbolGraph`
  - graph indexes that already exist and are safe for Phase 5 to consume:
    - `symbols`
    - `symbol_facts`
    - `occurrences`
    - `occurrences_by_symbol`
    - `occurrences_by_file`
    - `symbol_ids_by_name`
    - `symbol_ids_by_lower_name`
    - `symbol_ids_by_file`
    - `incoming_edges`
    - `outgoing_edges`
    - `edges`
- [lib.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/lib.rs)
  - `SymbolWorkspace::prepare_snapshot(snapshot) -> SymbolScaffoldIndex`

Phase 5 should treat `SymbolGraph` as the current read model for symbol-layer graph access rather
than inventing a second graph abstraction immediately.

### Definition and reference access

The current query seam is:

- [symbol_query.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_query.rs)
  - `SymbolQueryEngine::search_hits(...)`
  - `SymbolQueryEngine::show(...)`
  - `SymbolQueryEngine::definition_occurrences(...)`
  - `SymbolQueryEngine::reference_occurrences(...)`
  - `SymbolQueryEngine::resolve(...)`
- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/symbols.rs)
  - `SymbolSearchParams/Response`
  - `SymbolShowParams/Response`
  - `DefinitionLookupParams/Response`
  - `ReferenceLookupParams/Response`
  - `SymbolResolveParams/Response`

Phase 5 should add new impact-oriented read behavior either:

1. as library consumers of `SymbolQueryEngine` and `SymbolGraph`, or
2. as new daemon methods that sit beside these existing query methods

Do not change the existing definition/reference contracts unless a real compatibility issue is
proven.

### Import, export, reference, and containment edges

The current edge contract is:

- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/symbols.rs)
  - `GraphEdgeKind::{Contains, Defines, References, Imports, Exports}`
  - `GraphEdge`
  - `GraphNodeRef`
- [facts.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/facts.rs)
  - syntax-derived edge extraction
- [symbol_graph.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_graph.rs)
  - repo-local import/export resolution
  - stable incoming/outgoing edge indexes

Phase 5 impact work should consume these exact edge kinds first and add any new derived graph layer
as a separate consumer, not by mutating the Phase 4 meaning of these edge kinds.

### Snapshot and file-content access

The current snapshot/file seam is still the Phase 2 seam:

- [manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
  - `SnapshotAssembler::compose(...)`
  - `SnapshotAssembler::resolve_file(snapshot, path) -> Option<ResolvedFile>`
  - `SnapshotAssembler::diff(left, right) -> SnapshotDiffResponse`
- [snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
  - `ComposedSnapshot`
  - `SnapshotDiffResponse`
  - `SnapshotReadFileResponse`
- [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - `DaemonStateManager::create_snapshot(...)`

Phase 5 should continue to consume file contents through `ComposedSnapshot` and
`SnapshotAssembler`, not by reading the repo root ad hoc.

### Daemon query flow

The current symbol daemon path is:

- [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
  - `HandlerRegistry::{symbol_index_build, symbol_index_status, symbol_search, symbol_show, definition_lookup, reference_lookup, symbol_resolve}`
  - `load_symbol_snapshot(repo_id, snapshot_id)`
- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/symbols.rs)
  - `ParserSymbolService::{parse_build, parse_status, parse_inspect_file, symbol_index_build, symbol_index_status, search, show, definitions, references, resolve}`
  - `ensure_symbol_graph(...)`
  - `previous_indexed_snapshot(...)`
- [incremental.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbol-store/src/incremental.rs)
  - `IncrementalSymbolIndexer::refresh(previous_snapshot, snapshot, diff)`

Phase 5 daemon work should follow the same pattern already used by Phase 4:

1. define protocol types
2. add one `RequestBody` / `SuccessPayload` method
3. add one handler in `HandlerRegistry`
4. call into a focused service method under `hyperindex-daemon`
5. consume snapshot/store/query seams underneath

### Benchmark harness integration

The preserved harness seam is:

- [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  - `prepare_corpus(...)`
  - `execute_symbol_query(...)`
  - `run_incremental_refresh(...)`
- [runner.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/runner.py)
  - unchanged run/report/compare flow

Current daemon adapter expectations that Phase 5 should preserve:

- symbol build metadata remains machine-readable
- `refresh_mode`, `fallback_reason`, and `loaded_from_existing_build` stay available for compare
  flows
- query hits still normalize into the existing Phase 1 `QueryHit` shape

Phase 5 impact benchmarking should plug in through the existing harness adapter seam, not by
moving benchmark logic into the daemon.

## Current Tech Debt And Risks

- The symbol store validates schema shape and reports corruption clearly, but it still does not
  have a full multi-version migration framework; incompatible runtime stores are treated as
  rebuild/reset cases.
- Incremental refresh still rebuilds the final current graph from the reused-plus-updated fact
  batch rather than persisting a separate graph delta layer.
- Query-time canonicalization and resolution remain intentionally conservative:
  - unique exact import/export alias chains only
  - no property/member-access binding
  - no dynamic or computed-name binding
- Module resolution is intentionally narrow:
  - repo-local relative imports
  - workspace package names from `package.json`
  - no `tsconfig` aliases, package `exports`, or external dependency semantics
- There is still no checked-in Phase 3 exact-search engine, so the Phase 3 compatibility boundary
  is preserved by architecture and docs rather than by a live second engine implementation.
- The new hot-path profile is a deterministic smoke fixture, not a CI performance budget.

## Recommended First Milestones For Phase 5

1. Build the first impact-analysis read layer strictly as a consumer of the current
   `SymbolGraph`, `SnapshotAssembler`, and daemon query seams.
2. Start with the Phase 0 hero path:
   "where do we invalidate sessions?"
   Make the first Phase 5 milestone solve that class of query against the current exact symbol and
   import/export/containment evidence before widening scope.
3. Add one impact-oriented daemon method only after the library-level graph/file traversal path is
   real and testable.
4. Preserve the current harness contract and add Phase 5 benchmarking through the existing adapter
   seam instead of redefining `hyperbench`.
5. Keep `hyperctl symbol doctor` and `hyperctl symbol stats` in the validation loop so new Phase 5
   behavior does not drift away from a self-validating symbol substrate.
