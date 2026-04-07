# Repo Hyperindex Phase 1 Execution Plan

## Purpose

Phase 1 establishes the evaluation backbone for Repo Hyperindex before we build product engines. The goal is to create a deterministic, engine-agnostic harness that can evaluate future implementations of the TypeScript local impact engine wedge against stable corpora, query packs, and goldens.

This phase is intentionally about measurement and fixtures, not product runtime.

## Final Phase 1 Scope

Phase 1 includes only the following deliverables:

1. A benchmark and evaluation harness that can run repeatable query evaluations against any compliant engine implementation.
2. Corpora manifests and bootstrap tooling for both curated external corpora and locally generated synthetic corpora.
3. A deterministic synthetic TypeScript monorepo generator designed to preserve the future hero path: "where do we invalidate sessions?"
4. Query packs and golden expectations for exact, symbol, semantic, and impact-style evaluation inputs.
5. Reporting, comparison, and CI-friendly outputs for benchmark runs.
6. Durable planning and status docs that let implementation proceed in small, reviewable slices.

## Explicit Non-Goals

Phase 1 must not implement:

- The Hyperindex daemon
- Any production indexer
- Any production query engine
- Any VS Code extension
- Any UI or dashboard beyond machine-readable reports
- Any cloud or multi-user service
- Any engine-specific logic that assumes a single retrieval/indexing backend
- Any non-TypeScript product wedge beyond fixture coverage needed for the harness

To keep the boundary sharp, Phase 1 may define engine interfaces and synthetic expectations, but it must not ship an actual Hyperindex engine.

## Phase 1 Design Principles

- Preserve the Phase 0 wedge: TypeScript local impact engine.
- Preserve the future hero query path: "where do we invalidate sessions?"
- Keep the harness engine-agnostic so multiple implementations can be compared later.
- Prefer deterministic outputs, seeded generation, stable manifests, and normalized reports.
- Favor small, reviewable diffs over broad scaffolding.
- Optimize for local execution first, CI second.

## Toolchain Choice

### Proposed toolchain

- Python 3.12
- `uv` for environment and dependency management
- `pytest` for tests
- `ruff` for linting and formatting

### Rationale

The repository currently has no stronger established conventions beyond the Phase 0 planning document. Python 3.12 plus `uv`/`pytest`/`ruff` is the fastest path to a deterministic harness with strong local ergonomics, simple file I/O, stable JSON handling, and easy CI integration. It also keeps the evaluation layer separate from any later production language choice for the engine itself.

### Adaptation rule

If later repository-wide conventions emerge that are stronger than this proposal, the harness may adapt, but only if:

- determinism is preserved,
- local developer setup stays lightweight,
- and the harness remains engine-agnostic.

## Proposed File Tree

This is the target Phase 1 structure. Not every file needs to be created in the first implementation slice.

```text
docs/
  phase1/
    execution-plan.md
    status.md

src/
  hyperindex_eval/
    __init__.py
    cli.py
    models.py
    io.py
    normalization.py
    reporting.py
    compare.py
    engine_contract.py
    harness/
      __init__.py
      runner.py
      scoring.py
      timings.py
    corpora/
      __init__.py
      manifest.py
      bootstrap.py
    synthetic/
      __init__.py
      generator.py
      templates.py
      seeds.py
    query_packs/
      __init__.py
      loader.py
      validators.py

fixtures/
  corpora/
    manifests/
      synthetic-saas-small.json
      synthetic-saas-medium.json
    query-packs/
      hero-sessions.json
      ts-local-impact-core.json
    goldens/
      synthetic-saas-small.json
      synthetic-saas-medium.json

tests/
  test_engine_contract.py
  test_manifest_loader.py
  test_synthetic_generator.py
  test_query_pack_loader.py
  test_reporting.py
  test_compare.py

scripts/
  phase1_bootstrap.py
  phase1_generate_synthetic.py
  phase1_run_eval.py
  phase1_compare_runs.py

.github/
  workflows/
    phase1-harness.yml

pyproject.toml
uv.lock
README.md
```

## Data Model Direction

The harness should standardize a few durable concepts early:

- `CorpusManifest`: identifies a corpus, its source, bootstrap steps, layout, and determinism metadata.
- `SyntheticRepoSpec`: describes a seed, topology, packages, dependency shape, and scenario knobs.
- `QueryPack`: contains benchmark prompts grouped by query type and scenario.
- `GoldenSet`: stores expected evidence targets, ranking expectations, and optional latency thresholds.
- `EvaluationRun`: captures metadata, timings, normalized outputs, and score summaries.
- `EngineAdapter`: the minimal contract an engine must satisfy to participate in the harness.

All persisted artifacts should be text-based and diff-friendly. JSON is the default unless TOML or YAML offers a clear readability advantage.

## Milestone Order

Phase 1 should be implemented in this order:

### Milestone 1: Harness skeleton and contracts

