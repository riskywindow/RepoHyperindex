# Repo Hyperindex Phase 6 Chunk Model

## Purpose

This document defines the Phase 6 semantic chunk contract.

It is the authoritative description of chunk identity, chunk metadata, serialized chunk text, and
which chunk fields are safe to use for filtering and retrieval explanation.

## Chunking Principles

- Chunks are retrieval units, not answer units.
- Chunk ids must be stable across repeated builds of the same snapshot and config.
- Chunk metadata must be deterministic and diff-friendly.
- Symbol-backed chunks remain additive to the Phase 4 symbol graph.
- File-backed fallback chunks remain additive to the snapshot model.

## Chunk Identity

Public chunk id type:

- `SemanticChunkId`

Recommended stable identity inputs:

- `path`
- source span or file-relative region
- `SemanticChunkKind`
- optional `SymbolId`
- owning file content digest
- `chunk_schema_version`

Recommended construction rule:

- stable hash over the ownership tuple above
- render as a string-safe id suitable for logs, fixtures, and API payloads

## Chunk Kinds

Phase 6 approved kinds:

- `symbol_body`
- `file_header`
- `route_file`
- `config_file`
- `test_file`
- `fallback_window`

These kinds are retrieval hints and filter surfaces. They are not engine-specific ranking labels.

## Chunk Source Kinds

`SemanticChunkSourceKind` records ownership provenance:

- `symbol`
- `file`

Meaning:

- `symbol` means the chunk is anchored to a Phase 4 symbol identity
- `file` means the chunk is anchored only to a file or file region

## Chunk Metadata Contract

`SemanticChunkMetadata` includes:

- identity:
  - `chunk_id`
  - `chunk_kind`
  - `source_kind`
- file anchor:
  - `path`
  - `extension` optional
  - `language` optional
- workspace/package metadata:
  - `package_name` optional
  - `package_root` optional
  - `workspace_root` optional
- symbol metadata:
  - `symbol_id` optional
  - `symbol_display_name` optional
  - `symbol_kind` optional
  - `symbol_is_exported` optional
  - `symbol_is_default_export` optional
  - `span` optional
- content anchoring:
  - `content_sha256`
- serialized text metadata:
  - `text`

## Filter Semantics

Query filters apply against chunk metadata:

- path filters match `path`
- package-name filters match `package_name`
- package-root filters match `package_root`
- workspace-root filters match `workspace_root`
- language filters match `language`
- extension filters match `extension`
- symbol-kind filters match `symbol_kind`

If a chunk omits an optional field, it does not match filters that require that field.

## Chunk Text Serialization Contract

Chunk text metadata is split into:

- serializer config:
  - `SemanticChunkTextConfig`
- per-chunk serialized text facts:
  - `SemanticChunkTextMetadata`

### `SemanticChunkTextConfig`

This is build-level serializer policy:

- `serializer_id`
- `format_version`
- `includes_path_header`
- `includes_symbol_context`
- `normalized_newlines`

### `SemanticChunkTextMetadata`

This is per-chunk serialization metadata:

- `serializer_id`
- `format_version`
- `text_digest`
- `text_bytes`
- `token_count_estimate`

`text_digest` is the stable content key used by embedding-cache identity.

## Chunk Inspection Contract

`SemanticChunkRecord` is the inspection payload:

- `metadata`
- `serialized_text`
- `embedding_cache` optional

This shape is intentionally retrieval/debugging oriented. It does not include answer-generation
fields, prompt templates, or hidden model annotations.

## Symbol-Backed Chunks

When `source_kind = symbol`:

- `symbol_id` should be present
- `symbol_kind` should be present when the symbol graph provides it
- `span` should match the Phase 4 symbol record span
- `symbol_display_name` should be stable and human-readable

The semantic layer must not redefine symbol meanings or edge kinds.

## File-Backed Chunks

When `source_kind = file`:

- `symbol_id` may be absent
- `span` may be absent for whole-file anchors or present for fallback windows
- package/workspace metadata remains valid when derivable

## Required Determinism

Chunk materialization must be deterministic with respect to:

- snapshot contents
- buffer overlays
- chunk schema version
- serializer config
- provider model digest only where it affects cache identity, not chunk identity

## Fields Intentionally Excluded

The chunk contract does not include:

- generated answer text
- natural-language summaries of code
- speculative confidence prose
- backend-specific ANN payloads
- mutable ranking annotations stored on the chunk itself
