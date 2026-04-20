# Repo Hyperindex Phase 6 Decisions

## 2026-04-20 Phase 6 Closeout Readiness Truthfulness

### Decision

- Tighten the delivered Phase 6 surface without widening scope:
  - reject `semantic_query` and `semantic_inspect_chunk` when semantic retrieval is disabled
  - make semantic status/runtime readiness depend on a real vector-index warm load, not only on
    stored metadata rows
  - keep `SemanticIndexManifest.embedding_cache.entry_count` scoped to the build’s document
    embeddings instead of the mutable global cache size
- Document the exact Phase 7 plug-in seams in a dedicated handoff doc instead of asking the next
  engineer to recover them from status logs and implementation files.

### Why

- Phase 6 is only useful to Phase 7 if readiness, manifests, and public daemon behavior are
  truthful under corruption, disabled config, and repeated query traffic.
- The checked-in implementation already had the right architecture; the leverage was in removing a
  few misleading states:
  - a disabled semantic runtime still accepted query/inspect calls
  - status could report `ready` even when the persisted vector index would not warm-load
  - query traffic could change manifest cache counts even though the underlying build had not
    changed

### Consequences

- Semantic status is now self-validating for the persisted flat-index path and reports unloadable
  indexes as `not_ready`.
- Aggregate runtime status no longer counts corrupt semantic builds as ready.
- Build manifests stay stable across repeated query executions, which keeps benchmark artifacts and
  Phase 7 inspection flows reproducible.
- Phase 7 now has a single focused handoff document for semantic query, chunk inspection,
  explanations, filters, snapshot access, daemon flow, and harness integration.

## 2026-04-19 Semantic Performance And Durability Hardening

### Decision

- Keep the Phase 6 retrieval architecture intact and harden only the highest-leverage hot paths:
  - touched-file chunk extraction only
  - batched embedding-cache reads
  - prepared query scoring over the persisted flat index
- Treat corrupted embedding-cache rows as disposable:
  delete the bad row and continue as a cache miss.
- Treat embedding-provider availability as part of semantic query readiness.
- Add local operator recovery surfaces instead of widening the public daemon contract:
  - `hyperctl semantic doctor`
  - richer `hyperctl semantic stats`
  - `hyperctl semantic rebuild` local fallback when the daemon is down
- Truncate oversized serialized chunks to the configured embedding-provider
  `max_input_bytes` deterministically rather than failing the build.

### Why

- The user asked for a focused performance/reliability pass with measurable wins, clean recovery,
  and no core-architecture rewrite.
- The biggest avoidable costs were operational rather than architectural:
  incremental rebuilds were still resolving every file, cache reuse still paid one SQLite lookup
  per key, and flat-scan scoring was doing redundant norm work.
- Corrupted semantic cache rows are fully rebuildable from snapshot-derived inputs, so fail-open
  recovery is safer than treating the cache as authoritative.
- A persisted semantic build is not genuinely query-ready if the configured query embedding
  provider cannot start.

### Consequences

- Incremental single-file updates now avoid full-snapshot content resolution in the chunker.
- Embedding generation and cache reuse now pay one prepared SQLite lookup loop per batch instead of
  reopening the store for each key.
- Semantic query latency now reflects the flat scan itself more closely instead of repeated cosine
  setup work.
- Local semantic operator flows can now diagnose:
  - quick-check failures
  - stale/schema-mismatched builds
  - missing or unloadable vector indexes
  - unavailable embedding providers
  - current build-profile timings
- Very large chunks no longer abort semantic builds, but retrieval quality for the truncated tail
  remains a documented limit.

## 2026-04-19 Deterministic Hybrid Reranking And Evidence Fields

### Decision

- Replace the previous `rerank_mode = hybrid` vector-only downgrade with a real deterministic
  hybrid rerank layer inside `hyperindex-semantic`.
- Keep the reranker additive and bounded:
  - semantic score is the base score
  - lexical overlap, path/package hits, symbol-name hits, explicit symbol-kind hints, and existing
    export/default-export visibility add small deterministic priors
- Return additive explanation fields on semantic hits so rerank behavior is inspectable without
  adding generated prose answers.

### Why

- The user explicitly requested semantic query execution with phase-appropriate hybrid reranking
  and explanation fields.
- The execution plan and status notes already selected deterministic structural features over LLM
  reranking for the first quality-improvement slice.
- The current repo still has no exact-search engine to consult, so the reranker must work from
  chunk-local evidence and existing symbol metadata only.

### Consequences

- `semantic_query` now applies real reranking when `rerank_mode = hybrid`.
- `SemanticChunkMetadata` grows export/default-export booleans grounded in the Phase 4 symbol
  model.
