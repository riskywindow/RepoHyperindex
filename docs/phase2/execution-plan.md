# Repo Hyperindex Phase 2 Execution Plan

## Purpose

Phase 2 builds the local daemon/runtime spine for Repo Hyperindex while preserving the
Phase 0 product wedge and the shipped Phase 1 benchmark harness.

The goal of this phase is not to ship code intelligence yet. The goal is to establish the
local runtime substrate that later exact, symbol, semantic, and impact features can rely on:

- a local daemon/runtime boundary
- a versioned local protocol and config contract
- a persistent repo registry and manifest store
- normalized file-watch events
- git working-tree introspection
- immutable base snapshots plus working-tree and buffer overlays
- scheduler/status APIs and a CLI smoke path

Phase 2 must leave the Phase 1 harness usable as-is.

## Final Phase 2 Scope

Phase 2 includes only the following deliverables:

1. A Rust-stable local runtime workspace for Repo Hyperindex product code, separate from
   the Python Phase 1 harness.
2. A versioned local protocol contract for daemon/runtime requests and responses.
3. A versioned operator-facing local config contract.
4. A persistent local repo registry that can track one or more local repositories.
5. A persistent manifest store for immutable snapshot manifests and related runtime state.
6. Git working-tree introspection that can:
   - resolve repo root
   - capture the current commit
   - detect branch / detached-HEAD state
   - capture normalized working-tree status
7. File watching that emits a normalized local event stream with deterministic path handling.
8. Snapshot assembly that can represent:
   - immutable base snapshots
   - working-tree overlays
   - buffer overlays
9. A small scheduler/status layer for runtime jobs such as watch ingest and snapshot capture.
10. A CLI smoke path that proves the runtime can be configured, pointed at a repo, and asked
    for status and snapshot-oriented operations.
11. Durable docs and acceptance criteria that keep the phase reviewable and bounded.

## Phase 1 Interfaces Phase 2 Must Preserve

This audit is the minimum stable surface Phase 2 must not break.

### Public Phase 1 CLI surface

The `hyperbench` CLI in [cli.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/cli.py)
must remain stable:

- `hyperbench status`
- `hyperbench corpora validate`
- `hyperbench corpora bootstrap`
- `hyperbench corpora snapshot`
- `hyperbench corpora generate-synth`
- `hyperbench run`
- `hyperbench report`
- `hyperbench compare`

Phase 2 may add a new product CLI such as `hyperindex`, but it must not rename, remove, or
redefine the Phase 1 `hyperbench` commands.

### Harness adapter boundary

