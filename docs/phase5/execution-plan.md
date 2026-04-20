# Repo Hyperindex Phase 5 Execution Plan

## Purpose

Phase 5 turns the Phase 4 parser and symbol stack into the first real impact engine for Repo
Hyperindex.

The goal is to build deterministic blast-radius analysis over the existing symbol graph while
preserving:

- the Phase 0 wedge from
  [repo_hyperindex_phase0.md](/Users/rishivinodkumar/RepoHyperindex/repo_hyperindex_phase0.md):
  local-first TypeScript impact engine
- the Phase 1 benchmark harness and public impact-evaluation contracts under
  [bench/](/Users/rishivinodkumar/RepoHyperindex/bench)
- the Phase 2 runtime as the source of truth for repo identity, snapshots, overlays, watcher
  inputs, daemon lifecycle, and CLI transport
- the Phase 4 symbol graph as the source of truth for definitions, references, imports, exports,
  and containment

Phase 5 must stay benchmark-driven and incremental. It must not silently widen into semantic
retrieval, answer generation, codemods, editor UI, cloud sync, or other later-phase work.

## Closeout Note

Phase 5 closes with shipped first-class input support for `symbol` and `file`.

`config_key` remains a documented deferred boundary for Phase 6 because the checked-in fact model
still has no first-class config-key anchors or usage edges. The harness adapter preserves
benchmark compatibility today by degrading config-backed benchmark queries to their backing files.

## Final Phase 5 Scope

Phase 5 includes only the following deliverables:

1. A Rust-stable impact-analysis library layered on top of the existing
   [SymbolGraph](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_graph.rs)
   and Phase 2 snapshot inputs.
2. Deterministic direct and transitive impact analysis for phase-appropriate change scenarios:
   `modify_behavior`, `signature_change`, `rename`, and `delete`.
3. Deterministic target resolution for shipped Phase 5 input targets:
   `symbol` and `file`.
4. Deterministic ranked impact outputs for the phase-appropriate result kinds:
   `symbol`, `file`, `package`, and `test`.
5. A certainty-tier model with explicit reason paths and no hidden semantic guessing.
6. Supporting graph enrichments needed for impact ranking:
   canonical alias collapse, file/package adjacency, and test affinity.
7. Incremental impact updates driven by existing snapshot diffs and buffer overlays.
8. Daemon and CLI integration through compatible new Phase 5 protocol methods.
9. Phase 1 harness integration through the existing adapter seam so impact benchmarking runs
   against the real engine without redesigning `hyperbench`.
10. Durable docs, acceptance criteria, validation targets, and reviewable implementation slices.

## Explicit Non-Goals

Phase 5 must not implement any of the following:

- semantic retrieval, embeddings, reranking, or answer-generation behavior
- a checked-in exact-search engine
- compiler-grade TypeScript semantic resolution
- inferred call graphs, data-flow graphs, inheritance graphs, or dynamic dispatch sold as exact
  impact
- codemods, write-side refactors, or any editor mutation workflow
- a VS Code extension, browser UI, or dashboard
- cloud sync, team sharing, or multi-user runtime behavior
- router-framework-specific route indexing beyond file-backed route evidence
- schema-breaking changes to the Phase 1 harness contracts unless explicitly approved
- replacement of the Phase 2 snapshot, daemon, or CLI transport contracts
- replacement of the Phase 4 symbol graph or the meaning of its existing edge kinds
- any Phase 6 work
- native config-key targets without a checked-in fact-model extension

## Preservation Audit

This is the minimum stable surface Phase 5 must preserve.

### Phase 1 benchmark harness contracts to preserve

The Phase 1 harness remains the source of truth for benchmarkable impact behavior.

Key adapter and normalization seam:

