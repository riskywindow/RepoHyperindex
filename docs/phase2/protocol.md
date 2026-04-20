# Repo Hyperindex Phase 2 Local Protocol Contract

## Purpose

This document defines the public local daemon contract for Phase 2.

The contract is intentionally transport-neutral:

- it does not assume HTTP
- it does not require a live socket implementation yet
- it is suitable for CLI-to-daemon, stdio bridge, or local IPC transport

The Rust source of truth lives in
[hyperindex-protocol](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol),
but this document is the operator-facing and implementation-facing summary.

## Scope

This protocol now covers the Phase 2 runtime spine plus the parser/symbol methods that were later
integrated onto the same local transport:

- health and version
- daemon status
- repo registry operations
- repo status
- watch status and event reads
- snapshot create/show/list/diff/read-file
- buffer set/clear/list
- parse build/status/inspect-file
- symbol index build/status
- symbol search/show/definition/reference/location resolve
- shutdown

It does not cover:

- exact search
- semantic retrieval
- impact analysis

## Versioning

- `protocol_version`: `repo-hyperindex.local/v1`
- `config_version`: `1`

Every request and response carries `protocol_version`.

Breaking schema changes require a new protocol version.

## Transport Model

The message schema is transport-agnostic.

Expected transport progression:

1. stdio-compatible request/response exchange
2. local Unix domain socket transport for the daemon

The message schema must remain unchanged across those transport choices.

## Request Envelope

Every request has:

- `protocol_version`
- `request_id`
- `method`
- `params`

Canonical shape:

```json
{
  "protocol_version": "repo-hyperindex.local/v1",
  "request_id": "req-health-001",
  "method": "health",
  "params": {}
}
```

## Response Envelope

Every response has:

- `protocol_version`
- `request_id`
- `status`
- `method`
- `result` on success
- `error` on failure

Success shape:

```json
{
  "protocol_version": "repo-hyperindex.local/v1",
  "request_id": "req-health-001",
  "status": "success",
  "method": "health",
  "result": {
    "status": "ok",
    "message": "daemon scaffold is healthy"
  }
}
```

Error shape:

```json
{
  "protocol_version": "repo-hyperindex.local/v1",
  "request_id": "req-repos-show-404",
  "status": "error",
  "method": "repos_show",
  "error": {
    "category": "repo",
    "code": "repo_not_found",
    "message": "repo repo-missing was not found",
    "retriable": false,
    "details": {
      "repo_id": "repo-missing"
    }
  }
}
```

## Method Surface

The Phase 2 public method ids are:

- `health`
- `version`
- `daemon_status`
- `repos_add`
- `repos_list`
- `repos_remove`
- `repos_show`
- `repo_status`
- `watch_status`
- `watch_events`
- `snapshots_create`
- `snapshots_show`
- `snapshots_list`
- `snapshots_diff`
- `snapshots_read_file`
- `buffers_set`
- `buffers_clear`
- `buffers_list`
- `parse_build`
- `parse_status`
- `parse_inspect_file`
- `symbol_index_build`
- `symbol_index_status`
- `symbol_search`
- `symbol_show`
- `definition_lookup`
- `reference_lookup`
- `symbol_resolve`
- `shutdown`

## Method Contracts

### `health`

Purpose:

- cheap liveness/readiness check

Request params:

- empty object

Success result:

- `status`: `ok | degraded | starting`
- `message`

### `version`

Purpose:

- expose daemon, protocol, and config contract versions

Request params:

- empty object

Success result:

- `daemon_version`
- `protocol_version`
- `config_version`

### `daemon_status`

Purpose:

- summarize runtime-wide daemon state

Request params:

- empty object

Success result:

- `protocol_version`
- `config_version`
- `runtime_root`
- `state_dir`
- `socket_path`
- `daemon_state`: `starting | running | stopping | stopped`
- `pid`
- `transport`
- `repo_count`
- `manifest_count`
- `scheduler`

### `repos_add`

Purpose:

- add a repo to the persistent registry

Request params:

- `repo_root`
- optional `display_name`
- optional `notes`
- optional `ignore_patterns`
- `watch_on_add`

