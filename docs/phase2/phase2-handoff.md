# Repo Hyperindex Phase 2 Handoff

## What Phase 2 Built

Phase 2 established the local runtime spine for Repo Hyperindex without implementing code
intelligence:

- a Rust workspace outside `bench/` with separate crates for protocol, config, store, watcher,
  git-state, snapshot assembly, scheduler, daemon, and CLI
- a versioned local JSON request/response contract plus typed config models
- a persistent local repo registry in SQLite, immutable snapshot manifests on disk, and mutable
  buffer state in SQLite
- git-backed base snapshots, working-tree overlays, buffer overlays, deterministic snapshot ids,
  snapshot diffing, and file-resolution precedence
- a local daemon with typed handlers for daemon, repo, snapshot, and buffer flows
- a daemon-backed `hyperctl` path for repo registration, repo status, snapshot creation, snapshot
  diff/read, buffer set/clear/list, runtime maintenance, and daemon lifecycle
- targeted smoke coverage proving repo add -> status -> buffer -> snapshot -> read-file -> diff ->
  shutdown through the runtime spine

Phase 1 remained separate and unchanged under `bench/`.

## Intentionally Still Out Of Scope

Phase 2 still does not implement:

- parsing, AST extraction, or symbol tables
- indexing, search, ranking, or retrieval
- impact analysis
- a VS Code extension, browser UI, cloud service, or multi-user runtime
- a durable background watcher pump or persistent watch-event cursor API

The only goal here was to make the local runtime boundary real enough for Phase 3 to plug into.

## Phase 3 Plug-In Interfaces

### File access by snapshot

The current snapshot construction and resolution seam is:

- [base.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/base.rs)
  - `build_base_snapshot(repo_root: &Path, commit: &str) -> HyperindexResult<BaseSnapshot>`
- [overlays.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/overlays.rs)
  - `build_working_tree_overlay(repo_root: &Path, git_state: &GitRepoState) -> HyperindexResult<WorkingTreeOverlay>`
  - `build_buffer_overlays(buffers: &[BufferContents]) -> HyperindexResult<Vec<BufferOverlay>>`
- [manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
  - `SnapshotAssembler::compose(...) -> HyperindexResult<ComposedSnapshot>`
  - `SnapshotAssembler::resolve_file(snapshot: &ComposedSnapshot, path: &str) -> Option<ResolvedFile>`
  - `SnapshotAssembler::diff(left, right) -> SnapshotDiffResponse`
- [snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
  - stable Phase 2 types: `ComposedSnapshot`, `BaseSnapshot`, `WorkingTreeOverlay`,
    `BufferOverlay`, `SnapshotReadFileParams`, `SnapshotReadFileResponse`
- [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - `DaemonStateManager::create_snapshot(...) -> HyperindexResult<ComposedSnapshot>`

Phase 3 should plug parser/index work into `ComposedSnapshot` and `SnapshotAssembler`, not by
reaching directly into git or the working tree again. The current contract is point lookup plus
full manifest access; there is not yet a dedicated iterator/streaming file API.

### Watcher-driven refresh

The existing watcher seam is:

- [watcher.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-watcher/src/watcher.rs)
  - `WatcherService::polling(repo_root, config, ignore_patterns) -> HyperindexResult<WatcherService<PollingWatcher>>`
  - `WatcherService::watch_once(timeout) -> HyperindexResult<WatchRun>`
  - `WatchRun { backend, dropped_events, events }`
- [watch.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/watch.rs)
  - `NormalizedEvent`
  - `WatchBatch`
  - `WatcherStatus`
  - `WatchStatusParams/Response`
  - `WatchEventsParams/Response`
- [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - `attach_watcher(&RepoRecord)`
  - `detach_watcher(repo_id)`
  - `push_watch_batch(batch)`
  - `enqueue_job(..., JobKind::WatchIngest)`

Phase 3 should keep the watcher as the producer of normalized events and add durable ingestion on
top of `WatchBatch`, not replace the watcher model. The daemon already reserves protocol slots for
`watch_status` and `watch_events`, but those handlers are intentionally not live yet.

### Repo and git metadata access

The current repo/git seam is:

- [repos.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-repo-store/src/repos.rs)
  - `RepoStore::add_repo`
  - `RepoStore::list_repos`
  - `RepoStore::show_repo`
  - `RepoStore::remove_repo`
- [inspect.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-git-state/src/inspect.rs)
  - `GitInspector.inspect_repo(path) -> HyperindexResult<GitRepoState>`
- [repo.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/repo.rs)
  - `RepoRecord`
  - `RepoStatusResponse`
  - `WorkingTreeSummary`
- [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - `build_repo_status(repo_id) -> HyperindexResult<RepoStatusResponse>`

Phase 3 should treat `RepoRecord` plus `GitRepoState` as the source of repo identity and freshness
inputs. Do not bypass them with ad hoc repo-root probing in parser/index code.

### Daemon request/response flow

The request path that Phase 3 should extend is:

- [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
  - `DaemonRequest`
  - `RequestBody`
  - `DaemonResponse`
  - `ResponseBody`
  - `SuccessPayload`
- [client.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/client.rs)
  - `DaemonClient::send(body: RequestBody) -> HyperindexResult<SuccessPayload>`
- [server.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/server.rs)
  - `DaemonServer::handle_raw_request(raw: &[u8]) -> HyperindexResult<Vec<u8>>`
- [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
  - `HandlerRegistry::dispatch(request: DaemonRequest) -> DaemonResponse`

Phase 3 should add new runtime capabilities by:

1. defining protocol types in `hyperindex-protocol`
2. wiring one new `RequestBody` / `SuccessPayload` method
3. implementing one daemon handler in `HandlerRegistry`
4. calling the existing state/store/snapshot seams underneath
5. extending `hyperctl` through `DaemonClient::send`

That keeps stdio and Unix-socket transport behavior aligned.

## Current Tech Debt And Risks

- Watch state is not durable yet. `watch_status` and `watch_events` are part of the typed
  protocol, but the daemon returns `not_implemented`.
- Scheduler state is observable but still in-memory only. Restart preserves repo and snapshot
  metadata, not in-flight watch/scheduler progress.
- Snapshot contents are stored as UTF-8 text in manifests. That is fine for the TypeScript wedge,
  but not for arbitrary binary files.
- Base snapshot capture requires a resolvable git `HEAD` commit.
- Snapshot manifests store full file contents. This is acceptable for Phase 2, but Phase 3 should
  be deliberate about manifest size and parser/index memory behavior.
- The daemon error taxonomy is structured, but some path-level failures still collapse into
  coarse error codes rather than domain-specific Phase 3 codes.
- Unix-socket lifecycle is the intended local operator path, but this sandbox cannot exercise
  `bind()` directly; runtime smoke is covered through stdio tests instead.

## Recommended First Milestones For Phase 3

1. Implement durable watcher ingestion and cursor reads behind the already-reserved
   `watch_status` and `watch_events` protocol methods.
2. Add a parser-ready snapshot file service that builds on `ComposedSnapshot` and
   `SnapshotAssembler`, instead of letting parser/index code read the repo directly.
3. Introduce one repo-refresh pipeline that consumes `GitRepoState`, stored buffers, and watcher
   batches to produce a consistent Phase 3 input model.
4. Add one new daemon method for Phase 3 work only after the internal snapshot/repo seams are
   being used end to end; avoid widening the public surface before the internal flow is real.
5. Keep `hyperbench` untouched and integrate any future engine behavior through the existing Phase
   1 adapter seam rather than moving benchmark logic into the daemon.
