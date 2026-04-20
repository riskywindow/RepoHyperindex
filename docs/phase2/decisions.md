# Repo Hyperindex Phase 2 Decisions

## 2026-04-09 - Phase 4 parser and symbol services integrate through the existing Phase 2 daemon/runtime

### Decision

Keep the existing Phase 2 daemon, protocol envelope, snapshot model, and `hyperctl` transport as
the single local runtime boundary, and wire the real parser/symbol build/query services through
that boundary instead of creating a parallel Phase 4 control plane.

### Why

- The runtime spine from Phase 2 is already the source of truth for repo identity, snapshots,
  buffer overlays, daemon lifecycle, and local observability.
- Parser and symbol work needs those exact repo/snapshot/buffer inputs, so a second service
  boundary would duplicate state and make local freshness harder to reason about.
- The user path for Phase 4 is local-only and machine-consumable, so promoting JSON-first daemon
  and CLI flows is more valuable than preserving the old parser-free Phase 2 daemon boundary.

### Consequence

- `hyperd` and `hyperctl` now own parser and symbol build/status/query flows in addition to the
  original Phase 2 repo/snapshot/buffer lifecycle.
- Phase 2 status and protocol docs must call out this intentional scope expansion instead of
  pretending the daemon remains parser-free.
- Phase 4 implementation work should keep building on the current local daemon/runtime boundary
  rather than introducing a separate transport or UI surface.

## 2026-04-07 - Phase 2 runtime remains separate from the Phase 1 harness

### Decision

Treat `bench/` as the stable Phase 1 benchmark/eval layer and land Phase 2 runtime code in a
dedicated Rust workspace outside `bench/`.

### Why

- Phase 1 already established a clean harness boundary and stable public commands.
- Phase 2 needs runtime/daemon code, not a harness rewrite.
- Keeping the layers separate lowers regression risk for benchmark outputs and CI smoke flows.

### Consequence

- New runtime crates should live under `crates/`.
- The `hyperbench` CLI and its artifact contracts remain owned by the Python harness.
- Phase 2 product/runtime work must integrate through the existing adapter seam rather than by
  moving benchmark logic into Rust.

## 2026-04-07 - Phase 2 uses a transport-neutral versioned JSON protocol

### Decision

Define one canonical local protocol as transport-neutral, versioned JSON request/response
messages with typed Rust models.

The first transport should remain compatible with stdio for CLI smoke paths and harness
integration. A local Unix domain socket transport can be layered on for the long-running daemon
without changing the message schema.

### Why

- Phase 2 needs a versioned local protocol, but the final transport shape should not force
  HTTP or another network-shaped API.
- Stdio compatibility keeps the future shell adapter path straightforward.
- A transport-neutral schema lets the CLI, daemon, and future local clients share one contract.

### Consequence

- Protocol versioning lives in shared Rust types, not in transport-specific code.
- CLI smoke paths and daemon requests should serialize the same logical message shapes.
- HTTP is not the default control surface for Phase 2.

## 2026-04-07 - Phase 2 uses hybrid local storage

### Decision

Use a hybrid local storage model:

- TOML for operator-edited config
- SQLite for mutable runtime state such as repo registry, scheduler jobs, and event metadata
- JSON files for immutable snapshot manifests and debug artifacts

### Why

- Mutable daemon control-plane state benefits from transactions and simple local queries.
- Immutable manifests are easier to inspect and diff as JSON files.
- This split stays simple and testable without introducing a separate service dependency.

### Consequence

- Phase 2 storage work should define a small migration story for SQLite.
- Snapshot manifests remain easy to inspect by hand.
- Flat JSON files alone are not the target architecture for scheduler/event persistence.

## 2026-04-07 - Initial runtime scaffold maps the plan into ten Rust crates

### Decision

Implement the first Phase 2 runtime scaffold as a Cargo workspace under `crates/` with these
packages:

- `hyperindex-core`
- `hyperindex-protocol`
- `hyperindex-config`
- `hyperindex-repo-store`
- `hyperindex-watcher`
- `hyperindex-git-state`
- `hyperindex-snapshot`
- `hyperindex-scheduler`
- `hyperindex-daemon`
- `hyperindex-cli`