- `SemanticRetrievalHit` grows additive explanation payloads while the Phase 1 harness contract
  remains unchanged.

## 2026-04-19 Persisted Flat Vector Index And Vector-Only Query Execution

### Decision

- Ship the first real semantic retrieval layer as:
  - persisted chunk rows
  - persisted chunk-vector rows
  - persisted flat-index metadata in the semantic SQLite store
  - query-time warm-load of that flat cosine index
- Apply semantic-contract metadata filters before vector scoring.
- Keep this slice vector-only and do not apply hybrid reranking yet even when `rerank_mode =
  hybrid` is requested.

### Why

- The Phase 6 execution plan explicitly selected Candidate A first:
  flat cosine scan over a metadata-filtered candidate set.
- The current benchmark corpora are small enough that correctness, determinism, and store
  compatibility are more important than ANN throughput.
- Deferring hybrid reranking keeps the change reviewable and makes result ordering easier to test
  while the first real retrieval engine lands.

### Consequences

- Semantic query execution now returns real nearest-neighbor hits with deterministic tie-breaking
  by path, chunk kind, and chunk id.
- Semantic readiness now depends on a warm-loadable persisted vector index, not only on chunk-row
  presence.
- Local operator inspection grows to include `hyperctl semantic inspect-index`, while benchmark
  harness integration remains a separate next step.

## 2026-04-14 Embedding Provider Boundary And Cache Identity

### Decision

- Separate embedding concerns in the semantic runtime into:
  - provider identity/version
  - provider configuration
  - document embedding calls
  - query embedding calls
- Ship a deterministic fixture provider as the default runtime embedding path for CI and unit
  tests.
- Add one optional real provider path through an external-process contract:
  - `provider_kind = local_onnx` or `external_process`
  - local runtime command configured separately from persisted provider metadata
- Persist embeddings in the semantic SQLite store and key cache entries by:
  - embedding input kind (`document` vs `query`)
  - text digest
  - provider identity/version
  - provider config digest
- Record cache hit/miss/write/batch stats on stored semantic builds so cache reuse is measurable
  from local rebuild and stats flows.

### Why

- The user requested a clean provider seam that can support future experimentation without
  entangling query embeddings, document embeddings, and cache semantics.
- A deterministic fixture provider gives CI-stable vectors without network or heavyweight local
  model dependencies.
- The Phase 6 execution plan selected a local-first ONNX path, but a bundled Rust ONNX runtime
  would widen dependencies materially; a process-backed optional path preserves local-first
  execution while keeping the default repo footprint light.
- Cache invalidation must be explicit when either the text changes or the provider/version/config
  changes; a wider key prevents silent stale-vector reuse across experiments.

### Consequences

- Semantic rebuilds now materialize document embeddings and persist cache metadata on chunk rows.
- Rebuilding the same snapshot with the same provider/config reuses stored embeddings and reports
  cache hits instead of regenerating vectors.
- Query embeddings and document embeddings are intentionally cached under different identities even
  when the raw text matches.
- A future bundled ONNX runtime can replace the process-backed real-provider path without changing
  the cache or store contract, as long as provider identity/version/config are updated honestly.

## 2026-04-13 Symbol-First Semantic Chunk Materialization

### Decision

- Materialize semantic chunks directly from `ComposedSnapshot` through the existing Phase 4
  `SymbolWorkspace` so chunk text, spans, and overlays all resolve against the same snapshot view.
- Use one `symbol_body` chunk per major symbol kind:
  - `class`
  - `interface`
  - `type_alias`
  - `enum`
  - `function`
  - `method`
  - `constructor`
  - top-level or structural `variable` / `constant`
  - class or interface `property` / `field`
- When a file has no major symbol chunks, emit exactly one file-backed fallback chunk using:
  - `config_file`
  - `route_file`
  - `test_file`
  - otherwise `file_header`
- Keep chunk ids stable with the documented ownership tuple:
  - `path`
  - byte span or fallback file region
  - `chunk_kind`
  - optional `symbol_id`
  - owning file `content_sha256`
  - `chunk_schema_version`
- Persist full `SemanticChunkRecord` rows in the semantic SQLite store so `semantic_inspect_chunk`
  can round-trip the stored metadata and serialized text exactly.

### Why

- The Phase 6 execution plan already chose symbol-first chunks with file fallbacks as the
  preferred shape.
- Re-resolving file contents from `ComposedSnapshot` keeps chunk text correct for working-tree and
  buffer overlays without inventing a second content source.
- Full-record persistence is the smallest useful durable format for deterministic debugging while
  embeddings and ANN storage are still deferred.

### Consequences

- Semantic builds now produce real grounded chunk rows even though query execution is still
  placeholder-only.
