# Repo Hyperindex Phase 1 Decisions

## 2026-04-19 - Allow incremental semantic refresh over snapshot diffs without changing Phase 1 artifacts

### Decision

Permit the explicit user-requested Phase 6 incremental semantic refresh work in the runtime
crates as long as it remains additive to the checked-in Phase 1 harness contract.

### Why

- The semantic runtime already had real chunking, embedding, vector retrieval, and reranking
  paths, so the next requested correctness slice was changed-file refresh rather than a new public
  benchmark artifact shape.
- Phase 2 snapshot diffs and Phase 4 persisted symbol facts already provide the right seams for a
  debuggable incremental implementation without inventing semantic-only watcher machinery.
- The benchmark harness still lacks the real `daemon-semantic` adapter, so the runtime can grow
  refresh metadata and reuse behavior first while `bench/` stays stable.

### Consequence

- Phase 6 semantic builds may now reuse unchanged chunk/vector state, rebuild only touched files,
  and expose refresh stats plus fallback reasons under `crates/` and `docs/phase6/` while `bench/`
  schemas and Phase 1 run artifacts remain unchanged.
- Full rebuild remains the compatibility fallback whenever the prior semantic state is stale,
  incompatible, or corrupted.

## 2026-04-19 - Allow additive semantic hybrid reranking and explanation fields without changing Phase 1 artifacts

### Decision

Permit the explicit user-requested Phase 6 semantic hybrid reranking work in the runtime crates as
long as it remains additive to the checked-in Phase 1 harness contract.

### Why

- The runtime already has a real semantic retrieval path over persisted chunk/vector rows, so the
  next requested quality slice is deterministic reranking rather than new transport or harness
  shape.
- The checked-in Phase 1 harness still normalizes semantic results to `QueryHit`, so richer
  explanation and rerank metadata must stay additive on the Rust semantic contract until the real
  `daemon-semantic` adapter lands.

### Consequence

- Phase 6 semantic query execution may add transparent rerank signals and explanation payloads
  under `crates/` and `docs/phase6/` while `bench/` schemas, run artifacts, and compare flows
  remain unchanged.
- The next Phase 1-facing semantic step is still daemon-backed harness execution of the checked-in
  semantic pack rather than a harness contract rewrite.

## 2026-04-13 - Allow Phase 6 semantic chunk materialization without changing Phase 1 artifacts

### Decision

Permit the explicit user-requested Phase 6 semantic chunk extraction work in the runtime crates as
long as it remains additive to the checked-in Phase 1 harness contract.

### Why

- The active repo instructions default work to Phase 1, but this task explicitly requested Phase 6
  semantic chunk materialization.
- The runtime can grow real semantic chunk storage and inspect tooling without forcing benchmark
  artifact changes before the `daemon-semantic` adapter exists.

### Consequence

- Phase 6 semantic chunking, chunk persistence, and inspect tooling may land under `crates/` and
  `docs/phase6/` while `bench/` and Phase 1 artifact schemas remain unchanged.
- The next Phase 1-facing semantic step is still the dedicated `daemon-semantic` adapter rather
  than a harness contract rewrite.

## 2026-04-13 - Add a dedicated daemon-backed impact adapter without changing Phase 1 artifacts

### Decision

Keep the existing `--adapter daemon` symbol path intact and add a separate `--adapter
daemon-impact` path for Phase 5 impact-query benchmarking.

### Why

- The real Phase 5 engine is ready behind the daemon protocol, but its lifecycle and metrics are
  different enough from symbol benchmarking that overloading the symbol adapter would make
  operator output ambiguous.
- The checked-in Phase 1 impact pack still includes one `config_key` query while the current public
  daemon contract exposes only `symbol` and `file` targets. The adapter can bridge that gap
  backward-compatibly by degrading those queries to their backing file path without changing the
  Phase 1 query schema.
- Adding impact-specific metadata as additive fields keeps run/report/compare artifacts stable
  while still making fixture-vs-real, cold-vs-warm, and full-vs-incremental comparisons
  machine-readable.

### Consequence

- `hyperbench run --adapter daemon-impact` now benchmarks the real Phase 5 impact engine through
  the daemon protocol.
- `summary.json`, `metrics.jsonl`, `metric_summaries.csv`, and `refresh_results.csv` may now carry
  additive impact fields such as `prepare-impact-analyze-latency`, `impact_refresh_mode`, and
  persisted-build refresh stats.
- Existing fixture and daemon-symbol flows remain valid and unchanged.

## 2026-04-06 - Harness scaffold lives under `bench/`

### Decision

Scaffold the Phase 1 harness under `bench/` with the Python package at `bench/hyperbench/`.

### Why

- The active task explicitly requested scaffolding under `bench/` unless the repo already had a cleaner convention.
- The repository did not have an existing code layout, so there was no stronger convention to preserve.
- Using `bench/` keeps the Phase 1 harness visibly separated from future product/runtime code.

### Consequence

- This intentionally differs from the earlier proposed `src/hyperindex_eval/` planning tree in [docs/phase1/execution-plan.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/execution-plan.md).
- Future planning and implementation should treat `bench/` as the canonical home for the Phase 1 harness unless a later decision supersedes it.

## 2026-04-09 - Extend the harness contract backward-compatibly for real symbol benchmarking

### Decision

Keep the existing Phase 1 runner, query, and compare flow intact, but extend the adapter and
artifact surface in backward-compatible ways so the real Phase 4 symbol engine can participate.

### Why

- The existing `EngineAdapter` boundary was already narrow enough for a real symbol adapter.
- The missing piece was operational metadata, not a contract redesign:
  cold vs warm build behavior and incremental refresh mode needed to be machine-readable.
- Some restricted environments block Unix-domain socket binds, so the daemon protocol needed a
  stdio transport fallback to stay runnable without changing the harness UX.

### Consequence

- `PreparedCorpus` and `RefreshExecutionResult` now allow optional metadata and metric rows.
- `summary.json`, `events.jsonl`, `metrics.jsonl`, and `refresh_results.csv` may now include
  daemon-symbol build and refresh details while staying compatible with older fixture runs.
- The real symbol adapter remains scoped to symbol-query benchmarking only; exact, semantic, and
  impact adapters are still separate future slices.