Ship two initial binaries:

- `hyperd`
- `hyperctl`

### Why

- This follows the crate boundaries in
  [execution-plan.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase2/execution-plan.md)
  closely enough to keep future work incremental.
- It gives each Phase 2 concern a stable compile target without forcing early implementation of
  daemon handlers, watcher ingestion, git inspection, or snapshot assembly.
- It keeps the first slice intentionally reviewable: typed contracts and bootstrapping skeletons
  first, behavior later.

### Consequence

- The runtime now has a dedicated subtree with its own
  [AGENTS.override.md](/Users/rishivinodkumar/RepoHyperindex/crates/AGENTS.override.md).
- `hyperd` and `hyperctl` compile and expose stub command surfaces, but they do not yet provide
  real protocol handling or repo operations.
- The next implementation slices should fill in crate internals rather than reshaping the
  workspace again.

## 2026-04-07 - The public daemon contract uses one envelope and a small snake_case method surface

### Decision

Represent the public local daemon API with one versioned request envelope and one versioned
response envelope.

Method ids are explicit snake_case values:

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
- `buffers_set`
- `buffers_clear`
- `buffers_list`
- `shutdown`

### Why

- A single envelope keeps CLI, daemon, and future stdio bridge behavior aligned.
- Explicit method ids are easier to fixture, test, and handle than nested ad hoc command trees.
- This surface is large enough for Phase 2 runtime needs without widening into Phase 3 query
  intelligence.

### Consequence

- Transport code should route on method ids, not on transport-specific endpoints.
- Request and response fixtures can remain stable across stdio and local IPC transports.
- New methods should be added sparingly and only when Phase 2 scope requires them.

## 2026-04-07 - Config uses nested sections for directories, transport, registry, watcher, scheduler, logging, and ignores

### Decision

Define the public Phase 2 config contract as a nested TOML structure with these top-level
sections:

- `directories`
- `transport`
- `repo_registry`
- `watch`
- `scheduler`
- `logging`
- `ignores`

### Why

- The fields naturally cluster by runtime concern.
- Nested sections keep the operator-facing config readable as more runtime knobs are added.
- This maps cleanly to crate boundaries without forcing a flat, hard-to-evolve config schema.

### Consequence

- Runtime code should read config through typed models rather than hard-coded path conventions.
- Future config changes should prefer extending existing sections before adding top-level sprawl.
- The checked-in config fixture becomes the example source of truth for CLI and daemon behavior.

## 2026-04-07 - The repo registry uses SQLite rows plus JSON text fields keyed by canonical repo root

### Decision

Implement the Phase 2 repo registry in SQLite as one `repos` table with:

- scalar columns for stable queryable fields such as `repo_id`, canonical `repo_root`,
  `display_name`, timestamps, branch, head commit, and dirty state
- JSON text columns for flexible per-repo lists such as notes, warnings, and ignore patterns
- a unique index on canonical `repo_root`

Derive registry identity from the canonical repo root path, and normalize nested paths inside a
git repository up to the git toplevel before insertion.

### Why

- SQLite already matches the Phase 2 storage recommendation and gives atomic single-statement
  inserts and updates with straightforward temp-dir tests.
- The repo registry does not need a wider relational schema yet; notes, warnings, and ignore
  patterns are small per-repo lists that are simpler to keep in JSON text for now.
- Canonical-root uniqueness makes duplicate handling deterministic even when users add a nested
  path inside the same repository.

### Consequence

- `repos add` rejects duplicate canonical roots consistently and can report the existing `repo_id`.
- The daemon and CLI can both depend on the same library-first registry API without introducing a
  separate migration or service boundary.
- If Phase 3 ever needs to query notes or ignore patterns relationally, those fields can be
  normalized later behind the existing library API without changing the Phase 2 CLI contract.

## 2026-04-07 - Phase 2 watcher starts with a deterministic poll-based backend and debounced in-memory stream

### Decision

Implement the first concrete local file-watching backend as a poll-based watcher that:

- snapshots the watched repo tree
- diffs successive snapshots into raw filesystem changes
- normalizes those changes into `created`, `modified`, `removed`, and `renamed` events
- coalesces bursty changes through a debounced in-memory event stream

