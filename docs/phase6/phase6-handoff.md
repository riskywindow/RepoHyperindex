# Repo Hyperindex Phase 6 Handoff

## What Phase 6 Built

Phase 6 shipped the first real local semantic-retrieval engine for Repo Hyperindex on top of the
existing Phase 2 runtime, Phase 4 symbol graph, and Phase 1 harness seams.

The checked-in implementation now includes:

- deterministic symbol-first chunk materialization with file-backed fallbacks
- snapshot-scoped semantic builds persisted in SQLite
- document/query embedding generation behind a provider/cache seam
- a persisted flat cosine vector index with metadata-filtered retrieval
- deterministic hybrid reranking with explanation payloads
- incremental semantic refresh from `SnapshotDiffResponse`, including buffer-only snapshots
- daemon and CLI integration for:
  - `semantic_status`
  - `semantic_build`
  - `semantic_query`
  - `semantic_inspect_chunk`
  - local operator flows: `semantic doctor`, `semantic stats`, `semantic rebuild`,
    `semantic inspect-index`
- Phase 1 harness integration through the daemon-backed `daemon-semantic` adapter
- readiness hardening so status/runtime only report ready when the persisted vector index can
  actually warm-load

## Intentionally Out Of Scope

Phase 6 still does not ship:

- a global query planner
- trust/orchestration layers or answer synthesis
- exact-search ownership changes or a checked-in Phase 3 exact-search engine
- ANN, quantization, or segment compaction work
- LLM or cross-encoder reranking
- semantic edits, codemods, or any write-side workflow
- UI, extension, browser, cloud, or multi-user behavior
- cross-repo retrieval
- bundled ONNX/runtime packaging beyond the existing external-process seam

Important preserved boundary:

- the repository still has no checked-in Phase 3 exact-search runtime
- Phase 6 therefore owns semantic retrieval only and uses lexical/path/symbol signals additively
  inside reranking
- Phase 7 should keep exact-text retrieval as a separate ownership boundary unless the user
  explicitly reopens scope

## Phase 7 Plug-In Interfaces

### Semantic query execution

- Protocol:
  [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/semantic.rs)
  - `SemanticQueryParams`
  - `SemanticQueryResponse`
  - `SemanticRetrievalHit`
  - `SemanticQueryStats`
- Library:
  [semantic_query.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-semantic/src/semantic_query.rs)
  - `SemanticSearchEngine::search(...)`
  - `SemanticSearchEngine::search_loaded(...)`
  - `SemanticSearchScaffold`
- Daemon service:
  [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/semantic.rs)
  - `SemanticService::query(...) -> Result<SemanticQueryResponse, ProtocolError>`