- [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  - `prepare_corpus`
  - `execute_impact_query`
  - `run_incremental_refresh`
  - normalized `QueryHit { path, symbol, rank, reason, score }`
- [runner.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/runner.py)
- [schemas.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/schemas.py)
  - `ImpactQuery`
  - `ImpactTargetType::{SYMBOL, FILE, ROUTE, CONFIG_KEY}`
  - `ChangeHint::{MODIFY_BEHAVIOR, RENAME, SIGNATURE_CHANGE, DELETE}`
- [compare.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/compare.py)
- [benchmark-spec.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/benchmark-spec.md)

Checked-in Phase 1 impact query configs and goldens present in this repo:

- [query-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-pack.yaml)
- [golden-set.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/golden-set.yaml)
- [synthetic-saas-medium-impact-pack.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/synthetic-saas-medium-impact-pack.json)
- [synthetic-saas-medium-impact-goldens.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens/synthetic-saas-medium-impact-goldens.json)
- [next-js-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/next-js-curated-seed-pack.yaml)
- [next-js-curated-seed-goldens.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens/next-js-curated-seed-goldens.yaml)
- [svelte-cli-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/svelte-cli-curated-seed-pack.yaml)
- [svelte-cli-curated-seed-goldens.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens/svelte-cli-curated-seed-goldens.yaml)
- [vite-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/vite-curated-seed-pack.yaml)
- [vite-curated-seed-goldens.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens/vite-curated-seed-goldens.yaml)

Checked-in Phase 1 budget files present in this repo:

- [budgets.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/budgets.yaml)
- [compare-budget.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/compare-budget.yaml)

Planning implications:

- Phase 5 must adapt impact results to the existing `QueryHit` shape instead of changing harness
  artifacts.
- The synthetic impact pack is the primary acceptance target for the first real impact engine
  slice.
- Existing default compare-budget files are fixture-oriented and must not be silently overwritten.
  If Phase 5 needs new budgets, add them compatibly in new files later.

### Phase 2 runtime contracts to preserve

The checked-in Phase 2 runtime is the source of truth for snapshots, overlays, and daemon
lifecycle.

Transport and request flow:

- [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
- [client.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/client.rs)
- [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)

Snapshot and file access:

- [snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
- [manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
- [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)

Watcher and repo state seams:

- [repo.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/repo.rs)
- [watch.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/watch.rs)
- [watcher.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-watcher/src/watcher.rs)

Planning implications:

- Phase 5 impact code must consume files through `ComposedSnapshot` and `SnapshotAssembler`, not
  by probing the repo root directly.
- Incremental impact refresh must be driven by `SnapshotDiffResponse`, including
  `buffer_only_changed_paths`.
- Repo identity remains `RepoRecord` plus snapshot id, not ad hoc workspace probing.

### Phase 3 exact-search preservation boundary

There is no checked-in
[phase3-handoff.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase3/phase3-handoff.md),
no checked-in Phase 3 status doc, and no checked-in exact-search crate in this repository as of
2026-04-09.

The practical preservation boundary is therefore:

- do not redefine Phase 2 snapshot and daemon contracts to serve impact
- do not make the symbol graph the owner of text indexing
- do not make impact analysis require an exact-search engine to produce core results
- share only snapshot-derived file eligibility and path normalization seams when exact search
  eventually lands

Relevant current compatible abstraction:

- [snapshot_catalog.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-parser/src/snapshot_catalog.rs)
  - `SnapshotFileCatalog::build(...)`
  - snapshot-derived eligible/skipped file catalog
- [mod.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/mod.rs)
  - there is no checked-in exact-search command surface to preserve yet

Planning implications:

- Phase 5 can optionally consume a shared snapshot-derived file catalog.
- Phase 5 must not invent an exact-search API or route impact through a fake exact engine.

### Phase 4 symbol graph and symbol-query contracts to preserve

The Phase 4 symbol stack is the source of truth for syntax-derived graph evidence.

Core graph substrate:

- [symbol_graph.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_graph.rs)
  - `SymbolGraph`
  - `SymbolGraphBuilder::build_with_snapshot(...)`
  - indexes already available for impact:
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
- [symbol_query.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_query.rs)
  - `show`
  - `definition_occurrences`
  - `reference_occurrences`
  - `resolve`
  - canonical alias behavior already used by Phase 4 queries

Public symbol transport and daemon seams:

- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/symbols.rs)
- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/symbols.rs)
- [symbol.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/symbol.rs)

Planning implications:

- Phase 5 impact work should consume `GraphEdgeKind::{Contains, Defines, References, Imports, Exports}`
  exactly as they exist today.
- Any new impact-specific derived edges must live in a separate projection layer, not by changing
  the meaning of Phase 4 graph edges.
- Existing symbol build metadata such as `refresh_mode`, `fallback_reason`, and
  `loaded_from_existing_build` should remain available to the harness and compare flows.

## Proposed Runtime / Impact File Tree

This is the target Phase 5 shape. Not every file must land in the first implementation slice.

```text
docs/
  phase5/
    execution-plan.md
    acceptance.md
    status.md
    decisions.md

crates/
  hyperindex-impact/
    src/
      lib.rs
      model.rs
      target.rs
      enrich.rs
      graph.rs
      certainty.rs
      ranking.rs
      query.rs
      incremental.rs
  hyperindex-impact-store/
    src/
      lib.rs
      migrations.rs
      impact_store.rs
  hyperindex-protocol/
    src/
      impact.rs
      api.rs
  hyperindex-daemon/
    src/
      impact.rs
      handlers.rs
  hyperindex-cli/
    src/
      commands/
        impact.rs
        mod.rs
```

Recommended responsibility split:

- `hyperindex-impact`
  owns target resolution, derived impact projection, certainty computation, traversal, and
  ranking
- `hyperindex-impact-store`
  owns durable impact build metadata and rebuildable enrichment persistence
- existing protocol, daemon, and CLI crates remain the public surface

## Impact Model Candidates And Final Recommendation

### Candidate A: walk the existing `SymbolGraph` directly at query time

Pros:

- smallest implementation surface
- reuses Phase 4 graph exactly
- easy to validate in early unit tests

Cons:

- poor fit for `file`, `package`, `test`, and `config_key` ranking
- repeated query-time recomputation of the same file/package/test expansions
- awkward harness integration for non-symbol targets

### Candidate B: derive a multi-layer impact projection from the symbol graph

Shape:

- nodes:
  - `symbol`
  - `file`
  - `package`
  - `config_key`
  - `test`
- base edges:
  - containment
  - references
  - imports
  - exports
- derived edges:
  - symbol-to-file ownership
  - file-to-package membership
  - file-to-file adjacency
  - config-key declaration and usage
  - test affinity

Pros:

- supports all Phase 5 result kinds without changing Phase 4 edge semantics
- deterministic and snapshot-scoped
- compatible with incremental refresh by changed path
- keeps transitive traversal query-time and reviewable

Cons:

- requires a new derived layer and persistence surface

### Candidate C: precompute all transitive closures for every target

Pros:

- very fast warm queries

Cons:

- storage blow-up
- difficult invalidation for incremental edits
- high risk of Phase 5 complexity creep

### Final recommendation

Use Candidate B: a derived multi-layer impact projection built from the current symbol graph and
snapshot-derived file metadata.

Planning implications:

- Phase 4 graph semantics stay authoritative and unchanged.
- Phase 5 persists only direct, rebuildable enrichment data.
- Transitive traversal stays query-time so incremental invalidation remains bounded.

## Supported Target Kinds For This Phase

### First-class input targets

Phase 5 should support these input target kinds end to end:

- `symbol`
- `file`
- `config_key`

### Ranked output target kinds

Phase 5 should return ranked impact hits across these result kinds:

- `symbol`
- `file`
- `package`
- `test`

### Deferred target kinds

These stay explicitly out of the first Phase 5 implementation slice:

- first-class `route` targets backed by a dedicated route registry
- first-class `package` query targets
- arbitrary AST node or text-span targets

Rationale:

- the checked-in synthetic impact benchmark can be satisfied by `symbol`, `file`, and
  `config_key`
- `package` and `test` are needed as ranked impact outputs, not as initial query targets
- route behavior can remain file-backed in this phase without widening into framework-specific
  routing semantics

## Supported Change Scenarios For This Phase

Phase 5 should support these deterministic change scenarios:

### `modify_behavior`

Use for:

- internal logic changes that preserve name and callable shape

Primary expansion:

- target definition
- direct references/importers/exporters
- containing file
- package
- tests covering the directly impacted file or symbol

### `signature_change`

Use for:

- parameter-list or callable-shape changes

Primary expansion:

- all `modify_behavior` edges
- stronger caller and import-binding propagation
- package and test ranking boosted for direct callers

### `rename`

Use for:

- symbol or file identity changes that preserve intended behavior

Primary expansion:

- definition
- all references and import/export alias sites
- containing file/package
- tests referencing the renamed symbol or file

### `delete`

Use for:

- target removal

Primary expansion:

- definition
- all references/importers/exporters/callers
- containing file/package
- dependent tests

## Certainty-Tier Model And Rationale

Phase 5 should use three deterministic certainty tiers:

### `certain`

Use only when the engine has explicit syntax-derived evidence.

Examples:

- the target definition itself
- direct reference occurrences
- exact import/export alias chains already resolved by Phase 4
- file membership for a target symbol
- tests that explicitly import or reference the impacted file or symbol
- config-key declaration anchors with exact usage edges

### `likely`

Use for deterministic but expanded consequences that are one step removed from the exact edge.

Examples:

- package-level expansion from certainly impacted files
- tests inferred from file-level production-to-test adjacency
- file-level impact induced by certainly impacted symbols

### `possible`

Use for deterministic fallback expansion where the engine has evidence of adjacency but not direct
consumption.

Examples:

- broad package-affinity spillover after a file deletion
- conventional test matches when explicit import/reference evidence is absent

Rationale:

- the tier names match the Phase 0 product principle of trustworthy impact
- the model keeps benchmark behavior explicit instead of hiding uncertainty in one score
- every non-self hit must carry both a certainty tier and at least one reason path

## Graph Enrichment Strategy And Rationale

Phase 5 should add only deterministic, snapshot-scoped enrichments:

1. Canonical alias collapse
   - seed symbol targets through the current Phase 4 canonical-symbol behavior so import/export
     aliases resolve consistently
2. File adjacency
   - derive file-to-file edges from symbol-level imports/exports/references plus file ownership
3. Package membership
   - map files to packages using snapshot-local `package.json` discovery and repo-relative paths
4. Config-key anchors
   - add deterministic config-key identities and usage edges for exported config objects and
     properties found in supported TS/JS syntax
5. Test affinity
   - prefer explicit import/reference links from test files
   - fall back to deterministic naming and package heuristics only when explicit evidence is
     missing
6. Changed-path seed mapping
   - map snapshot diff paths to affected symbols, files, packages, config keys, and tests

Rationale:

- these enrichments are enough to satisfy the Phase 1 impact packs
- they remain compatible with the Phase 4 graph instead of mutating it
- they avoid widening into call-graph or semantic-analysis work

## Materialization / Persistence Strategy And Rationale

### Candidate A: no persistence, compute the whole impact projection on demand

Pros:

- smallest implementation surface

Cons:

- repeated rebuild work
- harder to benchmark warm query behavior
- weaker incremental update story

### Candidate B: extend the symbol store with impact tables

Pros:

- fewer files

Cons:

- couples symbol and impact migrations tightly
- mixes two read/write profiles in one store

### Candidate C: dedicated per-repo impact store with rebuildable direct enrichments

Pros:

- keeps symbol and impact lifecycles separate
- supports warm-query benchmarking and incremental refresh
- avoids persisting all-pairs closures

Cons:

- requires a second persistence crate

### Final recommendation

Use Candidate C.

Recommended location shape:

- `.hyperindex/data/impact/<repo_id>/impact.sqlite3`

Persist only:

- impact build metadata keyed by `repo_id`, `snapshot_id`, and symbol build id
- direct derived enrichment rows
- stats needed for status/debug commands

Do not persist:

- transitive closure tables
- per-query results
- heuristic caches that cannot be deterministically rebuilt

Rationale:

- the impact layer is rebuildable from the symbol build and snapshot
- warm-query performance matters for Phase 5
- not storing all transitive closures keeps incremental invalidation tractable

## Incremental Update Model

Phase 5 incremental behavior should follow this order:

1. Create or load the target snapshot through the existing Phase 2 daemon/runtime flow.
2. Reuse or refresh the symbol graph through the existing Phase 4 incremental symbol path.
3. Read `SnapshotDiffResponse` and seed changed files from:
   - `changed_paths`
   - `added_paths`
   - `deleted_paths`
   - `buffer_only_changed_paths`
4. Recompute only direct enrichment rows owned by changed files and their affected symbols,
   config keys, tests, and packages.
5. Reuse all unchanged enrichment rows.
6. Run transitive traversal on demand against the refreshed direct projection.

Required full-rebuild fallbacks:

- no prior impact build exists
- symbol build fell back to `full_rebuild`
- schema version changes
- package-boundary discovery changes in a way that invalidates package membership broadly
- config-key extraction invariants become inconsistent

Planning rule:

- persist direct enriched edges and stats
- compute transitive closure per query

## Integration Points With The Phase 2 Daemon And Snapshot System

Phase 5 should plug into the current Phase 2 runtime, not replace it.

Required seams:

- [snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
  - `ComposedSnapshot`
  - `SnapshotDiffResponse`
- [manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
  - `SnapshotAssembler::resolve_file(...)`
  - `SnapshotAssembler::diff(...)`
- [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - `DaemonStateManager::create_snapshot(...)`

Recommended daemon/public surface:

- add `impact_analyze`
- optionally add `impact_status`
- keep any build materialization internal at first unless a public build/status seam proves
  necessary for operator workflows

Planning implications:

- repo/snapshot lifecycle remains Phase 2-owned
- impact requests stay snapshot-scoped
- buffer-overlay freshness remains first-class

## Integration Points With The Phase 3 Exact-Search System

There is no checked-in Phase 3 exact-search engine to integrate with today.

Phase 5 should preserve a compatible seam only:

- allow future exact search to share `SnapshotFileCatalog` or an equivalent snapshot-derived file
  eligibility abstraction
- allow future exact search to augment config-key or file-based evidence if it becomes available
- do not make exact search a dependency for core impact answers

Planning implications:

- core Phase 5 impact answers must come from symbol-graph and snapshot evidence alone
- future exact-search integration should be additive evidence, not a new source of truth

## Integration Points With The Phase 4 Symbol Graph And Symbol-Query System

Phase 5 should consume the current symbol stack as-is:

- [symbol_graph.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_graph.rs)
- [symbol_query.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_query.rs)
- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/symbols.rs)

Required consumption rules:

- resolve symbol targets through existing symbol identities
- follow exact resolved alias chains only when Phase 4 can already prove them
- treat `GraphEdgeKind` semantics as fixed
- keep the impact layer as a consumer of symbol outputs, not a replacement for them

## Integration Points With The Phase 1 Impact Benchmark Harness

Phase 5 benchmarking must plug into the existing harness seam, not redesign it.

Required future integration points:

- extend the existing daemon-backed adapter path in
  [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  so `execute_impact_query(...)` uses the real daemon impact method
- preserve `PreparedCorpus`, `QueryExecutionResult`, `RefreshExecutionResult`, and `QueryHit`
  shapes
- continue surfacing build metadata needed by compare/report flows

Adapter mapping rule:

- `ImpactHit.reason` must map into the existing harness `reason` field using the primary stable
  relationship token:
  - `definition`
  - `reference`
  - `caller`
  - `callee`
  - `config`
  - `test`
- certainty tiers and reason paths should travel in adapter notes or metadata without breaking
  `QueryHit`

## Query Semantics And Response Shape

Recommended public request method:

- `impact_analyze`

Recommended request shape:

```json
{
  "repo_id": "repo-123",
  "snapshot_id": "snap-123",
  "target": {
    "target_kind": "symbol",
    "value": "packages/auth/src/session/service.ts#invalidateSession"
  },
  "change_hint": "modify_behavior",
  "limit": 20,
  "include_transitive": true,
  "include_reason_paths": true
}
```

Recommended response shape:

```json
{
  "repo_id": "repo-123",
  "snapshot_id": "snap-123",
  "target": {
    "target_kind": "symbol",
    "value": "packages/auth/src/session/service.ts#invalidateSession"
  },
  "summary": {
    "direct_count": 3,
    "transitive_count": 7,
    "certainty_counts": {
      "certain": 4,
      "likely": 4,
      "possible": 2
    }
  },
  "hits": [
    {
      "rank": 1,
      "score": 1.0,
      "result_kind": "symbol",
      "path": "packages/auth/src/session/service.ts",
      "display_name": "invalidateSession",
      "symbol_id": "sym.function.invalidatesession.abc123",
      "certainty": "certain",
      "relationship": "definition",
      "depth": 0,
      "direct": true,
      "reason_paths": []
    }
  ],
  "diagnostics": []
}
```

Recommended result-shape rules:

- sort deterministically by certainty tier, directness, relationship priority, depth, score, and
  path/symbol id tie-breakers
- every non-self hit must include at least one reason path
- every returned hit must identify whether it is direct or transitive
- package and test hits may omit `symbol_id`

## Validation Matrix

Phase 5 validation should stay incremental and benchmark-driven.

### Library validation

- `cargo test -p hyperindex-impact`
  - target resolution
  - certainty tier assignment
  - direct traversal
  - transitive traversal
  - ranking stability

### Persistence and incremental validation

- `cargo test -p hyperindex-impact-store`
  - schema bootstrap
  - build manifest lifecycle
  - incremental refresh by changed path
  - rebuild fallback behavior

### Daemon and CLI validation

- `cargo test -p hyperindex-daemon`
  - protocol wiring
  - snapshot-scoped impact requests
- `cargo test -p hyperindex-cli`
  - impact command rendering
  - JSON and text output

### Harness validation

- `UV_CACHE_DIR=/tmp/uv-cache uv run pytest`
  - preserve the Phase 1 harness
  - preserve compare/report behavior
  - add impact adapter coverage
- `hyperbench run --adapter daemon --mode smoke ...`
  - execute the checked-in synthetic impact pack against the real engine

### Incremental refresh validation

- one-file working-tree edit scenario
- one-file buffer-only scenario
- delete scenario
- rename/signature-change scenario

## Performance Targets And How They Will Be Measured

Phase 5 should preserve the Phase 0 product target direction while staying realistic about the
current codebase and synthetic benchmark size.

Primary targets for `synthetic-saas-medium`:

- warm impact query p95: `<= 300 ms`
- cold impact query p95 after first build: `<= 750 ms`
- single-file working-tree incremental impact refresh p95: `<= 250 ms`
- buffer-only incremental impact refresh p95: `<= 150 ms`
- query pass rate against the checked-in synthetic impact goldens: `1.0`

Measurement approach:

- use the existing `hyperbench` run/report/compare flow
- keep using `query-latency-p95`, `refresh-latency-p95`, and `query-pass-rate`
- add additive custom metrics only when needed, for example:
  - `impact-build-latency`
  - `impact-direct-hit-rate`
  - `impact-transitive-hit-rate`
  - `impact-certain-topk-rate`

Budget rule:

- do not mutate the current default Phase 1 compare budgets in place as part of the first Phase 5
  slice
- if Phase 5 needs engine-specific budgets, add new budget files compatibly after real
  measurements exist

## Recommended Implementation Order

Phase 5 should land in these slices:

1. Library-only impact target resolution and direct/transitive traversal over the current
   `SymbolGraph`
2. Deterministic certainty tiers, reason paths, and ranking
3. Config-key and test/package enrichments
4. Impact persistence and incremental refresh keyed to symbol builds and snapshot diffs
5. Daemon and CLI transport
6. Phase 1 harness integration and benchmark validation

## Risks And Mitigations

### Risk: Phase 5 overpromises semantic precision

Mitigations:

- keep certainty tiers explicit
- keep reason paths visible
- restrict `certain` to syntax-derived evidence already provable by current phases

### Risk: absence of a checked-in exact-search engine creates pressure to widen scope

Mitigations:

- keep exact search optional for impact
- reuse only shared snapshot-derived file-catalog logic
- do not add an exact-search protocol surface in Phase 5

### Risk: future config-key impact becomes fuzzy or framework-specific

Mitigations:

- keep native config-key support deferred until the repo has first-class config anchors
- avoid broad string-search-based config inference

### Risk: package and test ranking becomes noisy

Mitigations:

- separate `certain`, `likely`, and `possible`
- prefer explicit import/reference evidence over naming heuristics
- keep tie-breakers deterministic

### Risk: persistence or incremental invalidation grows too large

Mitigations:

- persist only direct enrichment rows and build metadata
- do not store all-pairs closure
- allow explicit full-rebuild fallbacks when invariants are violated

## Definition Of Done

Phase 5 is done only when all of the following are true:

1. A real impact engine exists over the checked-in symbol graph and snapshot model.
2. The engine supports shipped `symbol` and `file` input targets, with `config_key` carried as an
   explicit deferred boundary into Phase 6.
3. The engine returns deterministic direct and transitive impact results with certainty tiers and
   reason paths.
4. The engine can rank `symbol`, `file`, `package`, and `test` impact outputs.
5. Incremental impact refresh works for snapshot diffs and buffer overlays.
6. A compatible daemon and CLI impact surface exists.
7. The Phase 1 harness can run the checked-in synthetic impact pack against the real engine
   through the existing adapter seam.
8. Existing Phase 1 harness artifacts and Phase 2/4 public contracts remain preserved.
9. Validation commands and benchmark evidence are recorded in
   [status.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase5/status.md).
10. Hard non-goals remain unshipped.

## Assumptions That Do Not Require User Input Right Now

- There is no checked-in Phase 3 exact-search handoff or exact-search crate to integrate with as
  of 2026-04-09.
- TypeScript/JavaScript remains the only Phase 5 language scope.
- Route impact can remain file-backed in this phase; a first-class route registry is not required
  for the checked-in synthetic impact pack.
- The current Phase 4 symbol graph and alias-resolution behavior remain authoritative inputs for
  impact.
- Adding Phase 5 protocol methods is acceptable as long as it is done compatibly under the
  existing `repo-hyperindex.local/v1` surface.
- If new compare budgets are needed, they can be added compatibly in new files instead of
  overwriting the current default Phase 1 budget files.