Keep the watcher reusable from library code so the future daemon and scheduler can consume the
same event stream abstraction.

### Why

- Polling is deterministic and easy to exercise with temp-directory tests on the current local
  development environment.
- It avoids introducing transport or daemon assumptions into the watch layer.
- A debounced in-memory stream gives Phase 2 a clean handoff point for future daemon/event-store
  work without forcing persistent ingestion yet.

### Consequence

- `hyperctl watch once` can already demonstrate usable normalized events without a daemon.
- Repo-level ignore patterns and safe temporary-file exclusions are enforced in the watcher layer
  before events are emitted.
- A future native OS-notification backend can be added behind the same watcher abstraction
  without changing the CLI or the normalized event model.

## 2026-04-07 - Git working-tree introspection uses porcelain v1 plus a stable digest

### Decision

Implement Phase 2 git introspection from local `git` CLI primitives:

- resolve repo root with `rev-parse --show-toplevel`
- resolve branch with `symbolic-ref --short HEAD` when available
- resolve `HEAD` commit with `rev-parse HEAD`
- derive working-tree changes from `git status --porcelain=v1 --ignored=matching --untracked-files=all --find-renames`

Parse that porcelain output into sorted file lists for:

- dirty tracked files
- untracked files
- deleted files
- renamed files
- ignored files

Compute a stable working-tree digest from `HEAD` plus those sorted lists.

### Why

- Porcelain v1 is narrow, scriptable, and stable enough for Phase 2 snapshot identity work.
- It keeps correctness and testability ahead of broader git feature coverage.
- A digest derived from sorted working-tree categories is deterministic enough to key future
  snapshot assembly without introducing parser or build awareness.

### Consequence

- `hyperctl repo status` and `hyperctl repo head` can return stable machine-readable git state now.
- Renames and ignored files are practical best-effort based on porcelain status output rather than
  a deeper history analysis.
- If future phases need richer git semantics, they should layer them behind the same git-state
  abstraction instead of widening CLI or daemon contracts ad hoc.

## 2026-04-07 - Purge must delete disk artifacts, and in-memory store state must stay isolated

### Decision

Treat `repos remove --purge-state` as a real purge of repo-local runtime state:

- delete SQLite metadata
- delete buffer rows
- delete on-disk snapshot manifest files

Also ensure `RepoStore::open_in_memory()` uses temp-backed artifact paths instead of pseudo-paths
like `:memory:/manifests`.

### Why

- If purge removes only SQLite rows, manifest reindex on the next bootstrap can silently recreate
  state that the operator explicitly deleted.
- The old in-memory path model leaked manifest and backup artifacts into the repository during
  tests, which weakens validation hygiene and obscures real worktree changes.

### Consequence

- Restart/reindex now respects purged repo state.
- Test-only store instances no longer write manifest or backup artifacts into the workspace.
- Phase 3 can rely on purge semantics matching operator intent before adding more runtime state.

## 2026-04-07 - Snapshots are composed from git-backed base files plus working-tree and buffer overlays

### Decision

Model Phase 2 snapshots as three explicit layers:

- `BaseSnapshot`
  - stable git-backed file view from a specific `HEAD` commit
- `WorkingTreeOverlay`
  - on-disk upserts and deletions relative to the base
- `BufferOverlay`
  - unsaved in-memory file contents keyed by buffer id and path

Define one `ComposedSnapshot` that combines those layers and resolves files with strict precedence:

1. buffer overlay
2. working-tree overlay
3. base snapshot

Derive snapshot identity deterministically from repo id, repo root, base digest, working-tree
digest, and sorted buffer metadata hashes.

### Why

- The explicit layer split matches the Phase 2 goal of immutable base snapshots plus on-disk and
  unsaved-editor overlays.
- It keeps snapshot behavior narrow and testable without widening into parsing or indexing.
- Deterministic snapshot ids and manifest JSON files make the resulting state easier to reason
  about, diff, and persist locally.

### Consequence

- The snapshot and buffer libraries stay reusable, but the normal `hyperctl` path should move to
  daemon-backed requests instead of direct library calls.
- Buffer overlays can win over on-disk contents in composed snapshots without saving to disk.
- Phase 2 snapshot creation currently assumes a git-backed repo with a resolvable `HEAD` commit;
  non-git snapshot bases are intentionally out of scope for this slice.