Success result:

- `repo`

Each repo record includes:

- `repo_id`
- `repo_root`: canonicalized repo root path
- `display_name`
- `created_at`
- `updated_at`
- optional `branch`
- optional `head_commit`
- `is_dirty`
- optional `last_snapshot_id`
- `notes`
- `warnings`
- `ignore_settings`

### `repos_list`

Purpose:

- list registered repos

Request params:

- `include_removed`

Success result:

- `protocol_version`
- `repos`

### `repos_remove`

Purpose:

- remove a repo from the registry

Request params:

- `repo_id`
- `purge_state`

Success result:

- `repo_id`
- `removed`
- `purged_state`

### `repos_show`

Purpose:

- fetch one registered repo record

Request params:

- `repo_id`

Success result:

- `repo`

### `repo_status`

Purpose:

- fetch repo-scoped runtime state

Request params:

- `repo_id`

Success result:

- `repo_id`
- `repo_root`
- `display_name`
- optional `branch`
- optional `head_commit`
- `working_tree_digest`
- `is_dirty`
- `watch_attached`
- `dirty_path_count`
- `dirty_tracked_files`
- `untracked_files`
- `deleted_files`
- `renamed_files`
- `ignored_files`
- `last_snapshot_id`
- `active_job`
- `last_error_code`

Notes:

- `working_tree_digest` is derived from `HEAD` plus sorted working-tree file categories and is
  intended to be stable enough for Phase 2 snapshot identity.
- `renamed_files` and `ignored_files` are practical best-effort fields based on local git status
  output rather than a deeper history analysis.

### `watch_status`

Purpose:

- summarize watcher attachment state

Request params:

- optional `repo_id`

Success result:

- `watchers`

Each watcher record includes:

- `repo_id`
- `attached`
- `backend`
- `last_sequence`
- `dropped_events`

### `watch_events`

Purpose:

- read normalized watch events without exposing transport/watch backend internals

Request params:

- `repo_id`
- optional `cursor`
- `limit`

Success result:

- `repo_id`
- `next_cursor`
- `events`

Each event includes:

- `sequence`
- `kind`: `created | modified | removed | renamed | other`
- `path`
- optional `previous_path`

### `snapshots_create`

Purpose:

- request creation of a snapshot manifest

Request params:

- `repo_id`
- `include_working_tree`
- `buffer_ids`

Success result:

- `snapshot`

Each composed snapshot includes:

- `snapshot_id`
- `repo_id`
- `repo_root`
- `base`
- `working_tree`
- `buffers`

`base` includes:

- `kind`
- `commit`
- `digest`
- `file_count`
- `files`

`working_tree` includes:

- `digest`
- `entries`

Each working-tree entry includes:

- `path`
- `kind`: `upsert | delete`
- optional `content_sha256`
- optional `content_bytes`
- optional `contents`

Each buffer overlay includes:

- `buffer_id`
- `path`
- `version`
- `content_sha256`
- `content_bytes`
- `contents`

Phase 2 notes:

- Base snapshots currently assume a git-backed repo with a resolvable `HEAD` commit.
- Snapshot file resolution precedence is `buffer overlay` then `working-tree overlay` then `base`.

### `snapshots_show`

Purpose:

- fetch one immutable snapshot manifest

Request params:

- `snapshot_id`

Success result:

- `snapshot`

### `snapshots_list`

Purpose:

- list snapshot summaries for a repo

Request params:

- `repo_id`
- `limit`

Success result:

- `repo_id`
- `snapshots`

Each snapshot summary includes:

- `snapshot_id`
- `repo_id`
- `base_commit`
- `working_tree_digest`
- `has_working_tree`
- `buffer_count`

### `snapshots_diff`

Purpose:

- compare two existing snapshots at the manifest/overlay level

Request params:

- `left_snapshot_id`
- `right_snapshot_id`

Success result:

- `left_snapshot_id`
- `right_snapshot_id`
- `changed_paths`
- `added_paths`
- `deleted_paths`
- `buffer_only_changed_paths`

