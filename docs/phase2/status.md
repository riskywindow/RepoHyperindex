# Repo Hyperindex Phase 2 Status: Complete

## 2026-04-12 Impact Runtime Extension

### Scope

- Extended the existing local Phase 2 daemon/runtime to host the real Phase 5 impact path.
- Kept the integration local-only and observable through the current daemon protocol plus
  `hyperctl`.
- Did not add editor UI, semantic retrieval endpoints, or non-local transport.

### What Was Completed

- Wired the daemon runtime to serve:
  - `impact_status`
  - `impact_analyze`
  - `impact_explain`
- Added impact runtime summary fields to daemon status so operators can see:
  - configured impact store root
  - materialized snapshot count
  - ready build count
  - stale build count
- Added `hyperctl impact status` and `hyperctl impact explain` alongside the existing analyze
  command, with JSON output as the primary machine-consumed mode.
- Extended the checked-in daemon smoke script to prove the north-star local flow:
  - register repo
  - create snapshot
  - build symbol prerequisites
  - analyze impact
  - apply a buffer overlay
  - show refreshed impact output

### Commands Run

```bash
cargo fmt --all
cargo test -p hyperindex-daemon -p hyperindex-cli -p hyperindex-impact-store -p hyperindex-protocol
bash scripts/phase2-smoke.sh
cargo test -p hyperindex-protocol -p hyperindex-daemon -p hyperindex-cli -p hyperindex-impact-store
```

### Command Results

- `cargo fmt --all`
  - passed
- `cargo test -p hyperindex-daemon -p hyperindex-cli -p hyperindex-impact-store -p hyperindex-protocol`
  - passed
- `bash scripts/phase2-smoke.sh`
  - passed
  - validated the long-running daemon path, impact CLI flow, buffer-overlay refresh, and runtime
    status impact summary over the local Unix-socket transport
- `cargo test -p hyperindex-protocol -p hyperindex-daemon -p hyperindex-cli -p hyperindex-impact-store`
  - passed
  - revalidated the protocol fixture catalog and the daemon/CLI impact flow after the final
    example update

### Remaining Risks / TODOs

- Runtime-wide daemon status now reports aggregate impact readiness, but repo-level status does not
  yet inline impact details.
- The long-running daemon smoke path is covered by the checked-in shell script rather than a cargo
  integration test.

### Next Recommended Prompt

- surface repo-scoped impact readiness directly in `repo status` if operators need it
- profile repeated `impact_explain` calls before deciding whether to persist dedicated explain
  indexes

## Phase State

Phase 2 is complete.

The local runtime spine is coherent, validated, and documented for Phase 3 handoff. Repo
registration, git-state inspection, snapshots, buffers, daemon request routing, maintenance
commands, and the daemon-backed `hyperctl` path are all in place. The intentionally preserved
boundary is still sharp for the harness and the local runtime model: Phase 1 stays in `bench/`,
and the Phase 2 daemon/runtime remains local-only and observable even after taking on the real
parser and symbol services from the later Phase 4 slice.

## Latest Runtime Extension

Date:

- 2026-04-09

Scope:

- Extended the existing Phase 2 daemon/runtime to host the real parser and symbol services.
- Kept the integration local-only and machine-consumable through the current daemon protocol and
  `hyperctl`.
- Did not add editor UI, impact endpoints, or any non-local transport surface.

What Was Completed:

- Replaced the daemon-side Phase 4 placeholder service with real parser and symbol orchestration
  in:
  - [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/symbols.rs)
  - [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
- The daemon can now:
  - build or load parse artifacts for a repo/snapshot pair
  - build or load the durable symbol store and resolved symbol graph
  - reuse prior indexed snapshots incrementally from snapshot diffs when possible
  - answer symbol search/show/definition/reference/location queries against the real graph
- Extended daemon status to report parser and symbol-store runtime summaries via:
  - [status.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/status.rs)
  - [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - [daemon.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/daemon.rs)
- Promoted parser/symbol build metadata so machine consumers can see parse reuse and symbol
  refresh mode/fallback details through:
  - [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/symbols.rs)
  - [examples.json](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/fixtures/api/examples.json)
- Switched `hyperctl parse` onto the daemon protocol and expanded `hyperctl symbol` with the
  current user path:
  - `hyperctl parse build`
  - `hyperctl parse status`
  - `hyperctl parse inspect-file`
  - `hyperctl symbol build`
  - `hyperctl symbol status`
  - `hyperctl symbol search`
  - `hyperctl symbol show`
  - `hyperctl symbol defs`
  - `hyperctl symbol refs`
  - `hyperctl symbol resolve`
- Added daemon and CLI smoke coverage proving:
  - repo register -> snapshot create -> parse build -> symbol build -> query
  - buffer overlay -> refreshed snapshot -> incremental symbol rebuild -> refreshed query results

Key Decisions:

- Keep one local daemon/runtime boundary for repo, snapshot, buffer, parse, and symbol state
  instead of creating a separate Phase 4 control plane.
- Make JSON-first daemon/CLI responses the source of truth for machine-consumed parser/symbol
  commands, with human-readable summaries as a secondary layer.
- Prefer incremental symbol refresh from prior indexed snapshots when the snapshot diff and stored
  state are compatible, and fall back to a full rebuild when they are not.

Commands Run For The Latest Runtime Task:

```bash
cargo check -p hyperindex-daemon -p hyperindex-cli
cargo test -p hyperindex-protocol -p hyperindex-daemon -p hyperindex-cli
cargo fmt --all
```

Command Results:

- `cargo check -p hyperindex-daemon -p hyperindex-cli`
  - passed
- `cargo test -p hyperindex-protocol -p hyperindex-daemon -p hyperindex-cli`
  - passed
  - validated the shared protocol fixture roundtrip, daemon parser/symbol smoke flow, and the
    daemon-backed CLI refresh smoke
- `cargo fmt --all`
  - passed

Latest Risks / TODOs:

- Runtime status currently reports parser and symbol summary counts, but repo-scoped status still
  depends on the dedicated parse/symbol status endpoints for detailed build information.
- The symbol store currently rebuilds the resolved graph in memory from persisted facts at query
  time; that is acceptable for the current local path but may need caching if query latency
  becomes a real concern.
- The current CLI parser/symbol commands are daemon-backed in stdio mode for tests and local
  scripting, but the long-running Unix-socket lifecycle path is still constrained by sandbox
  limits in this environment.

Next Recommended Prompt:

- surface repo-scoped parser/symbol build summaries directly in `repo status` if operators need a
  single summary view
- add a daemon-backed harness adapter slice for symbol benchmark execution without changing the
  existing Phase 1 result schema
- keep semantic retrieval, impact analysis, and any UI/editor work out of scope

## What Was Completed

- Preserved the Phase 1 harness boundary and kept all `hyperbench`-owned behavior under `bench/`.
- Expanded the shared protocol contracts in:
  - [crates/hyperindex-protocol/src/repo.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/repo.rs)
  - [crates/hyperindex-protocol/src/snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
  - [crates/hyperindex-protocol/src/buffers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/buffers.rs)
- Added a daemon RPC client plus daemon-backed CLI routing in:
  - [crates/hyperindex-cli/src/client.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/client.rs)
  - [crates/hyperindex-cli/src/bin/hyperctl.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/bin/hyperctl.rs)
  - [crates/hyperindex-cli/src/commands/daemon.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/daemon.rs)
  - [crates/hyperindex-cli/src/commands/repo.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/repo.rs)
  - [crates/hyperindex-cli/src/commands/snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/snapshot.rs)
  - [crates/hyperindex-cli/src/commands/buffers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/buffers.rs)
- Added daemon lifecycle commands:
  - `hyperctl daemon start`
  - `hyperctl daemon status`
  - `hyperctl daemon stop`
- Promoted `--json` to a first-class output mode for the machine-consumed daemon, repo, snapshot,
  and buffer commands, including structured JSON error output from `hyperctl`.
- The public Phase 2 snapshot model now has explicit types for:
  - `BaseSnapshot`
  - `WorkingTreeOverlay`
  - `BufferOverlay`
  - `ComposedSnapshot`
- Implemented reusable git-state parsing and inspection in:
  - [crates/hyperindex-git-state/src/status.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-git-state/src/status.rs)
  - [crates/hyperindex-git-state/src/inspect.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-git-state/src/inspect.rs)
- Implemented the core snapshot logic in:
  - [crates/hyperindex-snapshot/src/base.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/base.rs)
  - [crates/hyperindex-snapshot/src/overlays.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/overlays.rs)
  - [crates/hyperindex-snapshot/src/manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
- The snapshot layer now supports:
  - git-backed base snapshots from a stable `HEAD` commit
  - working-tree upsert/delete overlays relative to the base
  - unsaved buffer overlays with deterministic ordering
  - deterministic snapshot ids from stable component hashes
  - file resolution precedence of buffer overlay, then working-tree overlay, then base snapshot
  - snapshot diffing for changed, added, deleted, and buffer-only changed paths
- Implemented buffer persistence in SQLite and snapshot manifest persistence as JSON files plus a
  SQLite index in:
  - [crates/hyperindex-repo-store/src/buffers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-repo-store/src/buffers.rs)
  - [crates/hyperindex-repo-store/src/manifests.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-repo-store/src/manifests.rs)
  - [crates/hyperindex-repo-store/src/migrations.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-repo-store/src/migrations.rs)
- Preserved the existing repo registry commands:
  - `hyperctl repos add`
  - `hyperctl repos list`
  - `hyperctl repos show`
  - `hyperctl repos remove`
- Preserved the existing repo inspection commands:
  - `hyperctl repo status`
  - `hyperctl repo head`
- Preserved the existing watch smoke path:
  - `hyperctl watch once --repo-id <id> --timeout-ms <n> [--json]`
- Added buffer CLI commands in:
  - [crates/hyperindex-cli/src/commands/buffers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/buffers.rs)
  - [crates/hyperindex-cli/src/bin/hyperctl.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/bin/hyperctl.rs)
  - `hyperctl buffers set --from-file`
  - `hyperctl buffers clear`
  - `hyperctl buffers list`
- Added snapshot CLI commands in:
  - [crates/hyperindex-cli/src/commands/snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/snapshot.rs)
  - [crates/hyperindex-cli/src/bin/hyperctl.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/bin/hyperctl.rs)
  - `hyperctl snapshot create`
  - `hyperctl snapshot show`
  - `hyperctl snapshot diff`
  - `hyperctl snapshot read-file`
- Updated the checked-in protocol examples and docs to match the new snapshot and buffer shapes:
  - [crates/hyperindex-protocol/fixtures/api/examples.json](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/fixtures/api/examples.json)
  - [docs/phase2/protocol.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase2/protocol.md)
- Added tests covering:
  - deterministic git working-tree digests
  - practical rename and ignored-file reporting
  - snapshot precedence rules
  - deterministic snapshot identity
  - snapshot diff correctness
  - buffer overlay wins without saving to disk
  - manifest and buffer persistence roundtrips
  - CLI snapshot/buffer behavior
- Implemented the first live daemon runtime in:
  - [crates/hyperindex-daemon/src/runtime.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/runtime.rs)
  - [crates/hyperindex-daemon/src/state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - [crates/hyperindex-daemon/src/handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
  - [crates/hyperindex-daemon/src/server.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/server.rs)
  - [crates/hyperindex-daemon/src/bin/hyperd.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/bin/hyperd.rs)
- `hyperd` now provides:
  - config loading and runtime directory creation
  - pid file creation and socket-path bootstrap for Unix-socket mode
  - stdio one-shot transport fallback through the same request/response contract
  - structured tracing initialization from config
  - clean lifecycle transitions for starting, running, stopping, and stopped states
- Added an internal daemon state manager that owns:
  - scheduler state
  - watcher attachment records
  - last per-repo error codes
  - an in-memory watch-event queue
  - connected-client and lifecycle metadata
- Upgraded the scheduler scaffold into an observable in-memory job model with:
  - `RepoRefresh`
  - `WatchIngest`
  - `SnapshotCapture`
  - pending/running/succeeded/failed state transitions
- Implemented daemon protocol handlers for:
  - `health`
  - `version`
  - `daemon_status`
  - `repos_add`
  - `repos_list`
  - `repos_show`
  - `repos_remove`
  - `repo_status`
  - `snapshots_create`
  - `snapshots_show`
  - `snapshots_list`
  - `snapshots_diff`
  - `snapshots_read_file`
  - `buffers_set`
  - `buffers_clear`
  - `buffers_list`
  - `shutdown`
- Added daemon smoke coverage that exercises:
  - health
  - repo add
  - daemon status
  - repo status
  - buffer set/clear
  - snapshot create/read-file/diff
  - shutdown
- Added an operator-facing runbook and smoke script in:
  - [docs/phase2/how-to-run-phase2.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase2/how-to-run-phase2.md)
  - [scripts/phase2-smoke.sh](/Users/rishivinodkumar/RepoHyperindex/scripts/phase2-smoke.sh)
- Hardened daemon bootstrap and store recovery in:
  - [crates/hyperindex-daemon/src/runtime.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/runtime.rs)
  - [crates/hyperindex-daemon/src/state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - [crates/hyperindex-repo-store/src/db.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-repo-store/src/db.rs)
  - [crates/hyperindex-repo-store/src/manifests.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-repo-store/src/manifests.rs)
  - [crates/hyperindex-repo-store/src/repos.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-repo-store/src/repos.rs)
- Added practical local maintenance commands in:
  - [crates/hyperindex-cli/src/commands/maintenance.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/maintenance.rs)
  - [crates/hyperindex-cli/src/bin/hyperctl.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/bin/hyperctl.rs)
  - `hyperctl doctor`
  - `hyperctl cleanup`
  - `hyperctl reset-runtime`
- Local runtime hardening now includes:
  - stale pid/lock and socket cleanup before restart
  - SQLite corruption quarantine plus automatic store rebuild
  - repo-registry sidecar backup and manifest reindex on store reopen
  - clearer repo-missing and buffer-overlay validation errors with recovery hints
  - snapshot and repo-state recovery that survives daemon restarts
- Tightened the Phase 2 deliverable review with two minimal but high-leverage fixes:
  - in-memory `RepoStore` instances now use isolated temp-backed artifact paths instead of leaking
    manifest or backup files into the workspace during tests
  - `hyperctl repos remove --purge-state` now removes manifest files and buffer rows as well as
    SQLite metadata, so restart/reindex cannot resurrect supposedly purged repo state
- Added targeted tests covering:
  - restart with existing runtime state
  - stale socket/lock cleanup
  - corrupted SQLite recovery with manifest reindex
  - invalid repo path handling
  - missing repo-root handling
  - bad buffer overlay input
  - purge-state removing manifests and buffers durably
  - in-memory manifest persistence staying off workspace paths

## Key Decisions

- Keep git introspection narrow and deterministic by relying on local `git` porcelain-v1 output.
- Model snapshots explicitly as immutable git-backed base files plus working-tree and buffer
  overlays.
- Resolve composed snapshot files with strict precedence: buffer overlay, then working-tree
  overlay, then base snapshot.
- Use deterministic hashes for file content metadata, working-tree digests, and snapshot ids.
- Persist snapshot manifests as JSON files plus a SQLite index, while keeping mutable buffer state
  in SQLite.
- Make the normal `hyperctl` operator path daemon-first and keep direct-library flows internal to
  tests or helper code.
- Keep the daemon state manager in memory and open `RepoStore` per request instead of sharing one
  long-lived SQLite connection across async server work.
- Keep the scheduler intentionally local and observable rather than durable or distributed.
- Preserve Unix sockets as the primary daemon transport while keeping a stdio fallback path for
  transport abstraction and environments where local socket binding is restricted.
- Prefer pragmatic local repair over opaque failure:
  - quarantine corrupted SQLite files instead of trying in-place salvage
  - restore the repo registry from a JSON sidecar backup
  - reindex immutable snapshot manifests from disk on reopen
  - keep `hyperctl doctor|cleanup|reset-runtime` local-only and intentionally small
- Treat purge as real purge:
  - removing a repo with `--purge-state` must delete both indexed state and on-disk manifest
    artifacts so the next daemon/store bootstrap does not recreate removed state
- Keep test-only state isolated:
  - `RepoStore::open_in_memory()` now uses a temp-backed manifest/backup root to avoid polluting
    the repository worktree during validation

## Commands Run

```bash
/bin/zsh -lc 'cargo fmt --all'
/bin/zsh -lc 'CARGO_TARGET_DIR=/tmp/repohyperindex-target cargo test --workspace'
/bin/zsh -lc 'UV_CACHE_DIR=/tmp/uv-cache uv run pytest'
git diff --check
```

## Command Results

- `cargo fmt --all`
  - passed
- `CARGO_TARGET_DIR=/tmp/repohyperindex-target cargo test --workspace`
  - passed
  - validated the full Phase 2 Rust workspace, including daemon smoke, snapshot/buffer flows,
    watcher normalization, git inspection, repo-store recovery, and the new purge/in-memory-store
    regressions
- `UV_CACHE_DIR=/tmp/uv-cache uv run pytest`
  - passed with `63` tests green
  - confirmed the Phase 1 harness still works without behavior changes
- `git diff --check`
  - passed

## Remaining Risks / TODOs

- Repo registry backup plus manifest reindex is intentionally pragmatic, not a full transactional
  recovery system; it is meant for local restart and local repair flows, not for enterprise-grade
  crash recovery.
- Unix-socket bootstrap is implemented, but the local sandbox used for testing does not permit
  `bind()` on Unix-domain sockets, so the checked-in live smoke script cannot complete in this
  environment even though it is the intended local-demo path on a normal workstation.
- Watch events are still in-memory only; there is not yet a persistent event log or cursor store.
- `watch_status` and `watch_events` remain typed but intentionally unimplemented in the live daemon.
- Watcher attachment records exist in daemon state, but there is not yet a background watcher pump
  that continuously refreshes and persists event batches.
- Snapshot creation currently assumes a git-backed repo with a resolvable `HEAD` commit.
- Snapshot contents are currently handled as UTF-8 text, which is acceptable for the TypeScript
  Phase 2 wedge but should be revisited only if binary/non-UTF-8 support becomes a real
  requirement.
- Snapshot access is currently whole-manifest plus point lookup. Phase 3 may want a more explicit
  parser/index-facing file enumeration API, but that should be layered on the existing snapshot
  model rather than replacing it.

## Next Recommended Prompt

Start Phase 3 on top of the documented Phase 2 seams in
[phase2-handoff.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase2/phase2-handoff.md).

- keep the current protocol, snapshot, repo-store, and daemon seams stable
- make watcher ingestion durable before adding parser/index work
- build Phase 3 file access on `ComposedSnapshot` and `SnapshotAssembler`
- preserve the Phase 1 harness boundary and adapter ownership
- keep parser/index/search work out of Phase 2 docs and status updates