## 2026-04-07 - The first daemon runtime uses request-scoped stores plus a simple local transport split

### Decision

Implement the first usable `hyperd` runtime with:

- one in-memory `DaemonStateManager` for lifecycle state, scheduler state, watcher attachments,
  last error codes, and an in-memory watch-event queue
- request-scoped `RepoStore` opens instead of a long-lived shared SQLite connection
- a local Unix-socket server as the primary transport
- a stdio one-shot fallback path that reuses the same JSON request/response handling

### Why

- SQLite connections are simpler and safer to open per request than to share across async daemon
  tasks in this Phase 2 slice.
- The daemon still needs one owner for local runtime state that is not naturally persisted in
  SQLite yet, such as lifecycle state, connected clients, watcher attachments, and scheduler
  progress.
- A Unix socket remains the right local control-plane transport for the real daemon, while the
  stdio path keeps the transport abstraction honest and gives tests a portable fallback where
  socket binding is restricted.

### Consequence

- Protocol handlers now reuse the same repo, git, snapshot, and buffer libraries as `hyperctl`
  instead of forking daemon-only logic.
- The scheduler is intentionally in-memory and observable, not a durable distributed queue.
- Smoke tests can validate daemon request flows through the server layer even when the local test
  sandbox does not permit Unix-domain socket binding.

## 2026-04-07 - The polished Phase 2 CLI path is daemon-first, with direct-library helpers kept internal

### Decision

Use the daemon boundary as the normal operator path for:

- `hyperctl daemon start|status|stop`
- `hyperctl repos add|list|show|remove`
- `hyperctl repo status`
- `hyperctl snapshot create|show|diff|read-file`
- `hyperctl buffers set|clear|list`

Keep direct-library flows out of the public CLI path and reserve them for tests or internal helper
code only.

Treat `--json` as a first-class output mode for the machine-consumed commands above, and keep the
human output concise by default.

### Why

- Phase 2 exists to prove the daemon/runtime boundary, not just the underlying libraries.
- A daemon-first CLI gives one obvious operator path and keeps later runtime work from having to
  support two public execution models.
- JSON output needs to be predictable for smoke scripts and future automation, while human output
  still needs to stay readable in demos.

### Consequence

- `hyperctl` now talks to the daemon for the main repo, snapshot, and buffer workflows rather than
  reaching into the libraries directly.
- The daemon protocol now includes `snapshots_read_file`, so buffer-overlay precedence can be
  demonstrated through the real daemon boundary.
- Tests can still exercise the same request/response path through the stdio transport fallback
  when the local environment blocks Unix-socket binds.

## 2026-04-07 - Phase 2 runtime hardening favors local repair over opaque failure

### Decision

Harden the local Phase 2 runtime with pragmatic recovery behavior:

- treat the daemon pid file as the local runtime lockfile and automatically clear it when the
  recorded pid is no longer alive
- remove stale Unix sockets before a new daemon bind
- rebuild corrupted or missing SQLite runtime state by quarantining the broken database, restoring
  the repo registry from a JSON sidecar backup, and reindexing immutable snapshot manifests from
  disk
- expose operator maintenance commands through:
  - `hyperctl doctor`
  - `hyperctl cleanup`
  - `hyperctl reset-runtime`

### Why

- Phase 3 needs predictable local restart behavior more than it needs enterprise-grade durability.
- The repo registry and manifest metadata are small enough to back up and reindex locally without
  adding a second service or a full transactional recovery system.
- Operator mistakes such as stale sockets, stale pid files, deleted repos, or bad buffer overlay
  paths should fail clearly and offer one obvious recovery path.

### Consequence

- Repo registry state now has a lightweight JSON backup alongside SQLite so restart and
  corruption-recovery flows can preserve registered repos and `last_snapshot_id` metadata.
- Immutable snapshot manifests remain the source of truth for snapshot recovery, with SQLite acting
  as a rebuildable index.
- `hyperctl doctor` and `hyperctl cleanup` are intentionally local-only maintenance flows; they do
  not introduce remote sync, auth, or multi-user coordination into Phase 2.
