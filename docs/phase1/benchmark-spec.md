# Repo Hyperindex Phase 1 Benchmark Schema Contract

## Purpose

This document defines the Phase 1 config contract for the Hyperbench harness. The contract is intentionally engine-agnostic and covers only schemas, validation, and file formats. It does not imply that corpora fetching, synthetic generation, or benchmark execution already exist.

## File Formats

Hyperbench schema documents support:

- YAML via `.yaml` or `.yml`
- JSON via `.json`

All root schema documents are versioned with `schema_version: "1"`.

## Root Documents

The current schema layer supports these top-level document types:

### Repo tiers

Model: `RepoTierCatalog`

Purpose:
- defines the benchmark size buckets `S`, `M`, `L`, and `XL`
- captures LOC and package-count ranges for each tier

Example:
- [bench/configs/repo-tiers.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/repo-tiers.yaml)

### Benchmark hardware targets

Model: `BenchmarkHardwareCatalog`

Purpose:
- records the hardware classes used to interpret benchmark results
- keeps primary, secondary, and stretch-floor expectations explicit

Example:
- [bench/configs/hardware-targets.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/hardware-targets.yaml)

### Synthetic corpus config

Model: `SyntheticCorpusConfig`

Purpose:
- describes a deterministic synthetic TypeScript corpus shape
- records the seed, tier, package count, total repo file count, dependency fanout,
  route/handler/config/test counts, and hero-path knob for session invalidation coverage

Example:
- [bench/configs/synthetic-corpus.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/synthetic-corpus.yaml)

### Corpus manifest

Model: `CorpusManifest`

Purpose:
- describes a benchmark corpus independent of any engine implementation
- records whether a corpus is `synthetic` or `external`
- ties a corpus to query packs, golden sets, and relevant hardware targets

Example:
- [bench/configs/corpus-manifest.synthetic.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/corpus-manifest.synthetic.yaml)

### Real repo catalog

Model: `RealRepoCatalog`

Purpose:
- records selected or placeholder real-world benchmark repositories
- keeps tier expectations, license verification state, clone strategy, risks, and manual follow-up explicit
- lets Phase 1 continue even when repo metadata is only partially verified

Example:
- [bench/configs/repos.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/repos.yaml)

### Query pack

Model: `QueryPack`

Purpose:
- groups benchmark prompts for a corpus
- supports exact, symbol, semantic, and impact query types in a single document
- carries per-query `tags` and optional manual-curation `notes`

Example:
- [bench/configs/query-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-pack.yaml)
- [bench/configs/query-packs/synthetic-saas-medium-exact-pack.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/synthetic-saas-medium-exact-pack.json)
- [bench/configs/query-packs/svelte-cli-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/svelte-cli-curated-seed-pack.yaml)

### Golden set

Model: `GoldenSet`

Purpose:
- records expected evidence targets for query evaluation
- supports per-query hit expectations and optional latency budgets

Example:
- [bench/configs/golden-set.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/golden-set.yaml)

### Run metadata

Model: `RunMetadata`

Purpose:
- captures benchmark run identity and environment metadata
- keeps run provenance diff-friendly and serializable

Example:
- [bench/configs/run-metadata.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/run-metadata.json)

### Metrics document

Model: `MetricsDocument`

Purpose:
- stores raw metric samples and aggregate summaries
- supports latency, accuracy, system, and custom metric categories

Example:
- [bench/configs/metrics-document.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/metrics-document.json)

### Compare budget

Model: `CompareBudget`

Purpose:
- defines limits for regression checks
- supports max-value, min-value, and max-regression-percent style thresholds

Example:
- [bench/configs/budgets.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/budgets.yaml)
- [bench/configs/compare-budget.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/compare-budget.yaml)

### Compare output

Model: `CompareOutput`

Purpose:
- represents a normalized comparison between baseline and candidate runs
- stores metric deltas, budget evaluation results, and an overall verdict

Example:
- [bench/configs/compare-output.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/compare-output.json)

## Query Model

The query pack uses a discriminated union on the `type` field.

Supported query models:

