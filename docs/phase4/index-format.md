# Repo Hyperindex Phase 4 Index Format

This document describes the persistence-facing contract for Phase 4 parse artifacts and symbol
indexes.

It is intentionally format-specific enough to implement, but not so detailed that it freezes an
unfinished storage schema.

## Goals

The Phase 4 persisted format must support:

- deterministic rebuilds from `repo_id` plus `snapshot_id`
- durable reuse across daemon restarts
- file-level inspection and debugging
- stable symbol and occurrence identity within the documented model
- future incremental refresh without redesigning the manifest contract

## Parse Artifact Manifest

The public parse persistence record is `ParseArtifactManifest`.

Fields:

- `build_id`
- `repo_id`
- `snapshot_id`
- `parser_config_digest`
- `artifact_root`
- `file_count`
- `diagnostic_count`
- `created_at`

Required invariants:

- one manifest is scoped to exactly one repo/snapshot/build tuple
- `parser_config_digest` changes whenever parse-affecting config changes
- `artifact_root` is deterministic under the runtime data root
- `file_count` counts only files accepted by the parser language packs

## File Artifact Metadata

`FileParseArtifactMetadata` is the portable per-file unit.

Required fields:

- `artifact_id`
- `path`
- `language`
- `source_kind`
- `stage`
- `content_sha256`
- `content_bytes`
- `parser_pack_id`
- `facts`
- `diagnostics`

Recommended implementation rule:

- `artifact_id` should be deterministic from repo, snapshot, path, and content identity

## Symbol Index Manifest

The public symbol persistence record is `SymbolIndexManifest`.

Fields:

- `build_id`
- `repo_id`
- `snapshot_id`
- `parser_build_id`
- `created_at`
- `stats`
- `storage`

This is the handoff object for every query response that wants to prove which index answered the
request.

## Symbol Index Storage Metadata

`storage` is represented by `SymbolIndexStorage`.

Current fields:

- `format`
- `path`
- `schema_version`
- `manifest_sha256`

The initial format enum includes only `sqlite`.

Required invariants:

- `schema_version` must reflect durable on-disk schema compatibility
- `manifest_sha256`, when present, should identify the logical index contents rather than the
  transport response
- `path` should point at a per-repo symbol store, not the control-plane runtime database

## Stats

`SymbolIndexStats` contains:

- `file_count`
- `symbol_count`
- `occurrence_count`
- `edge_count`
- `diagnostic_count`

Rules:

- counts must be deterministic for the same snapshot and config
- counts should describe persisted facts, not query-time filtering
- partial indexing should be surfaced through diagnostics, not hidden by overstating counts

## Suggested On-Disk Layout

The current config contract points at:

- parser artifacts under `RuntimeConfig.parser.artifact_dir`
- symbol storage under `RuntimeConfig.symbol_index.store_dir`

A recommended Phase 4 layout is:

```text
.hyperindex/
  data/
    parse-artifacts/
      <repo_id>/
        <snapshot_id>/
          <parse_build_id>/
            manifest.json
            files/
              <artifact_id>.json
    symbols/
      <repo_id>.symbols.sqlite3
```

The precise internal SQLite tables are intentionally not frozen in this document. The manifest and
metadata contract is the stable boundary for now.

## Suggested Persistence Granularity

Implementations should persist at least:

- one parse build manifest per parse build
- one file artifact metadata record per parsed file
- one symbol-index manifest per symbol-index build
- durable rows for symbols, occurrences, and graph edges

They should not require:

- serialized syntax trees in the public contract
- compiler caches
- whole-program semantic state

## Refresh and Reuse Expectations

This contract is designed so later implementation slices can reuse work safely.

Expected reuse keys:

- `snapshot_id`
- `content_sha256`
- `parser_config_digest`
- `schema_version`

Expected invalidation triggers:

- file content changes
- parse-affecting config changes
- symbol-schema changes

## Forward-Compatibility Rule

Phase 4 may add:

- new optional manifest fields
- new storage formats
- new stats
- richer per-file artifact metadata

Phase 4 should avoid:

- making persistence depend on unspecified in-memory state
- coupling the manifest contract to a single parser implementation detail
- exposing storage internals that the next implementation slice will immediately need to change
