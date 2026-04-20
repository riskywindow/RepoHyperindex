# Repo Hyperindex Phase 6 Index Format

## Purpose

This document defines the Phase 6 persisted semantic index metadata contract.

It describes the build manifest, storage metadata, and cache metadata that another engineer needs
to implement a real semantic store without re-deriving the wire contract.

## Scope

This document covers:

- semantic build records
- semantic index manifests
- storage metadata
- embedding-cache metadata

It does not define a production ANN format in this slice.

## Persistence Model

Phase 6 persists one semantic build record per repo and snapshot compatibility boundary.

Compatibility inputs:

- `snapshot_id`
- `semantic_config_digest`
- `chunk_schema_version`
- embedding provider `model_digest`
- optional `symbol_index_build_id`

The public manifest type is:

- `SemanticIndexManifest`

The public build-record type is:

- `SemanticBuildRecord`

## Build Record Contract

`SemanticBuildRecord` contains:

- `build_id`
- `state`
- `requested_at`
- `started_at`
- `finished_at`
- `manifest`
- `diagnostics`
- `loaded_from_existing_build`

Meaning:

- `build_id` is the stable semantic build identity
- `state` reports build lifecycle state
- timestamps are machine-readable and deterministic in shape
- `manifest` is optional so in-progress or failed builds can still surface metadata cleanly
- `loaded_from_existing_build` distinguishes reuse from fresh materialization

## Manifest Contract

`SemanticIndexManifest` contains:

- build identity:
  - `build_id`
  - `repo_id`
  - `snapshot_id`
- compatibility metadata:
  - `semantic_config_digest`
  - `chunk_schema_version`
  - `symbol_index_build_id` optional
- embedding provider metadata:
  - `embedding_provider`
- chunk text serializer metadata:
  - `chunk_text`
- storage metadata:
  - `storage`
- embedding cache metadata:
  - `embedding_cache`
- build counts:
  - `indexed_chunk_count`
  - `indexed_file_count`
- build timestamp:
  - `created_at`

## Storage Metadata

`SemanticIndexStorage` defines the persisted storage handle:

- `format`
- `path`
- `schema_version`
- `manifest_sha256`

Phase 6 currently approves:

- `SemanticStorageFormat::Sqlite`

`manifest_sha256` is optional because the storage backend may compute it lazily.

## Embedding Provider Metadata

`SemanticEmbeddingProviderConfig` is part of the manifest because build compatibility depends on
it.

Required fields:

- `provider_kind`
- `model_id`
- `model_digest`
- `vector_dimensions`
- `normalized`
- `max_input_bytes`
- `max_batch_size`

This is configuration metadata, not a runtime health report.

## Embedding Cache Metadata

Build-level cache metadata:

- `SemanticEmbeddingCacheManifest`
  - `key_algorithm`
  - `entry_count`
  - `store_path` optional

Per-entry metadata:

- `SemanticEmbeddingCacheMetadata`
  - `cache_key`
  - `input_kind`
  - `model_digest`
  - `text_digest`
  - `provider_config_digest`
  - `vector_dimensions`
  - `normalized`
  - `stored_at` optional

Recommended key semantics:

- `cache_key = sha256(input_kind + text_digest + provider_identity + provider_config_digest)`

## Chunk Text Metadata

The persisted index format separates build-level serializer policy from per-chunk text facts.

Build-level:

- `SemanticChunkTextConfig`

Per-chunk:

- `SemanticChunkTextMetadata`

Why:

- serializer policy changes should invalidate compatibility deterministically
- per-chunk text facts should remain cheap to diff and inspect

## Query-Time Manifest Use

`SemanticQueryResponse.manifest` should return the manifest for the build used by the query.

This allows:

- reproducible benchmark runs
- machine-readable operator inspection
- stale-build detection without reading local store internals

## Required Invariants

- `build_id` must uniquely identify one compatible semantic build
- `storage.schema_version` must match the active semantic store schema
- `embedding_provider.model_digest` must be stable for cache compatibility
- `chunk_schema_version` must gate chunk-id reuse
- `indexed_chunk_count` and `indexed_file_count` must describe the materialized build, not query
  results

## Intentionally Deferred

This contract intentionally defers:

- ANN graph layout
- segment compaction rules
- vector quantization
- multi-build eviction policy
- cross-repo storage
- remote-store formats

Those choices can be added later without changing the current public semantic manifest fields.
