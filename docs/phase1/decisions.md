# Repo Hyperindex Phase 1 Decisions

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
