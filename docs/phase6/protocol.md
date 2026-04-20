# Repo Hyperindex Phase 6 Semantic Protocol

## Purpose

This document defines the public Phase 6 semantic retrieval contract.

It covers only the retrieval-side protocol and config surface. It does not authorize answer
generation, chat responses, semantic edits, or benchmark adapter behavior.

## Design Rules

- Keep the public daemon surface small.
- Keep the contract retrieval-only.
- Preserve the Phase 2 snapshot model as the only semantic content source.
- Reuse established Phase 4 enums where available:
  - `LanguageId`
  - `SymbolKind`
  - `SourceSpan`
  - `SymbolId`
  - `SymbolIndexBuildId`
- Keep local operator flows additive:
  - `semantic rebuild`
  - `semantic stats`
- Do not add public daemon `semantic_warm_load` or `semantic_rebuild` methods in this slice.

## Public Daemon Methods

Phase 6 exposes these daemon methods:

- `semantic_status`
- `semantic_build`
- `semantic_query`
- `semantic_inspect_chunk`

### `semantic_status`

Purpose:
- report semantic readiness for one repo and snapshot
- enumerate compatible semantic builds known to the runtime
- return machine-readable diagnostics without executing retrieval

Request:
- `SemanticStatusParams`
  - `repo_id`
  - `snapshot_id`
  - `build_id` optional filter

Response:
- `SemanticStatusResponse`
  - `repo_id`
  - `snapshot_id`
  - `state`
  - `capabilities`
  - `builds`
  - `diagnostics`

### `semantic_build`

Purpose:
- materialize or reuse one semantic build record for one repo and snapshot
- return build metadata and manifest fields without changing the query contract

Request:
- `SemanticBuildParams`
  - `repo_id`
  - `snapshot_id`
  - `force`

Response:
- `SemanticBuildResponse`
  - `repo_id`
  - `snapshot_id`
  - `build`

### `semantic_query`

Purpose:
- execute one retrieval query against one repo and snapshot
- return retrieval hits only
- keep normalized retrieval output machine-readable

Request:
- `SemanticQueryParams`
  - `repo_id`
  - `snapshot_id`
  - `query`
  - `filters`
  - `limit`
  - `rerank_mode`

Response:
- `SemanticQueryResponse`
  - `repo_id`
  - `snapshot_id`
  - `query`
  - `manifest` optional
  - `hits`
  - `stats`
  - `diagnostics`

### `semantic_inspect_chunk`

Purpose:
- inspect one chunk record by stable chunk id
- expose serialized chunk text and chunk metadata for debugging and deterministic review

Request:
- `SemanticInspectChunkParams`
  - `repo_id`
  - `snapshot_id`
  - `chunk_id`
  - `build_id` optional

Response:
- `SemanticInspectChunkResponse`
  - `repo_id`
  - `snapshot_id`
  - `manifest` optional
  - `chunk`
  - `diagnostics`

## Core Semantic Types

### Stable identifiers

- `SemanticBuildId`
- `SemanticChunkId`
- `EmbeddingCacheKey`

These ids are stable wire types, not anonymous strings in the public contract.

### Build and manifest types

- `SemanticBuildState`
- `SemanticBuildRecord`
- `SemanticBuildResponse`
- `SemanticIndexManifest`
- `SemanticIndexStorage`
- `SemanticEmbeddingCacheManifest`

### Chunk and serialization types

- `SemanticChunkKind`
- `SemanticChunkSourceKind`
- `SemanticChunkMetadata`
- `SemanticChunkRecord`
- `SemanticChunkTextConfig`
- `SemanticChunkTextMetadata`

### Query and result types

- `SemanticQueryText`
- `SemanticQueryFilters`
- `SemanticQueryParams`
- `SemanticQueryStats`
- `SemanticRetrievalHit`
- `SemanticQueryResponse`

### Provider and cache types

- `SemanticEmbeddingProviderKind`
- `SemanticEmbeddingProviderConfig`
- `SemanticEmbeddingCacheMetadata`

## Query Contract

Phase 6 query input supports:

- natural-language query text:
  - `SemanticQueryText.text`
- path filters / globs:
  - `SemanticQueryFilters.path_globs`
- package filters:
  - `SemanticQueryFilters.package_names`
  - `SemanticQueryFilters.package_roots`
- workspace-root filters:
  - `SemanticQueryFilters.workspace_roots`
- language filters:
  - `SemanticQueryFilters.languages`
- extension filters:
  - `SemanticQueryFilters.extensions`
- symbol-kind filters:
  - `SemanticQueryFilters.symbol_kinds`
- top-k / limit control:
  - `SemanticQueryParams.limit`
- rerank control:
  - `SemanticQueryParams.rerank_mode`

The contract does not add answer text, summary text, or generated explanations beyond retrieval
reason strings and deterministic explanation fields.

## Retrieval Result Contract

Each `SemanticRetrievalHit` contains:

- ordering:
  - `rank`
- scoring:
  - `score`
  - `semantic_score`
  - `rerank_score`
- chunk payload:
  - `chunk`
- operator-facing retrieval evidence:
  - `reason`
  - `snippet`
  - `explanation` optional

`explanation`, when present, is deterministic retrieval evidence only. It may include:

- normalized query terms
- matched text/path/symbol/package terms
- scored rerank signals

`chunk` is the authoritative metadata anchor for filters, inspection, and future benchmark
normalization.

## Query Stats Contract

`SemanticQueryStats` is required in the response and records:

- `limit_requested`
- `candidate_chunk_count`
- `filtered_chunk_count`
- `hits_returned`
- `rerank_applied`
- `elapsed_ms`

This stays retrieval-focused and does not include answer-generation metrics.

## Runtime Config Contract

`RuntimeConfig.semantic` includes:

- `enabled`
- `store_dir`
- `chunk_schema_version`
- `embedding_provider`
- `chunk_text`
- `query`

Nested config types:

- `SemanticEmbeddingProviderConfig`
  - `provider_kind`
  - `model_id`
  - `model_digest`
  - `vector_dimensions`
  - `normalized`
  - `max_input_bytes`
  - `max_batch_size`
- `SemanticChunkTextConfig`
  - `serializer_id`
  - `format_version`
  - `includes_path_header`
  - `includes_symbol_context`
  - `normalized_newlines`
- `SemanticQueryConfig`
  - `default_search_limit`
  - `max_search_limit`
  - `default_rerank_mode`
  - `default_path_globs`

## Error Taxonomy

The shared protocol error payload remains the wire error shape:

- `ProtocolError`
  - `category`
  - `code`
  - `message`
  - `retriable`
  - `payload`

Phase 6 adds semantic-specific categories and subjects:

- `ErrorCategory::Semantic`
- `ErrorSubjectKind::SemanticBuild`
- `ErrorSubjectKind::SemanticChunk`
- `ErrorSubjectKind::SemanticQuery`

Phase 6 semantic error codes:

- `SemanticNotReady`
- `SemanticBuildNotFound`
- `SemanticChunkNotFound`
- `SemanticFilterUnsupported`

Expected usage:

- use `SemanticNotReady` when the snapshot lacks a compatible semantic build or prerequisite
  materialization
- use `SemanticBuildNotFound` when a requested `build_id` is unknown
- use `SemanticChunkNotFound` when a requested `chunk_id` is unknown
- use `SemanticFilterUnsupported` when a filter is well-formed but intentionally unsupported by
  the current semantic implementation

Validation failures should continue using `InvalidRequest` with machine-readable `payload.validation`
entries.

## Fixture Coverage

The protocol fixture
[semantic-examples.json](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/fixtures/api/semantic-examples.json)
roundtrips these public methods and shapes:

- `semantic_status`
- `semantic_build`
- `semantic_query`
- `semantic_inspect_chunk`
- semantic success payloads
- semantic error payloads

## Out Of Scope For This Contract

- `semantic_warm_load`
- public daemon `semantic_rebuild`
- answer-generation payloads
- chat/session payloads
- semantic edit/write actions
- benchmark adapter transport details