The existing adapter contract in
[adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
is the Phase 2 integration seam and must stay logically compatible:

- `prepare_corpus(bundle) -> PreparedCorpus`
- `execute_exact_query(...) -> QueryExecutionResult`
- `execute_symbol_query(...) -> QueryExecutionResult`
- `execute_semantic_query(...) -> QueryExecutionResult`
- `execute_impact_query(...) -> QueryExecutionResult`
- `run_incremental_refresh(...) -> RefreshExecutionResult`

The normalized result shapes that Phase 2 must preserve are:

- `PreparedCorpus { corpus_id, latency_ms, notes }`
- `QueryHit { path, symbol, rank, reason, score }`
- `QueryExecutionResult { query_id, query_type, latency_ms, hits, notes }`
- `RefreshExecutionResult { scenario_id, latency_ms, changed_queries, notes }`

### Harness artifacts and output names

Phase 1 run/report/compare artifact names must remain stable, as documented in
[bench/README.md](/Users/rishivinodkumar/RepoHyperindex/bench/README.md),
[how-to-run-phase1.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/how-to-run-phase1.md),
and [benchmark-spec.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/benchmark-spec.md):

`hyperbench run` writes:

- `summary.json`
- `events.jsonl`
- `metrics.jsonl`
- `query_results.csv`
- `refresh_results.csv`
- `metric_summaries.csv`

`hyperbench report` writes:

- `report.json`
- `report.md`

`hyperbench compare` writes:

- `compare.json`
- `compare.md`

Phase 2 runtime work must not repurpose these files or move responsibility for report/compare
logic out of the harness.

### Query, corpus, and golden contracts

Phase 2 must preserve the Phase 1 schema layer and deterministic evaluation assets:

- corpus manifests
- synthetic corpus configs
- query packs
- golden sets
- compare budgets
- run metadata and compare outputs

The canonical hero path from Phase 0 and Phase 1 must remain represented in artifacts and
future runtime design:

- `where do we invalidate sessions?`

### Compatibility expectations

Phase 2 may add:

- new Rust crates
- a daemon binary
- a product CLI
- a shell-facing engine binary for the harness

Phase 2 must not require:

- a rewrite of the Python harness
- a redesign of benchmark schemas
- changes to Phase 1 public output filenames
- coupling the harness to one retrieval backend beyond the existing adapter seam

## Explicit Non-Goals

Phase 2 must not implement any of the following:

- parsing or AST extraction
- exact search ranking / execution
- symbol extraction or symbol graph logic
- semantic retrieval or embedding pipelines
- impact analysis logic
- a VS Code extension
- a browser UI or dashboard
- cloud sync or any shared multi-user service
- remote daemon hosting
- production query relevance, scoring, or index freshness beyond snapshot/runtime plumbing
- any redesign of the Phase 1 harness into product runtime code

To keep the boundary sharp, Phase 2 may define protocol messages and storage records that later
phases will use, but it must not ship the Phase 3 intelligence layer early.

## Proposed Runtime File Tree

This is the target Phase 2 runtime layout. Not every file needs to be created in the first
implementation slice, but the structure should remain intentionally separate from `bench/`.

```text
docs/
  phase2/
    execution-plan.md
    acceptance.md
    status.md
    decisions.md

crates/
  hyperindex-protocol/
    src/
      lib.rs
      config.rs
      errors.rs
      repo.rs
      snapshot.rs
      status.rs
      watch.rs
  hyperindex-store/
    src/
      lib.rs
      db.rs
      migrations.rs
      repos.rs
      manifests.rs
      events.rs
      jobs.rs
  hyperindex-git/
    src/
      lib.rs
      inspect.rs
      status.rs
      paths.rs
  hyperindex-watch/
    src/
      lib.rs
      watcher.rs
      normalize.rs
  hyperindex-snapshot/
    src/
      lib.rs
      base.rs
      overlays.rs
      manifest.rs
  hyperindex-scheduler/
    src/
      lib.rs
      queue.rs
      jobs.rs
      status.rs
  hyperindex-daemon/
    src/
      main.rs
      server.rs
      handlers.rs
      runtime.rs
  hyperindex-cli/
    src/
      main.rs
      commands/
        config.rs
        repo.rs
        snapshot.rs
        status.rs
        watch.rs

bench/
  ...
```

This tree preserves a clear separation:

- `bench/` remains the Phase 1 evaluation layer
- `crates/` becomes the Phase 2 product/runtime layer

## Toolchain Choice And Rationale

### Final toolchain

- Rust stable for all Phase 2 runtime code
- Cargo workspace for crate composition and testing
- `serde` and `serde_json` for typed protocol payloads
- `toml` for config serialization
- `clap` for CLI entry points
- `tokio` for daemon, transport, and scheduler orchestration
- `notify` for file watching
- `rusqlite` for local persistent control-plane state
- `tempfile` and normal Rust unit/integration tests for deterministic local validation

### Rationale

- The prompt explicitly prefers Rust stable for Phase 2 runtime code.
- The repo already has a strong Phase 1 convention: Python plus `uv` for the harness. Keeping
  Phase 2 in Rust preserves a clean language split between evaluation and runtime.
- Rust is a good fit for a local daemon, file watching, typed protocol structures, and durable
  local state with predictable performance.
- `tokio` is justified in Phase 2 because daemon transport, watch ingestion, and scheduler
  coordination are naturally asynchronous concerns.
- `rusqlite` keeps storage simple, typed, local-first, and testable without introducing a
  separate database service.

## Crate / Module Boundaries

### `hyperindex-protocol`

Responsibility:

- versioned config contract
- versioned request / response envelopes
- typed repo, snapshot, watch, and status models
- typed error categories for local protocol failures

Must not contain:

- git execution
- file watching
- SQLite access
- scheduler execution logic

### `hyperindex-store`

Responsibility:

- schema migrations
- repo registry persistence
- manifest index persistence
- normalized event stream persistence
- scheduler job persistence

Must not contain:

- transport code
- CLI argument parsing
- query-engine behavior

### `hyperindex-git`

Responsibility:

- repo discovery
- commit and branch inspection
- normalized working-tree status collection
- repo-relative path normalization helpers

Must not contain:

- watch logic
- snapshot composition
- storage concerns beyond returned typed records

### `hyperindex-watch`

Responsibility:

- file-system watcher setup
- conversion from backend watcher events into normalized runtime events
- deterministic event ordering rules for tests

Must not contain:

- scheduler policy
- git introspection
- parsing or indexing behavior

### `hyperindex-snapshot`

Responsibility:

- base snapshot identities
- working-tree overlay records
- buffer overlay records
- immutable manifest assembly and validation

Must not contain:

- watch backend integration
- daemon transport
- query behavior

### `hyperindex-scheduler`

Responsibility:

- queue model
- repo-scoped job coalescing
- status reporting
- lightweight execution policy for runtime maintenance jobs

Must not contain:

- query execution
- report/compare logic
- benchmark scoring logic

### `hyperindex-daemon`

Responsibility:

- daemon process entry point
- local transport listener
- request dispatch
- coordination between store, watch, git, snapshot, and scheduler crates

Must not contain:

- benchmark harness code
- Phase 3 engine behavior

### `hyperindex-cli`

Responsibility:

- operator-facing CLI commands
- local smoke path
- daemon startup / connection ergonomics
- JSON output for scripting

Must not contain:

- direct benchmark logic
- schema scoring or compare behavior

## Protocol Choice Candidates And Final Recommendation

### Candidate A: JSON over stdio subprocess boundary

Description:

- a single Rust binary invoked by CLI or harness
- JSON request/response on stdin/stdout

Pros:

- simplest initial harness integration
- no socket lifecycle
- easy local testability

Cons:

- weak fit for a long-running daemon
- awkward for watch subscriptions and background runtime status
- harder to separate daemon lifecycle from command lifecycle

### Candidate B: Local HTTP/JSON on loopback

Description:

- daemon listens on `127.0.0.1`
- CLI speaks HTTP with JSON payloads

Pros:

- familiar request model
- easy manual inspection with `curl`

Cons:

- broader surface than needed for a local-only runtime
- unnecessary HTTP semantics for an internal daemon
- higher risk of accidental future API sprawl

### Candidate C: Transport-agnostic versioned JSON messages with local IPC transport

Description:

- canonical typed request/response envelopes defined in Rust
- JSON serialization as the message format
- first transport is stdio for smoke tests and harness-facing integration
- daemon transport is a local Unix domain socket on macOS/Linux

Pros:

- keeps the message contract stable while allowing transport evolution
- local-first and not network-shaped by default
- easy to unit test in-process and easy to adapt to the harness shell boundary
- preserves a clean path from CLI smoke flows to a real daemon

Cons:

- slightly more upfront design work than raw CLI stdout
- requires explicit framing rules

### Final recommendation

Choose Candidate C.

Phase 2 should define a transport-neutral, versioned JSON protocol with typed Rust models.
The canonical protocol contract should not depend on HTTP. It should support:

- request envelope with `protocol_version`, `request_id`, `method`, and typed `params`
- response envelope with `protocol_version`, `request_id`, `ok`, typed `result`, and typed `error`
- explicit method families for:
  - config
  - repo registry
  - snapshot capture
  - watch/status
  - scheduler status

Transport plan:

- Milestone 1: stdio transport for local smoke tests and the eventual harness bridge
- Milestone 2: local Unix domain socket transport for the daemon on macOS/Linux

This keeps the shell adapter path viable without forcing the benchmark harness to know about
daemon internals.

## Storage Choice Candidates And Final Recommendation

### Candidate A: Flat JSON/TOML files only

Description:

- TOML config
- JSON repo registry
- JSON manifest store
- JSON event log

Pros:

- very simple to inspect by hand
- diff-friendly
- low initial complexity

Cons:

- scheduler queue and event-stream queries become clumsy
- atomic multi-record updates are awkward
- deduplication, leases, and status transitions are harder to reason about

### Candidate B: SQLite for mutable control-plane state plus JSON manifest files

Description:

- TOML for operator config
- SQLite for repo registry, manifest index, watch cursor, event log metadata, and scheduler jobs
- JSON manifest files on disk for immutable snapshot records

Pros:

- transactional updates for repo/scheduler state
- simple local deployment story
- easy test isolation
- immutable manifests remain human-inspectable and diff-friendly
- scales better than flat files for event streams and job status

Cons:

- requires migrations and a small storage abstraction
- mixes two storage forms

### Candidate C: Embedded KV store such as `redb` / `sled`

Description:

- a Rust-native embedded KV store holds all runtime state

Pros:

- fast local writes
- single-language dependency stack

Cons:

- weaker inspection ergonomics than SQLite
- less familiar operational tooling
- more custom schema discipline required

### Final recommendation

Choose Candidate B.

The Phase 2 target storage design should be:

- TOML config for operator-edited settings
- SQLite as the source of truth for mutable runtime state
- JSON manifest files for immutable snapshots and exported debug artifacts

Recommended SQLite responsibility split:

- `repos`
- `snapshot_manifests`
- `watch_events`
- `scheduler_jobs`
- `scheduler_job_history`

Recommended filesystem responsibility split:

- immutable snapshot manifest JSON
- optional debug exports used by smoke tests or operator inspection

This hybrid design keeps control-plane state transactional while preserving the diff-friendly
artifact style already established in Phase 1.

## Snapshot Invariants

Phase 2 snapshot handling must obey the following invariants:

1. A base snapshot is immutable and anchored to one repo id plus one git commit.
2. A working-tree overlay never mutates the base snapshot; it records deltas relative to it.
3. A buffer overlay never mutates the base snapshot or working-tree overlay; it sits on top as
   the highest-precedence layer.
4. Overlay application order is always:
   - base snapshot
   - working-tree overlay
   - buffer overlays
5. Snapshot manifests are immutable once written. Any state change writes a new manifest.
6. The same logical inputs must yield the same manifest identity and field ordering where
   reasonable.
7. All paths stored in manifests are normalized relative to repo root and use `/`.
8. Buffer overlays store versioned editor state metadata and content hashes, not parsing output.
9. Snapshot records must not contain search results, symbol tables, embeddings, or impact edges
   in Phase 2.
10. Snapshot creation must not depend on network access.

## Scheduler Model

Phase 2 needs a small scheduler, not a general job platform.

### Final model

- single local daemon process
- repo-scoped serialized work
- bounded in-memory execution plus durable persisted job state
- FIFO by enqueue time, with repo-level coalescing for redundant watch-driven refresh work

### Job families in scope

- repo registration / refresh
- watch event ingest
- snapshot capture
- working-tree reconciliation
- manifest finalization

### Status model

Every job should have one of these states:

- `pending`
- `running`
- `succeeded`
- `failed`
- `cancelled`

Every repo should expose summarized scheduler status:

- watcher attached / not attached
- last successful snapshot id
- queue depth
- active job kind
- last error summary, if any

### What the scheduler must not do in Phase 2

- query execution
- ranking or retrieval work
- background parsing
- semantic indexing
- impact analysis

## Validation Matrix

The table below defines the expected validation surface for Phase 2 implementation work.

| Area | Smallest relevant validation | What it proves |
| --- | --- | --- |
| Protocol types | `cargo test -p hyperindex-protocol` | versioning, request/response shapes, error serialization |
| Config | `cargo test -p hyperindex-protocol config` | config parsing, defaults, version gating |
| Storage | `cargo test -p hyperindex-store` | migrations, repo persistence, manifest index writes, job transitions |
| Git introspection | `cargo test -p hyperindex-git` | repo discovery, status normalization, detached-HEAD handling |
| Watch normalization | `cargo test -p hyperindex-watch` | deterministic event mapping and path normalization |
| Snapshot assembly | `cargo test -p hyperindex-snapshot` | base/overlay composition and manifest invariants |
| Scheduler | `cargo test -p hyperindex-scheduler` | queueing, coalescing, and status transitions |
| CLI smoke path | `cargo run -p hyperindex-cli -- status` plus one repo/snapshot smoke flow | end-to-end operator path works locally |
| Daemon transport | targeted integration test for local IPC | daemon accepts typed requests and returns typed responses |
| Harness preservation | `UV_CACHE_DIR=/tmp/uv-cache uv run pytest tests/test_cli.py tests/test_runner.py` | public Phase 1 CLI and runner contracts still hold |
| Phase 1 smoke | `bash bench/scripts/ci-smoke.sh` before major integration merges | harness still runs end to end |

For docs-only changes, content review plus `git diff --check` is sufficient.

## Risks And Mitigations

### Risk: Phase 2 silently expands into Phase 3 engine work

Mitigation:

- keep parsing, search, symbol extraction, semantic retrieval, and impact logic explicitly out of scope
- require acceptance against [acceptance.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase2/acceptance.md)

### Risk: The daemon protocol diverges from the harness shell boundary

Mitigation:

- define one canonical JSON protocol model
- keep stdio support viable from the beginning
- localize adapter-specific serialization to the boundary layer

### Risk: Storage becomes over-engineered before product behavior exists

Mitigation:

- use SQLite only for mutable runtime state
- keep immutable manifests as plain JSON
- defer blob stores, compaction strategies, and advanced cache layers

### Risk: Watch events become nondeterministic or platform-fragile

Mitigation:

- normalize paths and event kinds immediately
- record sequence ids at ingest time
- prefer deterministic tests built around synthetic event fixtures

### Risk: Snapshot semantics become ambiguous before query features exist

Mitigation:

- adopt explicit snapshot invariants now
- keep manifests metadata-first and content-addressable where needed
- document overlay precedence before parser/index code exists

### Risk: Phase 1 harness stability regresses while Phase 2 code lands

Mitigation:

- preserve `hyperbench` commands and outputs
- keep product runtime code out of `bench/`
- run targeted Phase 1 tests on every Phase 2 milestone

## Definition Of Done

Phase 2 is done only when all of the following are true:

1. A Rust-based local runtime/daemon spine exists separately from the Phase 1 harness.
2. A versioned local protocol and config contract are implemented and documented.
3. A persistent repo registry and manifest store exist with clear ownership between mutable
   state and immutable artifacts.
4. File watching emits a normalized event stream suitable for later indexing phases.
5. Git working-tree introspection is implemented and tested.
6. Immutable base snapshots plus working-tree and buffer overlays are represented with clear
   invariants.
7. Scheduler and status APIs exist and are reachable from a CLI smoke path.
8. The Phase 1 harness public commands, schemas, and artifact contracts still work.
9. The implementation still does not ship parsing, exact search, symbol extraction, semantic
   retrieval, impact analysis, extension logic, or cloud sync.
10. [status.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase2/status.md),
    [decisions.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase2/decisions.md),
    and [acceptance.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase2/acceptance.md)
    reflect the final state and remaining risks.

## Assumptions That Do Not Require User Input Right Now

- Phase 2 should continue to target local-first macOS and Linux usage only.
- The product wedge remains a local-first TypeScript impact engine, even though Phase 2 does
  not yet implement impact logic.
- The Phase 1 harness remains the benchmark/eval source of truth and should not be folded into
  the runtime crates.
- A local daemon is warranted in Phase 2, but its initial operator surface should remain CLI
  plus docs, not UI.
- The first transport must remain compatible with a shell/subprocess bridge so the existing
  Python harness can integrate later without a redesign.
- Mutable runtime state will benefit from SQLite before Phase 3 begins, while immutable snapshot
  artifacts should stay JSON for inspection and debugging.
- The runtime can remain language-agnostic at the protocol/store level in Phase 2, even though
  the long-term product wedge is TypeScript-first.