- `ExactQuery`
  Requires `text`; supports `mode`, `path_globs`, and `languages`.
- `SymbolQuery`
  Requires `symbol`; supports `scope`.
- `SemanticQuery`
  Requires `text`; supports `path_globs` and `rerank_mode`.
- `ImpactQuery`
  Requires `target_type` and `target`; supports `change_hint`.

Every query also shares:

- `query_id`
- `title`
- `tags`
- optional `notes`
- `limit`

All checked-in symbol query packs currently use `scope: repo`. The real Phase 4 daemon adapter
added for symbol benchmarking in the latest cross-phase update supports that stable repo-scoped
path first and keeps narrower package/file scoping out of this slice.

## Golden Model

Golden expectations are deliberately evidence-first.

Each `GoldenExpectation` includes:

- a `query_id`
- one or more `expected_hits`
- an optional `expected_top_hit`
- an optional `max_latency_ms`
- optional `notes`

Each `ExpectedHit` captures:

- `path`
- optional `symbol`
- `reason`
- `rank_max`
- optional `min_score`

This keeps goldens robust against harmless formatting or ranking noise while still expressing the expected behavioral evidence.

## Validation Rules

Key validation rules enforced by the schema layer include:

- repo tiers cannot have inverted ranges
- hardware target IDs and repo tier entries must be unique within a catalog
- real repo catalogs must include at least one selected entry for each required Phase 1 tier: `S`, `M`, and `L`
- partially verified or placeholder real-repo entries must include manual verification steps
- synthetic corpus configs must include the core SaaS roles: `auth`, `session`, `api`, `worker`, and `tests`
- synthetic corpus `package_count` must be large enough to represent the declared roles
- hero-path-enabled synthetic corpus configs must reserve at least one route, handler, config file, test file, and auth flow
- synthetic corpus manifests must declare `synthetic_config_id`
- external corpus manifests must declare `source_uri` or `local_path`
- query packs cannot reuse `query_id`
- goldens cannot reuse `query_id`
- every query pack must have exactly one matching golden set
- every query in a pack must have a matching expectation in its golden set
- synthetic query coverage must meet or exceed the Phase 1 target counts:
  `100 exact`, `50 symbol`, `30 semantic`, `30 impact`
- metrics summaries must keep percentiles and mean inside min/max bounds
- compare budgets must define at least one threshold limit
- compare outputs must compare two different run IDs

## Canonical Phase 1 Query Artifacts

The canonical checked-in Phase 1 query artifacts live in two directories:

- [bench/configs/query-packs](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs)
- [bench/configs/goldens](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens)

Synthetic query artifacts are deterministic and machine-generated from the synthetic corpus
shape. They are split into four type-specific packs and four matching golden sets:

- `synthetic-saas-medium-exact-pack`
- `synthetic-saas-medium-symbol-pack`
- `synthetic-saas-medium-semantic-pack`
- `synthetic-saas-medium-impact-pack`

The matching golden sets use the same prefix and end in `-goldens`.

The checked-in synthetic pack set meets the Phase 1 minimum counts on its own:

- `100` exact queries
- `50` symbol queries
- `30` semantic queries
- `30` impact queries

Hero-query coverage is represented in all three required forms:

- exact/symbol-adjacent:
  `exact-invalidate-session` and `symbol-invalidate-session`
- semantic:
  `semantic-hero-session-invalidation`
- impact:
  `impact-invalidate-session`

## CI Smoke Contract

Phase 1 CI intentionally runs only the fast deterministic path. The canonical workflow is
[.github/workflows/phase1-smoke.yml](/Users/rishivinodkumar/RepoHyperindex/.github/workflows/phase1-smoke.yml),
which delegates to
[bench/scripts/ci-smoke.sh](/Users/rishivinodkumar/RepoHyperindex/bench/scripts/ci-smoke.sh).

The smoke path covers:

- schema and manifest validation via `hyperbench corpora validate`
- Python unit tests via `pytest`
- deterministic synthetic corpus generation via `hyperbench corpora generate-synth`
- fixture-backed smoke benchmark execution via `hyperbench run --adapter fixture --mode smoke`
- report rendering via `hyperbench report`
- baseline-versus-candidate comparison via `hyperbench compare`

