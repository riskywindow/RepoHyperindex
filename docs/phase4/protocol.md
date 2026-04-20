# Repo Hyperindex Phase 4 Protocol Contract

This document defines the public Phase 4 parser and symbol-analysis transport contract.

Scope:

- contract only
- no live parsing, extraction, or type-checker semantics yet
- narrow enough to implement incrementally on top of Phase 2 snapshots

The Rust source of truth is:

- [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/symbols.rs)
- [config.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/config.rs)
- [errors.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/errors.rs)

## Conventions

- All parser and symbol methods are snapshot-scoped. Callers must provide both `repo_id` and `snapshot_id`.
- Line and column positions are 1-based.
- Byte ranges are half-open: `start` inclusive, `end` exclusive.
- `symbol_id` is stable across occurrences within the same indexed snapshot set.
- `occurrence_id` is more precise and may vary across snapshots.
- Responses may stay placeholder-shaped until the real engine lands, but the schema should remain stable.

## Public Methods

### Parse lifecycle

- `parse_build`
  - requests a parse build for a repo/snapshot pair
  - returns a `ParseBuildRecord`
- `parse_status`
  - returns known parse builds for a repo/snapshot pair
- `parse_inspect_file`
  - returns `FileParseArtifactMetadata`
  - may also return `FileFacts` when `include_facts = true`

### Symbol-index lifecycle

- `symbol_index_build`
  - requests a symbol-index build for a repo/snapshot pair
  - returns a `SymbolIndexBuildRecord`
- `symbol_index_status`
  - returns known symbol-index builds for a repo/snapshot pair

### Query methods

- `symbol_search`
  - text lookup over indexed symbol names
  - request shape is `SymbolSearchQuery { text, mode, kinds, path_prefix }`
- `symbol_show`
  - fetches one symbol record by `symbol_id`
- `definition_lookup`
  - returns definition occurrences for one `symbol_id`
- `reference_lookup`
  - returns reference occurrences for one `symbol_id`
- `symbol_resolve`
  - resolves a symbol from a source location
  - selectors supported today:
    - `line_column`
    - `byte_offset`

## Request/Response Shape

Requests still use the Phase 2 envelope:

```json
{
  "protocol_version": "repo-hyperindex.local/v1",
  "request_id": "req-symbol-search-001",
  "method": "symbol_search",
  "params": {}
}
```

Successful responses keep the same success envelope:

```json
{
  "protocol_version": "repo-hyperindex.local/v1",
  "request_id": "req-symbol-search-001",
  "status": "success",
  "method": "symbol_search",
  "result": {}
}
```

Error responses keep the same error envelope:

```json
{
  "protocol_version": "repo-hyperindex.local/v1",
  "request_id": "req-symbol-show-001",
  "status": "error",
  "method": "symbol_show",
  "error": {}
}
```

Concrete examples live in
[examples.json](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/fixtures/api/examples.json).

## Parse Models

### Parse config

`RuntimeConfig` now includes:

- `parser`
  - `enabled`
  - `max_file_bytes`
  - `diagnostics_max_per_file`
  - `cache_mode`
  - `artifact_dir`
  - `language_packs`
- `language_packs`
  - each pack declares `pack_id`, `enabled`, `languages`, `include_globs`, `grammar_version`

Phase 4 ships one default pack, `ts_js_core`, covering:

- `typescript`
- `tsx`
- `javascript`
- `jsx`
- `mts`
- `cts`

### Parse build records

`ParseBuildRecord` is the public unit for parse build status:

- `build_id`
- `state`
- `requested_at`
- `started_at`
- `finished_at`
- `counts`
- optional `manifest`

`ParseArtifactManifest` exists so implementations can persist parse outputs without changing query
responses later.

### Parse inspect-file

`FileParseArtifactMetadata` is intentionally metadata-first:

- content identity
- source origin
- parse stage
- parser pack id
- fact counts
- diagnostics

It does not expose concrete syntax trees or grammar-specific nodes.

## Symbol Models

### Symbol-index config

`RuntimeConfig.symbol_index` currently includes only:

- `enabled`
- `store_dir`
- `default_search_limit`
- `max_search_limit`
- `persist_occurrences`

This is intentionally small. Build scheduling, refresh policy, and retention policy should remain
internal until real indexing behavior exists.

### Query payloads

Query responses reuse a few core records:

- `SymbolRecord`
- `SymbolOccurrence`
- `GraphEdge`
- `SymbolIndexManifest`

The contract is explicit about syntax-derived navigation data, but it does not claim:

- type inference
- overload resolution
- cross-package semantic binding beyond future documented module-resolution rules
- compiler-grade “go to definition” behavior

## Error Contract

`ProtocolError` is now:

- `category`
- `code`
- `message`
- `retriable`
- optional `payload`

`payload` is machine-readable and contains:

- `subject`
- `validation`
- `retry_after_ms`
- `supported_protocol_versions`
- `context`

This replaces the earlier untyped JSON `details` blob.

The Phase 4-specific error codes added here are:

- `unsupported_language`
- `invalid_position`
- `parse_build_not_found`
- `parse_artifact_not_found`
- `index_not_ready`
- `symbol_index_not_found`
- `symbol_not_found`
- `resolution_not_found`
- `snapshot_mismatch`

## Compatibility Rule

The Phase 4 contract is allowed to gain optional fields and additional enum variants.

It should not:

- rename existing methods
- remove existing fields
- change line/column or byte-span semantics
- claim stronger semantics than the implementation actually provides
