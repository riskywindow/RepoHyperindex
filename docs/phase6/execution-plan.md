# Repo Hyperindex Phase 6 Execution Plan

## Purpose

Phase 6 adds the first real semantic retrieval engine for Repo Hyperindex.

The goal is to ship benchmarkable, local-first TypeScript semantic search that plugs into the
existing runtime, symbol, impact, and harness seams without reopening ownership boundaries that
earlier phases already established.

Phase 6 must preserve:

- the Phase 0 wedge from
  [repo_hyperindex_phase0.md](/Users/rishivinodkumar/RepoHyperindex/repo_hyperindex_phase0.md):
  local-first TypeScript code intelligence with the hero path
  `where do we invalidate sessions?`
- the Phase 1 harness as the source of truth for benchmarkable semantic behavior under
  [bench/](/Users/rishivinodkumar/RepoHyperindex/bench)
- the Phase 2 runtime as the source of truth for snapshots, overlays, repo identity, and daemon
  lifecycle
- the practical Phase 3 boundary that exact file search owns exact-text retrieval when that engine
  exists
- the Phase 4 parser and symbol graph as the source of truth for definitions, references,
  imports/exports, and containment
- the Phase 5 impact engine as the source of truth for blast-radius analysis

Phase 6 must stay retrieval-only. It must not widen into answer generation, semantic edits,
general chat, or backend-specific platform work.

## Final Phase 6 Scope

Phase 6 includes only these deliverables:

1. A real semantic retrieval library over snapshot-resolved TypeScript/JavaScript corpora.
2. Deterministic chunking for symbol-backed and file-backed semantic retrieval units.
3. One local-first embedding provider implementation that works on a primary laptop without a
   required network service.
4. One persisted semantic store per repo with snapshot-scoped semantic build metadata.
5. Incremental semantic refresh from `SnapshotDiffResponse`, including buffer-only snapshots.
6. A minimal daemon contract for semantic readiness and semantic retrieval.
7. CLI integration for semantic query execution and operator-facing status.
8. A daemon-backed Phase 1 semantic adapter path through the existing `hyperbench` contract.
9. Validation and docs that let another engineer implement and benchmark Phase 6 without
   re-deriving the architecture.

## Explicit Non-Goals

Phase 6 must not implement any of the following:

- answer generation, chat responses, or summary synthesis
- LLM orchestration over retrieved code
- semantic refactors, codemods, or editor write actions
- a UI, VS Code extension, browser app, or cloud service
- cross-repo or multi-user search
- a new exact-search engine
- replacement of the Phase 2 snapshot model
- replacement of the Phase 4 symbol graph or edge meanings
- replacement of the Phase 5 impact engine
- a mandatory remote embedding provider
- semantic ranking that silently mutates Phase 1 harness contracts

## Preservation Audit

This is the minimum stable surface Phase 6 must preserve.

### Phase 1 benchmark harness contracts to preserve

The Phase 1 harness remains the source of truth for semantic benchmark behavior.

Primary semantic seams:

- [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  - `EngineAdapter.prepare_corpus(...)`
  - `EngineAdapter.execute_semantic_query(...)`
  - `EngineAdapter.run_incremental_refresh(...)`
  - normalized `QueryHit { path, symbol, rank, reason, score }`
- [runner.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/runner.py)
  - `run_benchmark(...)`
- [schemas.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/schemas.py)
  - `SemanticQuery`
  - `SemanticRerankMode::{OFF, HYBRID}`
  - `GoldenExpectation`
  - `ExpectedHit`
- [benchmark-spec.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/benchmark-spec.md)

Checked-in semantic benchmark assets already present:

- [synthetic-saas-medium-semantic-pack.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/synthetic-saas-medium-semantic-pack.json)
- [synthetic-saas-medium-semantic-goldens.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens/synthetic-saas-medium-semantic-goldens.json)
- [budgets.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/budgets.yaml)

Planning implications:

- Phase 6 must add a real daemon-backed semantic adapter instead of redesigning `hyperbench`.
- Semantic results must still normalize into `QueryHit`.
- `path_globs` and `rerank_mode` from `SemanticQuery` are part of the benchmark contract and must
  map to real engine behavior.

### Phase 2 runtime and snapshot interfaces to preserve

The checked-in Phase 2 runtime remains the source of truth for repo identity, snapshots, daemon
transport, and overlay precedence.

Primary seams:

- [snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
  - `ComposedSnapshot`
  - `SnapshotDiffResponse`
  - `SnapshotReadFileResponse`
- [manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
  - `SnapshotAssembler::resolve_file(...)`
  - `SnapshotAssembler::diff(...)`
- [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - `DaemonStateManager::create_snapshot(...)`
  - `DaemonStateManager::runtime_status(...)`
- [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
  - `RequestBody`
  - `SuccessPayload`
- [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
  - `HandlerRegistry`

Planning implications:

- Phase 6 semantic indexing must consume files only through `ComposedSnapshot` and
  `SnapshotAssembler`.
- Buffer overlays are first-class semantic inputs, not a later optimization.
- Semantic build identity is snapshot-scoped, not repo-root-scoped.

### Phase 3 exact-search ownership boundary to preserve

There is still no checked-in Phase 3 exact-search crate or daemon method in this repository.

The preserved boundary is therefore architectural:

- semantic retrieval must not become the owner of exact file search
- exact text search, when it lands, remains the source of truth for exact lexical retrieval
- semantic ranking may consume exact-search candidates later, but it must not redefine exact-search
  behavior

Current practical seams:

- [schemas.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/schemas.py)
  - `ExactQuery`
- [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  - `EngineAdapter.execute_exact_query(...)`
- [snapshot_catalog.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-parser/src/snapshot_catalog.rs)
  - snapshot-derived file eligibility

Planning implications:

- Phase 6 may define an optional lexical-candidate seam, but it must remain additive.
- If no exact-search engine is present, Phase 6 must still be fully functional with its own local
  lexical features and embeddings.

### Phase 4 parser and symbol interfaces to preserve

The Phase 4 symbol system remains the source of syntax-derived structure.

Primary seams:

- [symbol_graph.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_graph.rs)
  - `SymbolGraph`
  - `SymbolGraphBuilder::build_with_snapshot(...)`
- [symbol_query.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_query.rs)
  - `search_hits(...)`
  - `show(...)`
  - `definition_occurrences(...)`
  - `reference_occurrences(...)`
  - `resolve(...)`
- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/symbols.rs)
  - `SymbolId`
  - `SymbolRecord`
  - `GraphEdgeKind::{Contains, Defines, References, Imports, Exports}`
  - `SymbolIndexBuildId`
- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/symbols.rs)
  - `ParserSymbolService`

Planning implications:

- Symbol-backed semantic chunks should anchor to `SymbolId` and `SymbolRecord`.
- Semantic retrieval must not reinterpret or mutate Phase 4 edge kinds.
- Semantic chunk metadata may read symbol graph facts, but the symbol graph remains authoritative.

### Phase 5 impact interfaces to preserve

The Phase 5 impact engine remains the source of truth for blast-radius behavior.

Primary seams:

- [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/impact.rs)
  - `ImpactAnalyzeResponse`
  - `ImpactHit`
  - `ImpactReasonPath`
  - `ImpactManifest`
- [impact_enrichment.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-impact/src/impact_enrichment.rs)
  - `ImpactEnrichmentPlan`
  - package/test/file enrichment indexes
- [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/impact.rs)
  - `ImpactService`

Planning implications:

- Phase 6 must not call `impact_analyze` in the hot query path.
- Phase 6 may reuse static Phase 5 enrichment metadata, especially package and test associations,
  as additive chunk metadata or rerank features.
- Blast-radius answers remain an impact concern, not a semantic-search concern.

## Proposed Runtime / Semantic File Tree

This is the Phase 6 target shape. The exact file count can stay smaller in the first slice as long
as ownership remains clear.

```text
docs/
  phase6/
    execution-plan.md
    acceptance.md
    status.md

crates/
  hyperindex-semantic/
    src/
      lib.rs
      semantic_chunking.rs
      semantic_model.rs
      embedding_provider.rs
      lexical_features.rs
      semantic_search.rs
      semantic_rerank.rs
      incremental.rs
  hyperindex-semantic-store/
    src/
      lib.rs
      migrations.rs
      semantic_store.rs
  hyperindex-protocol/
    src/
      semantic.rs
      api.rs
      config.rs
  hyperindex-daemon/
    src/
      semantic.rs
      handlers.rs
  hyperindex-cli/
    src/
      commands/
        semantic.rs

tests/
  test_daemon_semantic_adapter.py
```

Recommended crate ownership:

- `hyperindex-semantic`
  - chunking, embedding provider abstraction, candidate generation, hybrid reranking, and
    incremental semantic build logic
- `hyperindex-semantic-store`
  - persisted snapshot-scoped semantic builds and embedding cache
- `hyperindex-protocol`
  - semantic request/response and config types
- `hyperindex-daemon`
  - semantic service and transport wiring
- `hyperindex-cli`
  - operator-facing semantic commands

## Integration Points

### Phase 2 daemon and snapshot integration

Phase 6 must plug into the current daemon/runtime flow exactly the way Phase 4 and Phase 5 did:

1. add protocol types in
   [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
   and a new
   [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/semantic.rs)
2. add handler methods in
   [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
3. implement one focused daemon service under `crates/hyperindex-daemon/src/semantic.rs`
4. assemble semantic inputs from `ComposedSnapshot`, not from direct repo reads
5. drive incremental refresh from `SnapshotDiffResponse`

Recommended public methods:

- `semantic_status`
- `semantic_search`

Recommended non-public operator flows:

- local CLI `semantic rebuild`
- local CLI `semantic stats`

These should stay CLI-local unless a real daemon need appears, mirroring the Phase 5 operator
pattern.

### Phase 3 exact-search integration where relevant

Phase 6 should define one optional lexical-candidate seam and keep it internal to semantic
ranking. The semantic engine may use:

- query token overlap
- filename/path token overlap
- later exact-search candidates if a real Phase 3 engine lands

Recommended rule:

- if no exact-search engine exists, semantic retrieval still runs correctly
- if an exact-search engine exists later, its results are an additive input to semantic rerank, not
  a semantic ownership transfer

### Phase 4 parser and symbol graph integration where relevant

Phase 6 should use Phase 4 data in two places:

1. Chunk construction
   - top-level symbols become primary chunk anchors
   - symbol display names, kinds, paths, and byte spans become stable metadata
2. Rerank/context features
   - import/export/containment/reference adjacency can be used as additive structural signals

Recommended stable lookup inputs:

- `SymbolGraph.symbols`
- `SymbolGraph.symbol_ids_by_file`
- `SymbolGraph.incoming_edges`
- `SymbolGraph.outgoing_edges`
- `SymbolQueryEngine.resolve(...)`

### Phase 5 impact integration where relevant

Phase 6 should use Phase 5 only as an additive structural context source.

Allowed use:

- package membership from `ImpactEnrichmentPlan.package_by_file`
- test affinity from `ImpactEnrichmentPlan.tests_by_file`
- related-file metadata from `ImpactEnrichmentPlan.reverse_dependents_by_file`

Disallowed use in the initial query hot path:

- running `impact_analyze` per semantic query
- treating impact certainty as semantic truth

### Phase 1 semantic benchmark harness integration

The harness integration path should mirror the separate `daemon-impact` adapter pattern from
Phase 5.

Recommended adapter:

- add `daemon-semantic` in
  [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)

Responsibilities:

- `prepare_corpus(...)`
  - create or warm semantic state for the clean snapshot
- `execute_semantic_query(...)`
  - call `semantic_search`
  - normalize hits into `QueryHit`
- `run_incremental_refresh(...)`
  - edit one file
  - create a dirty snapshot
  - measure semantic refresh/query behavior

`runner.py`, `report.py`, and `compare.py` should remain unchanged except for additive metrics.

## Semantic Chunk Model Candidates

### Candidate A: File-level chunks

Pros:

- simplest build model
- smallest metadata surface
- easy incremental refresh

Cons:

- poor precision on large files
- weak symbol attribution
- noisy semantic matches for route/config/test files with multiple concerns

### Candidate B: Fixed sliding windows only

Pros:

- good coverage even when symbol extraction is sparse
- predictable token/byte budgets

Cons:

- unstable semantics when edits shift boundaries
- weak alignment to Phase 4 symbols
- more duplicate content and harder explanations

### Candidate C: Symbol-first structural chunks with file fallback windows

Shape:

- primary chunks for exported functions, classes, methods, constants, and route/config/test
  anchors
- file fallback windows for files or regions with weak symbol coverage
- stable metadata for path, package, symbol id, symbol kind, byte span, and file role

Pros:

- aligned with the Phase 4 source of truth
- better query explanations and harness normalization
- smaller candidate set than raw windows
- preserves file-backed coverage for config and route assets

Cons:

- chunker depends on symbol graph availability
- needs fallback behavior for weakly parsed files

### Final Recommendation

Ship Candidate C.

Phase 6 should use symbol-first structural chunks plus file fallback windows. That is the best fit
for the current repository because:

- Phase 4 already gives stable symbol spans and identities
- Phase 1 goldens care about file and symbol evidence, not just raw text windows
- the hero queries span routes, handlers, config, and tests, which need both symbol and file
  anchors

Recommended chunk kinds:

- `symbol_body`
- `file_header`
- `route_file`
- `config_file`
- `test_file`
- `fallback_window`

## Embedding Provider Candidates

### Candidate A: Rust-native local ONNX provider

Shape:

- embed inside the Rust workspace
- no required network dependency
- deterministic on one machine class

Pros:

- matches the local-first wedge
- easiest to benchmark and cache
- simplest daemon deployment story

Cons:

- model quality ceiling may be lower than a strong hosted provider

### Candidate B: Hosted API provider

Shape:

- remote embedding API behind a provider trait

Pros:

- higher likely retrieval quality
- less local model packaging work

Cons:

- violates local-first-by-default
- adds network cost, secrets, and nondeterministic latency

### Candidate C: Local sidecar for heavier code-focused models

Shape:

- Python or external process hosting a larger embedding model

Pros:

- better code-text alignment than small generic local models

Cons:

- operationally heavier than a Rust-native path
- more moving pieces before the benchmark seam is stable

### Final Recommendation

Ship Candidate A first.

Recommended initial provider:

- Rust-native local ONNX embedding through a provider trait
- default model recommendation: `BAAI/bge-small-en-v1.5`

Rationale:

- it keeps Phase 6 local-first and benchmarkable on the same machine
- it is small enough to make cold-start and incremental validation practical
- it gives the repo one deterministic default before any hosted-provider discussion

Important follow-on rule:

- keep the provider abstraction additive so a hosted provider or heavier local code model can be
  added later without changing protocol or store semantics

## Vector / ANN Index Candidates

### Candidate A: Flat cosine scan over persisted vectors

Pros:

- deterministic
- easiest to debug
- simplest correctness story
- enough for the current synthetic/curated Phase 1 corpora

Cons:

- query latency scales linearly with chunk count

### Candidate B: `usearch` or HNSW sidecar index

Pros:

- faster warm query latency on larger corpora
- local and embeddable

Cons:

- extra persistence format and rebuild complexity
- harder debugging than flat scan

### Candidate C: SQLite vector extension

Pros:

- keeps storage in the same SQLite family as other runtime stores
- metadata and vectors stay close together

Cons:

- extension packaging and portability risk
- more moving parts in local bootstrap

### Final Recommendation

Start with Candidate A and leave an internal seam for Candidate B.

Phase 6 should initially ship:

- persisted chunk and embedding rows in `hyperindex-semantic-store`
- flat cosine scan over a metadata-filtered candidate set

Rationale:

- the checked-in benchmark corpus sizes are still small enough for a flat first slice
- the main risk right now is correctness and contract fit, not ANN throughput
- it keeps the implementation reviewable and self-validating

Fallback rule:

- if the warm-query p95 targets below are not met after basic filtering and caching, the first
  upgrade path should be a local `usearch`/HNSW sidecar, not an external vector service

## Metadata Filter Strategy

Recommended filter inputs:

- `path_globs` from `SemanticQuery`
- language
- chunk kind
- package root and package name
- file role tags: route, config, test, source
- symbol kind when present

Recommended strategy:

1. apply hard allow-list filters before scoring
2. compute vector scores only for the filtered candidate set
3. apply hybrid rerank only within that filtered set

Rationale:

- Phase 1 semantic queries already provide `path_globs`
- pushdown filters reduce query cost without changing semantic meaning
- file-role metadata improves precision for route/config/test-oriented prompts

Filter rules Phase 6 should not ship:

- hidden repo-global deny lists that mutate benchmark behavior
- query-time filtering that requires probing the repo root outside the snapshot

## Reranking Strategy

Phase 6 should support exactly the current harness modes:

- `off`
- `hybrid`

Recommended `off` behavior:

- rank by semantic similarity only
- deterministic tie-breaks by path, chunk kind, and chunk id

Recommended `hybrid` behavior:

- weighted combination of:
  - semantic score
  - path and filename token overlap
  - symbol-name/export-name token overlap
  - chunk kind priors for route/config/test queries
  - optional static package/test metadata from Phase 5 enrichments

Recommended non-goal:

- do not ship a cross-encoder or LLM reranker in Phase 6

Rationale:

- hybrid rerank is already part of the Phase 1 semantic query contract
- deterministic feature-based reranking is enough to make the first semantic slice benchmarkable
- an LLM reranker would widen scope and complicate reproducibility

## Persistence And Cache Strategy

Recommended semantic store shape:

- one repo-local semantic SQLite store under a new `semantic.store_dir`
- one snapshot-scoped semantic build record per compatible snapshot/model/chunk-schema combination
- persisted chunk metadata plus persisted embedding rows

Recommended persisted identities:

- `semantic_build_id`
- `embedding_model_id`
- `embedding_model_digest`
- `chunk_schema_version`
- `symbol_index_build_id`
- `snapshot_id`

Recommended cache keys:

- chunk identity:
  `sha256(path + byte_span + chunk_kind + symbol_id? + content_sha256 + chunk_schema_version)`
- embedding cache:
  `sha256(chunk_text_digest + embedding_model_digest)`

Rationale:

- Phase 6 needs deterministic rebuild/reuse behavior similar to Phase 4 and Phase 5
- symbol-build identity and chunk-schema identity prevent stale cross-version reuse
- content-hash reuse keeps incremental refresh cheap

## Incremental Update Model

Phase 6 should reuse the same general pattern already proven in Phase 4 and Phase 5:

1. materialize or load the current snapshot
2. find the most recent compatible prior semantic build for the same repo
3. diff snapshots via `SnapshotAssembler::diff(...)`
4. rebuild chunks and embeddings only for:
   - changed paths
   - added paths
   - deleted paths
   - buffer-only changed paths
5. preserve unchanged chunk rows and embedding rows by cache key
6. fall back to full rebuild when:
   - no compatible prior build exists
   - chunk schema changed
   - embedding model digest changed
   - symbol build identity changed incompatibly
   - semantic store rows are corrupt

Recommended changed-file ownership rule:

- semantic chunk ownership is file-scoped first
- symbol-backed chunks are regenerated from the current symbol graph for the owning file
- deleted files remove their owned chunks

## Query Semantics And Response Shape

Phase 6 should ship one public semantic retrieval method, not a generated-answer method.

Recommended public request:

- `SemanticSearchParams`
  - `repo_id`
  - `snapshot_id`
  - `query`
  - `limit`
  - `path_globs`
  - `rerank_mode`

Recommended public response:

- `SemanticSearchResponse`
  - `repo_id`
  - `snapshot_id`
  - `query`
  - `manifest`
  - `hits`
  - `diagnostics`

Recommended hit shape:

- `SemanticHit`
  - `rank`
  - `score`
  - `semantic_score`
  - `rerank_score`
  - `path`
  - `symbol_id` optional
  - `symbol_display_name` optional
  - `chunk_id`
  - `chunk_kind`
  - `start_line`
  - `end_line`
  - `reason`
  - `snippet`

Recommended status method:

- `semantic_status`
  - reports readiness, model id, build identity, store metadata, and diagnostics

Normalization rule for Phase 1:

- `DaemonSemanticAdapter` maps `SemanticHit` into `QueryHit { path, symbol, rank, reason, score }`
  without changing harness artifacts

## Validation Matrix

Phase 6 validation must cover all of the following:

### Rust unit and integration coverage

- chunk construction from symbol-backed and file-backed inputs
- embedding provider interface and cache-key stability
- store persistence and migration validation
- incremental refresh reuse and fallback behavior
- hybrid rerank determinism
- daemon handler contract tests

### Runtime compatibility coverage

- Phase 2 snapshot creation, read-file, and diff behavior still pass
- Phase 4 parser/symbol build and query flows still pass
- Phase 5 impact analyze/explain/status flows still pass

### Harness coverage

- daemon-backed semantic smoke run through `hyperbench`
- full synthetic semantic pack run
- run/report/compare artifact generation unchanged
- refresh scenario coverage through the harness

### Operator coverage

- semantic status inspection
- rebuild after incompatible model or schema change
- readable degraded state when the semantic store is corrupt or missing

## Performance Targets And Measurement

The following targets are concrete enough to guide the first Phase 6 implementation.

Primary measurement harness:

- `hyperbench run --adapter daemon-semantic`
- Phase 1 compare/report outputs
- additive daemon-side build/query timing fields

Recommended first targets on `synthetic-saas-medium`:

- semantic build cold prepare:
  - target `<= 4000 ms`
- warm semantic query latency:
  - target p50 `<= 60 ms`
  - target p95 `<= 120 ms`
- incremental one-file semantic refresh:
  - target p95 `<= 250 ms`
- hero query:
  - `semantic-hero-session-invalidation` top hit must pass
- synthetic semantic pack:
  - overall pass rate target `>= 0.60` for the first shippable slice

Measurement notes:

- cold and warm runs must be recorded separately
- refresh latency must include the semantic refresh path, not only the query call
- compare budgets should be added in new semantic-specific files rather than overwriting existing
  fixture-oriented defaults

## Risks And Mitigations

### Risk: generic local embeddings underperform on code-text queries

Mitigation:

- keep the embedding provider behind one trait
- use chunk metadata and deterministic hybrid rerank to recover precision
- validate against the checked-in semantic pack before discussing provider expansion

### Risk: chunk boundaries drift and make results unstable

Mitigation:

- prefer symbol-backed chunks
- keep fallback windows stable and versioned by chunk schema
- persist chunk ids derived from content and structural metadata

### Risk: flat vector scan misses latency targets on larger corpora

Mitigation:

- push down metadata filters first
- cache embeddings aggressively
- reserve a clean internal seam for a later HNSW sidecar

### Risk: semantic retrieval starts duplicating exact search or impact behavior

Mitigation:

- keep exact search and impact as explicit preserved ownership boundaries
- forbid `impact_analyze` in the semantic hot path
- normalize semantic outputs as retrieval hits only

### Risk: snapshot and buffer overlay correctness regresses

Mitigation:

- derive all semantic inputs from `ComposedSnapshot`
- require explicit tests for `buffer_only_changed_paths`
- keep repo-root file reads out of semantic build/query code

## Definition Of Done

Phase 6 is done only when all of the following are true:

1. A real semantic retrieval engine exists in the Rust workspace.
2. The engine is snapshot-scoped and local-first.
3. Chunking is deterministic and uses symbol-first structural chunks with file fallbacks.
4. Semantic build state persists per repo and reuses compatible prior work incrementally.
5. A daemon-backed semantic adapter runs through the existing Phase 1 harness.
6. The checked-in synthetic semantic pack runs end to end through `hyperbench`.
7. The hero semantic query passes at top-1.
8. Phase 2, Phase 4, and Phase 5 compatibility validations still pass.
9. Docs and status files are updated with commands run, results, risks, and next steps.

## Assumptions That Do Not Require User Input Right Now

- Phase 6 should ship exactly one default embedding provider first, not a provider marketplace.
- The default provider should be local and deterministic rather than hosted.
- The first semantic slice should stay retrieval-only and should not attempt answer synthesis.
- Snapshot-scoped semantic materialization is acceptable, matching the existing runtime model.
- Existing Phase 1 semantic query packs and goldens remain the primary acceptance target.
- A separate `daemon-semantic` adapter is preferable to overloading the existing symbol or impact
  adapters.