The smoke path explicitly does not include:

- real-repo bootstrap
- network-dependent corpus fetches
- full benchmark runs
- CI coverage for the real Hyperindex symbol engine

This keeps CI fast, deterministic, and compatible with fresh clones and offline-safe local
development.

## Real-Corpus Seed Workflow

Real-corpus query artifacts are intentionally smaller and manually curated. Because upstream
repos can change and this repository does not pin or bootstrap them during docs-only or
offline work, the checked-in real-corpus seed packs are placeholders with explicit notes.

Current seed packs:

- [bench/configs/query-packs/svelte-cli-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/svelte-cli-curated-seed-pack.yaml)
- [bench/configs/query-packs/vite-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/vite-curated-seed-pack.yaml)
- [bench/configs/query-packs/next-js-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/next-js-curated-seed-pack.yaml)

## Operator Docs

Phase 1 operator-facing usage lives in:

- [bench/README.md](/Users/rishivinodkumar/RepoHyperindex/bench/README.md)
- [docs/phase1/how-to-run-phase1.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/how-to-run-phase1.md)
- [docs/phase1/how-to-add-a-corpus.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/how-to-add-a-corpus.md)
- [docs/phase1/phase1-handoff.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/phase1-handoff.md)

Those guides are the source for fresh-clone setup, smoke and full local runs, report and
compare commands, and the manual workflow for adding real corpora without blocking Phase 1.

Manual curation workflow for each real repo:

1. Run `hyperbench corpora bootstrap --dry-run` and then bootstrap the pinned repo once
   `pinned_ref` is set in [bench/configs/repos.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/repos.yaml).
2. Run `hyperbench corpora snapshot` on the local checkout to capture the pinned SHA,
   manifest hash, file count, and LOC.
3. Inspect stable areas of the snapshot and replace the placeholder exact token, symbol,
   semantic prompt, and impact target in the corresponding curated seed pack.
4. Replace `__manual_verification_required__` in the matching golden set with concrete
   expected paths and symbols, keeping notes for any manually curated judgments.
5. Re-run `hyperbench corpora validate`, `ruff`, and `pytest` to ensure the curated pack
   remains schema-valid and cross-artifact validation still passes.

## Serialization Contract

Every root document inherits the same helper behavior:

- load from JSON text
- load from YAML text
- load from a file path
- dump to JSON text
- dump to YAML text

The canonical implementation lives in [bench/hyperbench/schemas.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/schemas.py).

## Run Output Contract

`hyperbench run` writes a run directory with:

- `summary.json`
  Canonical machine-readable run summary, including host/run metadata and top-level instrumentation.
- `events.jsonl`
  Event stream for corpus preparation, query execution, refresh execution, and run summary emission.
- `metrics.jsonl`
  Metric sample and summary records for query latency, pass rate, refresh timing, wall clock, memory, and output disk usage when available.
- `query_results.csv`
  Flat per-query rollup for spreadsheet or PR review.
- `refresh_results.csv`
  Flat per-refresh-scenario rollup.
- `metric_summaries.csv`
  Aggregate metric summaries suitable for compare and reporting.

Backward-compatible run-output extensions now used by the daemon-backed symbol and impact adapters:

- `summary.json`
  Adds `benchmark_dimensions`, `prepare`, and `refresh_summary` sections so fixture vs daemon,
  cold vs warm, and full-build vs incremental behavior are machine-readable.
- `events.jsonl`
  The `prepare` and `refresh` events may now include adapter metadata such as transport mode,
  parser/symbol build summaries, impact analyze/materialization summaries, refresh mode, and
  fallback reason.
- `refresh_results.csv`
  May include `refresh_mode`, `fallback_reason`, `loaded_from_existing_build`,
  `parse_build_latency_ms`, `symbol_build_latency_ms`, `impact_analyze_latency_ms`,
  `impact_refresh_mode`, and `target_path`.