### `snapshots_read_file`

Purpose:

- resolve one path from a composed snapshot using Phase 2 overlay precedence

Request params:

- `snapshot_id`
- `path`

Success result:

- `snapshot_id`
- `path`
- `resolved_from`
  - `kind`: `buffer_overlay | working_tree_overlay | base_snapshot`
  - optional `buffer_id`
- `contents`

Phase 2 note:

- this is a manifest-level diff contract only
- it must not imply semantic impact analysis

### `buffers_set`

Purpose:

- register or replace one unsaved editor buffer overlay

Request params:

- `repo_id`
- `buffer_id`
- `path`
- `version`
- optional `language`
- `contents`

Success result:

- `buffer`

The returned buffer state includes metadata only:

- `buffer_id`
- `repo_id`
- `path`
- `version`
- optional `language`
- `content_sha256`
- `content_bytes`

### `buffers_clear`

Purpose:

- remove one buffer overlay

Request params:

- `repo_id`
- `buffer_id`

Success result:

- `repo_id`
- `buffer_id`
- `cleared`

### `buffers_list`

Purpose:

- list tracked buffers for one repo

Request params:

- `repo_id`

Success result:

- `repo_id`
- `buffers`

### `shutdown`

Purpose:

- request daemon stop

Request params:

- `graceful`
- optional `timeout_ms`

Success result:

- `accepted`
- optional `message`

## Stable Error Taxonomy

Every error payload includes:

- `category`
- `code`
- `message`
- `retriable`
- optional `details`

### Categories

- `validation`
- `config`
- `transport`
- `storage`
- `repo`
- `watch`
- `snapshot`
- `buffer`
- `scheduler`
- `daemon`
- `internal`

### Current codes

- `invalid_request`
- `unsupported_protocol_version`
- `config_not_found`
- `config_invalid`
- `repo_already_exists`
- `repo_not_found`
- `repo_state_unavailable`
- `watch_not_configured`
- `watch_not_running`
- `snapshot_not_found`
- `snapshot_conflict`
- `buffer_not_found`
- `scheduler_busy`
- `shutdown_in_progress`
- `timeout`
- `not_implemented`
- `internal`

The code should be stable enough for CLI handling and tests. Human-readable guidance belongs in
`message`, not in the code field.

## Config Contract

The Phase 2 config contract lives in
[config.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/config.rs).

Top-level fields:

- `version`
- `protocol_version`
- `directories`
- `transport`
- `repo_registry`
- `watch`
- `scheduler`
- `logging`
- `ignores`

### `directories`

- `runtime_root`
- `state_dir`
- `data_dir`
- `manifests_dir`
- `logs_dir`
- `temp_dir`

### `transport`

- `kind`: `stdio | unix_socket`
- `socket_path`
- `connect_timeout_ms`
- `request_timeout_ms`
- `max_frame_bytes`

### `repo_registry`

- `backend`: currently `sqlite`
- `sqlite_path`
- `manifests_dir`

### `watch`

- `backend`: `stub | poll | notify`
- `poll_interval_ms`
- `debounce_ms`
- `batch_max_events`

### `scheduler`

- `max_concurrent_repos`
- `coalesce_window_ms`
- `idle_flush_ms`
- `job_lease_ms`

### `logging`

- `verbosity`: `error | warn | info | debug | trace`
- `format`: `text | json`

### `ignores`

- `global_patterns`
- `repo_patterns`
- `exclude_dot_git`
- `exclude_node_modules`
- `exclude_target`

## Fixtures And Examples

The checked-in protocol fixtures live at:

- [default-config.toml](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/fixtures/config/default-config.toml)
- [examples.json](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/fixtures/api/examples.json)

Those fixtures are covered by roundtrip serialization tests in
[lib.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/lib.rs).

## Phase Boundaries

This contract is intentionally small and Phase-appropriate.

It is acceptable in Phase 2 to define request/response types for future runtime operations as
long as:

- the transport remains abstracted
- the schema remains typed and versioned
- the daemon does not yet implement the full behavior
- the contract does not silently widen scope into parsing or query intelligence