- Handler entry:
  [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
  - `HandlerRegistry::semantic_query(...)`

Phase 7 should extend query behavior through `SemanticQueryParams` and `SemanticSearchEngine`
rather than inventing a second semantic query path.

### Chunk inspection and metadata access

- Protocol:
  [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/semantic.rs)
  - `SemanticChunkMetadata`
  - `SemanticChunkRecord`
  - `SemanticInspectChunkParams`
  - `SemanticInspectChunkResponse`
- Chunk materialization:
  [chunker.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-semantic/src/chunker.rs)
  - `ScaffoldChunker`
  - `ChunkingPlan`
- Store access:
  [semantic_store.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-semantic-store/src/semantic_store.rs)
  - `SemanticStore::load_chunk(...)`
  - `SemanticStore::list_chunks(...)`
- Daemon service:
  [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/semantic.rs)
  - `SemanticService::inspect_chunk(...)`

`SemanticChunkMetadata` is the authoritative filter/debug anchor. Phase 7 should add chunk-facing
metadata there rather than hiding retrieval state in ad hoc explanation strings.

### Rerank explanations

- Protocol:
  [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/semantic.rs)
  - `SemanticRetrievalExplanation`
  - `SemanticRerankSignal`
  - `SemanticRetrievalHit::{score, semantic_score, rerank_score, explanation}`
- Reranker:
  [semantic_rerank.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-semantic/src/semantic_rerank.rs)
  - `SemanticReranker::build_hit(...)`

Current contract:

- `score` is the returned rank score
- `semantic_score` is the vector score before additive priors
- `rerank_score` is the final reranked score when reranking is on, and the semantic score when it
  is off
- `explanation.signals` is deterministic evidence only; it is not generated prose

Phase 7 should preserve this deterministic explanation model unless the user explicitly asks for a
new public contract.

### Filterable retrieval

- Query filter types:
  [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/semantic.rs)
  - `SemanticQueryFilters`
- Filter application:
  [semantic_query.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-semantic/src/semantic_query.rs)
  - `matches_filters(...)`
  - `matches_path_globs(...)`
- Filterable metadata fields:
  [chunk-model.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase6/chunk-model.md)

Current supported filters are:

- `path_globs`
- `package_names`
- `package_roots`
- `workspace_roots`
- `languages`
- `extensions`
- `symbol_kinds`

Phase 7 should add new filter surfaces by extending `SemanticQueryFilters` plus
`SemanticChunkMetadata` together so daemon, CLI, and harness behavior stay aligned.

### Snapshot and file-content access

- Snapshot contract:
  [snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
  - `ComposedSnapshot`
  - `SnapshotDiffResponse`
  - `SnapshotReadFileResponse`
- Runtime assembler:
  [manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
  - `SnapshotAssembler::resolve_file(...)`
  - `SnapshotAssembler::diff(...)`
- Semantic incremental path:
  [incremental.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-semantic/src/incremental.rs)
  - `IncrementalSemanticIndexer`

Phase 7 should continue to read code only through `ComposedSnapshot` plus `SnapshotAssembler`.
Do not probe repo roots directly in the semantic path.

### Daemon query flow

- Protocol envelope:
  [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
  - `RequestBody::{SemanticStatus, SemanticBuild, SemanticQuery, SemanticInspectChunk}`
  - `SuccessPayload::{SemanticStatus, SemanticBuild, SemanticQuery, SemanticInspectChunk}`
- Request dispatch:
  [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
  - `HandlerRegistry::semantic_status(...)`
  - `HandlerRegistry::semantic_build(...)`
  - `HandlerRegistry::semantic_query(...)`
  - `HandlerRegistry::semantic_inspect_chunk(...)`
- Service implementation:
  [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/semantic.rs)
  - `SemanticService::{status, build, query, inspect_chunk}`

Current behavior worth preserving:

- `semantic_status` is the readiness truth source and now warm-loads the vector index before
  reporting `ready`
- `semantic_build` is the materialization entry point and reuses a compatible build when possible
- `semantic_query` rejects disabled, empty, low-signal, and over-limit requests before retrieval
- `semantic_inspect_chunk` is a deterministic debug/read path, not a planner or answer API

### Benchmark harness integration

- Adapter seam:
  [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  - `DaemonSemanticAdapter.prepare_corpus(...)`
  - `DaemonSemanticAdapter.execute_semantic_query(...)`
  - `DaemonSemanticAdapter.run_incremental_refresh(...)`
  - `_semantic_request_payload(...)`
  - `_compact_semantic_query(...)`
- Runner:
  [runner.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/runner.py)
  - `run_benchmark(...)`

Phase 7 should keep benchmarking routed through the existing adapter and normalized `QueryHit`
contract instead of moving benchmark logic into daemon code.

## Current Tech Debt And Risks

- Retrieval quality is still below the checked-in fixture baseline on the semantic pack hero path.
  The smoke harness runs cleanly, but quality is not yet “done.”
- The vector index is still a persisted flat scan. It is coherent and benchmarkable for current
  corpora, but it is not an ANN design.
- Reranking uses only chunk-local text/path/package/symbol evidence plus existing symbol metadata.
  There is still no exact-search candidate feed.
- Chunk truncation protects reliability on oversized inputs, but truncated tail content can still
  reduce recall on very large files or symbols.
- The build manifest now stays stable across query traffic, but the cache itself still stores both
  document and query embeddings in one SQLite database.
- The external-process/local-ONNX provider seam remains operator-configured; Phase 6 does not ship
  a bundled model runtime.

## Recommended First Milestones For Phase 7

1. Improve semantic quality before widening architecture:
   tune chunk text, lexical priors, and filter-aware candidate behavior until the hero semantic
   query passes at top-1 in the checked-in harness.
2. Add richer chunk metadata and inspection fields only where they directly support better
   retrieval, filtering, or explanation transparency.
3. Decide whether the next scale step is a better candidate set or an ANN layer, but keep the
   public `SemanticQueryParams` / `SemanticQueryResponse` contract stable while doing it.
4. If Phase 7 needs daemon-level semantic orchestration, layer it on top of
   `SemanticService::{status, build, query, inspect_chunk}` instead of bypassing the current read
   model.
5. Keep Phase 1 harness compatibility as a hard gate for any quality/latency work:
   every retrieval change should remain benchmarkable through `daemon-semantic`.