- `metrics.jsonl` and `metric_summaries.csv`
  May include prepare/build and refresh-phase metrics such as:
  - `prepare-latency`
  - `prepare-parse-build-latency`
  - `prepare-symbol-build-latency`
  - `prepare-impact-analyze-latency`
  - `refresh-parse-build-latency`
  - `refresh-symbol-build-latency`
  - `refresh-impact-analyze-latency`

The run summary captures:

- OS, CPU, RAM, and tool versions
- git SHA when available
- adapter name
- corpus id
- wall-clock timing
- overall query latency p50 and p95
- refresh latency p50 and p95
- peak RSS or nearest available memory measure
- output disk usage

Memory collection degrades gracefully. If the platform cannot provide peak RSS, the field remains `null` and budget evaluation can warn instead of failing.

## Report And Compare Contract

`hyperbench report` reads a completed run directory and emits:

- `report.json`
- `report.md`

The Markdown form is intentionally short and PR-friendly.

`hyperbench compare` reads a baseline run, a candidate run, and a budget config, then emits:

- `compare.json`
- `compare.md`

The JSON output uses the Phase 1 compare schema, while the Markdown output is optimized for issues and pull requests.

Budget evaluation behavior:

- missing or unavailable metrics produce `warn` budget results instead of hard failures
- threshold failures still respect the configured severity
- overall compare verdict is `fail` if any budget fails, `warn` if any budget warns, otherwise `pass`

## Profiling Helpers

Phase 1 also includes lightweight helper scripts under [bench/scripts](/Users/rishivinodkumar/RepoHyperindex/bench/scripts):

- [profile-harness.sh](/Users/rishivinodkumar/RepoHyperindex/bench/scripts/profile-harness.sh)
  Runs the Python harness under `cProfile`.
- [profile-rust-engine-placeholder.sh](/Users/rishivinodkumar/RepoHyperindex/bench/scripts/profile-rust-engine-placeholder.sh)
  Placeholder guidance for future Rust engine profiling.

## Synthetic Bundle Outputs

The synthetic generator emits a deterministic bundle with:

- `repo/`
  The generated TypeScript monorepo fixture.
- `corpus-manifest.json`
  A generated manifest pointing at the local `repo/` directory.
- `ground_truth.json`
  Hero-path and evidence metadata for later evaluation work.
- `query-pack.json`
  Combined lexical, symbol, semantic, and impact-style query inputs.
- `golden-set.json`
  Combined evidence expectations for the generated query pack.
- `query-packs/`
  Type-specific generated exact, symbol, semantic, and impact query packs.
- `goldens/`
  Type-specific golden sets matching the generated query packs.
- `edit_scenarios.json`
  Deterministic file-edit scenarios for incremental refresh testing.

The current generator is intentionally fixture-only. It does not run any benchmark engine.

## Example File Set

The example configs under `bench/configs/` are meant to be valid fixtures for schema tests:

- [bench/configs/repo-tiers.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/repo-tiers.yaml)
- [bench/configs/hardware-targets.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/hardware-targets.yaml)
- [bench/configs/synthetic-corpus.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/synthetic-corpus.yaml)
- [bench/configs/corpus-manifest.synthetic.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/corpus-manifest.synthetic.yaml)
- [bench/configs/query-packs/synthetic-saas-medium-exact-pack.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/synthetic-saas-medium-exact-pack.json)
- [bench/configs/goldens/synthetic-saas-medium-exact-goldens.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens/synthetic-saas-medium-exact-goldens.json)
- [bench/configs/query-packs/vite-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/vite-curated-seed-pack.yaml)
- [bench/configs/corpus-manifest.synthetic.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/corpus-manifest.synthetic.yaml)
- [bench/configs/repos.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/repos.yaml)
- [bench/configs/query-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-pack.yaml)
- [bench/configs/golden-set.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/golden-set.yaml)
- [bench/configs/run-metadata.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/run-metadata.json)
- [bench/configs/metrics-document.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/metrics-document.json)
- [bench/configs/compare-budget.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/compare-budget.yaml)
- [bench/configs/compare-output.json](/Users/rishivinodkumar/RepoHyperindex/bench/configs/compare-output.json)