- Unsaved buffer overlays change both chunk content and chunk identity when the owning file digest
  changes.
- The semantic store schema advances to include durable chunk rows in addition to build metadata.

## 2026-04-13 Public Semantic Contract Shape

### Decision

- Keep the public semantic daemon contract limited to:
  - `semantic_status`
  - `semantic_build`
  - `semantic_query`
  - `semantic_inspect_chunk`
- Do not add public daemon `semantic_rebuild` or `semantic_warm_load` methods in this protocol
  slice.
- Keep local rebuild/stats behavior CLI-local until the real semantic engine needs a wider daemon
  operator contract.

### Why

- The user requested a contract-only slice with a small API.
- Build, query, and inspect are enough to define the public retrieval surface and the persisted
  metadata model.
- Public warm-load or rebuild lifecycle APIs would widen the daemon contract before there is a
  proven need for them.

### Consequences

- The wire contract is explicit enough for implementation without overcommitting to daemon
  lifecycle operations.
- The CLI may still expose local rebuild/stats helpers without changing the public daemon API.

## 2026-04-13 Semantic Schema Reuse Boundary

### Decision

- Reuse existing Phase 4 protocol types where they are already authoritative:
  - `LanguageId`
  - `SymbolKind`
  - `SourceSpan`
  - `SymbolId`
  - `SymbolIndexBuildId`
- Add semantic-specific ids and metadata only for truly new ownership:
  - `SemanticBuildId`
  - `SemanticChunkId`
  - `EmbeddingCacheKey`
  - semantic manifest/build/query/filter/result types

### Why

- This preserves the existing parser/symbol boundary instead of duplicating symbol metadata in a
  second semantic-specific taxonomy.
- It keeps semantic filters phase-appropriate while avoiding contract drift from the symbol graph.

### Consequences

- Symbol-kind and language filters in the semantic query contract are explicitly aligned with prior
  phases.
- Future real semantic implementation can consume Phase 4 facts without changing the wire shape.

## 2026-04-13 Semantic Workspace Scaffold Layout

### Decision

- Create two new Phase 6 crates in the existing Rust workspace:
  - `crates/hyperindex-semantic`
  - `crates/hyperindex-semantic-store`
- Keep semantic protocol/config/status types in `crates/hyperindex-protocol`.
- Keep daemon transport handling in `crates/hyperindex-daemon`.
- Keep operator-facing semantic commands in `crates/hyperindex-cli`.

### Why

- This matches the repository’s current multi-crate runtime style from Phases 4 and 5.
- It keeps semantic retrieval logic, persisted semantic metadata, transport glue, and CLI glue in
  separate ownership zones.
- It avoids widening the Phase 6 slice into benchmark or daemon orchestration code before the core
  semantic seams are stable.

### Consequences

- Phase 6 semantic code now lives in dedicated subtrees with local guardrails:
  - `crates/hyperindex-semantic/AGENTS.override.md`
  - `crates/hyperindex-semantic-store/AGENTS.override.md`
- Future chunking, embedding, cache, and vector work can land inside those crates without
  destabilizing earlier phases.

## 2026-04-13 Public And Local Semantic Surfaces

### Decision

- Add daemon-facing protocol methods for:
  - `semantic_status`
  - `semantic_search`
- Keep these operator flows CLI-local for now:
  - `semantic rebuild`
  - `semantic stats`

### Why

- This follows the Phase 6 execution plan’s recommended split between minimal daemon contract and
  local operator workflows.
- It keeps the public daemon surface small while the semantic engine is still scaffold-only.

### Consequences

- The daemon can report semantic readiness and accept semantic query requests without exposing
  extra rebuild lifecycle methods yet.
- The CLI can materialize and inspect local semantic scaffold builds without adding new daemon
  handlers.

## 2026-04-13 Honest Placeholder Retrieval Semantics

### Decision

- Persist snapshot-scoped semantic build metadata now.
- Return explicit placeholder diagnostics for chunking, embeddings, vector search, and semantic
  query execution.
- Do not synthesize fake semantic hits.
- Keep `symbol_index_build_id` optional in the scaffold build manifest until real symbol-backed
  chunk materialization lands.

### Why

- The user requested scaffold-only Phase 6 components with no real chunk extraction, embeddings,
  vector index, daemon handlers beyond glue, or benchmark integration.
- Placeholder diagnostics are safer and more reviewable than pretending the semantic engine is
  ready.

### Consequences

- The Phase 6 workspace compiles and persists build metadata, but semantic search currently returns
  empty hit lists with clear diagnostics.
- Real chunk extraction, embedding generation, and vector search can now be added incrementally on
  top of a stable transport/store layout.
