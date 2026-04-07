# Repo Hyperindex Agent Guide

## Phase 1 Scope

Phase 1 is limited to the evaluation backbone for Repo Hyperindex. Work in this repository should currently focus on:

- the benchmark and evaluation harness
- corpora manifests and bootstrap tooling
- a deterministic synthetic TypeScript monorepo generator
- query packs and goldens
- reporting, compare flows, and CI-oriented harness support
- durable docs that keep implementation incremental and reviewable

The source of truth for scope is [docs/phase1/execution-plan.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/execution-plan.md).

## Hard Non-Goals

Do not implement any of the following during Phase 1:

- the Hyperindex daemon
- a production indexer
- a production query engine
- a VS Code extension
- a UI beyond machine-readable harness output
- a cloud or multi-user service
- engine-specific behavior that couples the harness to one backend
- real corpora ingestion logic unless a task explicitly introduces it

## Build, Test, and Lint Commands

Run these from the repo root:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv sync
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench --help
UV_CACHE_DIR=/tmp/uv-cache uv run ruff check .
UV_CACHE_DIR=/tmp/uv-cache uv run pytest
```

Use the smallest relevant validation set for each task, then record what you ran in [docs/phase1/status.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/status.md).

## Repo Conventions For The Harness

- Treat [docs/phase1/execution-plan.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/execution-plan.md) as the Phase 1 contract.
- Keep the harness engine-agnostic. Define interfaces and schemas, not real engine behavior.
- Prefer Python 3.12 with `uv`, `pytest`, and `ruff`.
- Keep outputs deterministic and diff-friendly.
- Preserve the product wedge: TypeScript local impact engine.
- Preserve the future hero query path: "where do we invalidate sessions?"
- Favor small, reviewable diffs over broad scaffolding.
- Put Phase 1 harness code under `bench/` unless a later documented decision changes that.
- Do not add benchmark logic, engine code, or real corpora code unless the active task explicitly requires it.

## What Done Means For Codex On This Repo

For any task, Codex is done only when:

1. The requested repo changes are implemented, not just described.
2. The scope stays inside Phase 1 unless the user explicitly changes scope.
3. The smallest relevant validation commands have been run.
4. Results of those commands are reported back clearly.
5. [docs/phase1/status.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/status.md) is updated with completed work, decisions, commands run, remaining risks/TODOs, and the next recommended prompt.
6. Any deviation from the execution plan is documented in [docs/phase1/decisions.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/decisions.md).

## Status Update Requirement

After each meaningful task, update [docs/phase1/status.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/status.md) with:

- what was completed
- key decisions
- commands run
- remaining risks / TODOs
- the next recommended prompt