- Create the Python project scaffold.
- Define core models and the engine adapter contract.
- Add a minimal CLI entry point for loading manifests and query packs.
- Add unit tests for schema validation and normalization behavior.

### Milestone 2: Corpora manifests and bootstrap tooling

- Define manifest schemas for external and synthetic corpora.
- Implement bootstrap commands that can prepare local fixture directories deterministically.
- Record provenance, versions, and seeds in generated metadata.

### Milestone 3: Deterministic synthetic monorepo generator

- Generate a TypeScript monorepo fixture with stable package structure and symbol names.
- Include auth, session, API, worker, and test package relationships.
- Ensure the repo can support the hero query path around session invalidation.
- Make generation fully seed-driven so goldens remain stable.

### Milestone 4: Query packs and goldens

- Define the initial query pack format.
- Author the first goldens for exact, symbol, semantic, and impact-style prompts.
- Include explicit hero queries such as "where do we invalidate sessions?" and symbol/file variants around `invalidateSession`.

### Milestone 5: Reporting and comparison

- Emit machine-readable run outputs.
- Add normalized summaries for accuracy and latency metrics.
- Add comparison tooling for before/after or engine-to-engine evaluation.

### Milestone 6: CI integration

- Add a focused CI workflow for lint, tests, and a lightweight deterministic smoke evaluation.
- Keep CI corpora small and fast while preserving contract coverage.

## Risks and Mitigations

### Risk: The harness accidentally bakes in engine assumptions

Mitigation:
- Define a narrow engine adapter contract.
- Normalize outputs before scoring.
- Keep benchmark assets independent of backend implementation details.

### Risk: Synthetic fixtures drift away from the real product wedge

Mitigation:
- Encode auth/session invalidation scenarios directly into the synthetic repo design.
- Keep at least one hero query pack centered on the session invalidation path.
- Document why each synthetic scenario exists.

### Risk: Goldens become flaky or hard to maintain

Mitigation:
- Use seeded generation only.
- Prefer evidence targets and rank bands over brittle full-output snapshots.
- Normalize paths, timestamps, and ordering before comparison.

### Risk: Corpora bootstrap becomes too heavyweight for local iteration

Mitigation:
- Separate small CI corpora from larger local benchmark corpora.
- Store lightweight manifests in-repo and keep larger assets reproducible via bootstrap commands.

### Risk: Phase 1 scope expands into product implementation

Mitigation:
- Treat any real indexing, semantic retrieval, or impact inference as out of scope.
- If a task requires actual engine behavior, stub against the engine adapter and defer implementation.

## Validation Commands

These commands define the intended validation surface for Phase 1 once scaffolding exists:

```bash
uv run ruff check .
uv run ruff format --check .
uv run pytest
uv run python scripts/phase1_generate_synthetic.py --spec fixtures/corpora/manifests/synthetic-saas-small.json --output /tmp/hyperindex-synth
uv run python scripts/phase1_run_eval.py --engine dummy --corpus synthetic-saas-small --query-pack hero-sessions
uv run python scripts/phase1_compare_runs.py --baseline path/to/baseline.json --candidate path/to/candidate.json
```

For the current docs-only slice, validation is limited to file existence and content review because no code scaffold should be created yet.

## Definition of Done

Phase 1 is done when all of the following are true:

1. A Python-based harness can load corpora manifests, query packs, and goldens without engine-specific assumptions.
2. A deterministic synthetic TypeScript monorepo generator produces stable output from a fixed seed.
3. The synthetic corpus supports the future hero query path around session invalidation.
4. Query packs and goldens cover exact, symbol, semantic, and impact-style evaluations.
5. Reports include both per-query details and aggregate summaries suitable for comparison.
6. CI runs a lightweight deterministic validation path for the harness.
7. The implementation still does not include the Hyperindex daemon, indexer, query engine, extension, UI, or cloud service.

## Assumptions That Do Not Require User Input Right Now

- The repository is intentionally starting from a near-empty state, so Phase 1 docs can establish the initial harness structure.
- Python 3.12 plus `uv`/`pytest`/`ruff` is an acceptable default until stronger repo conventions exist.
- Initial manifests, query packs, and goldens should live in-repo because they are part of the evaluation source of truth.
- The first synthetic corpus should model a SaaS-style TypeScript monorepo with packages for auth, session, API, worker, and tests.
- The first hero query pack should explicitly exercise session invalidation behavior.
- CI should begin with a small deterministic smoke corpus and not attempt large benchmark runs.
- External real-world corpora can be referenced by manifest now and added incrementally later; Phase 1 does not need all corpora on day one.
- Normalized JSON outputs are sufficient for the first reporting format.

## Recommended Next Implementation Slice

The next prompt should implement Milestone 1 only:

- scaffold the Python project,
- define the engine adapter contract and core models,
- add manifest/query-pack loading primitives,
- and add tests for those contracts without introducing any real engine behavior.
