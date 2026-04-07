# Repo Hyperindex Phase 1 Handoff

## What Phase 1 Built

Phase 1 delivered the evaluation backbone for Repo Hyperindex without shipping any real
engine implementation.

Built in this phase:

- a typed, engine-agnostic benchmark harness under [bench/hyperbench](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench)
- strongly validated YAML and JSON schemas for corpora, query packs, goldens, budgets, and compare outputs
- deterministic synthetic TypeScript monorepo generation with seeded output and hero-query coverage for
  `where do we invalidate sessions?`
- real-corpus selection artifacts, dry-run bootstrap planning, and snapshot metadata
- deterministic synthetic query packs and goldens meeting the Phase 1 target counts
- a runnable `FixtureAdapter` that lets the harness execute end to end today
- a placeholder `ShellAdapter` boundary for the future Rust engine
- machine-readable run outputs plus report and compare artifacts
- smoke CI coverage and operator-facing docs

## Intentionally Out Of Scope

Phase 1 did not build:

- the Hyperindex daemon
- a production indexer
- a production exact, symbol, semantic, or impact query engine
- a VS Code extension
- a UI or cloud service
- real-repo benchmark execution in CI
- any engine-specific coupling that would force the harness to one backend

The harness is meant to evaluate engines, not to be one.

## What The First Rust Engine Adapter Should Implement In Phase 2

The first real adapter should satisfy the existing boundary in
[adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
without forcing a harness redesign.

Minimum Phase 2 adapter responsibilities:

1. `prepare_corpus(bundle) -> PreparedCorpus`
   - create or refresh any engine-local index state for the selected bundle
   - return preparation latency and operator-facing notes
2. `execute_exact_query(...) -> QueryExecutionResult`
   - return normalized hits with stable path, optional symbol, rank, reason, and score
3. `execute_symbol_query(...) -> QueryExecutionResult`
4. `execute_semantic_query(...) -> QueryExecutionResult`
5. `execute_impact_query(...) -> QueryExecutionResult`
6. `run_incremental_refresh(...) -> RefreshExecutionResult`
   - apply the edit scenario, refresh engine state, and report the changed query ids

Recommended Phase 2 implementation rules:

- preserve deterministic ordering of returned hits where practical
- normalize paths relative to the corpus bundle so results compare cleanly across environments
- return clear adapter errors instead of raw subprocess failures
- keep adapter-specific serialization localized to the adapter layer
- avoid embedding report, compare, or budget logic in the engine itself

If the Rust engine is invoked as a subprocess first, the shell-facing contract should map
cleanly into the existing `QueryExecutionResult` and `RefreshExecutionResult` types. A direct
Python FFI or RPC integration can come later if needed.

## Current Risks And Tech Debt

- The `ShellAdapter` is still a placeholder, so the Phase 2 engine integration path is defined but not exercised.
- The fixture path proves harness correctness, but it does not simulate retrieval mistakes, partial matches, or ranking noise.
- Run summaries are stable and machine-readable, but there is not yet a dedicated typed schema for every emitted JSONL record.
- Real-corpus execution remains manual until repo pins are filled in and curated seed packs are replaced with verified expectations.
- The harness is self-validating for bundle inputs, but real engine normalization rules will still need careful path and symbol handling in Phase 2.

## Recommended Next Milestones

### Milestone 1: Rust adapter contract bring-up

- implement the first runnable Rust-engine-backed adapter behind the current boundary
- prove `prepare_corpus`, one query type, and clear adapter error handling

### Milestone 2: Exact and symbol parity

- make exact and symbol queries pass against the synthetic corpus goldens
- validate normalized path and symbol output contracts

### Milestone 3: Incremental refresh path

- wire edit scenarios into the real engine adapter
- prove the hero path around session invalidation and changed-query reporting

### Milestone 4: Semantic and impact evaluation

- add the engine’s semantic and impact query support to the same harness
- start collecting real quality deltas against the existing fixture baseline

### Milestone 5: Real-corpus execution hardening

- pin real repos
- bootstrap them locally
- replace placeholder seed packs with verified curated expectations
- compare synthetic and real-corpus behavior through the same report/compare flow

## Phase 2 Starting Point

If a new engineer is starting Phase 2, begin with these files:

- [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
- [runner.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/runner.py)
- [schemas.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/schemas.py)
- [benchmark-spec.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/benchmark-spec.md)
- [how-to-run-phase1.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/how-to-run-phase1.md)

Those files define the contract the Rust engine should plug into, the artifacts it must
produce, and the fastest smoke path to verify the integration.
