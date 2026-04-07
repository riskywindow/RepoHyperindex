# How To Add A Corpus

This guide covers adding either a new synthetic corpus variant or a new real-corpus entry during Phase 1.

## Ground Rules

- Keep the harness engine-agnostic.
- Do not add Hyperindex engine behavior here.
- Prefer deterministic artifacts and reviewable diffs.
- Preserve the product wedge: TypeScript local impact engine.
- Keep the hero path in mind: `where do we invalidate sessions?`

## Option 1: Add Or Adjust A Synthetic Corpus

Synthetic corpora are the preferred Phase 1 path because they are deterministic and do not depend on upstream repos.

### 1. Update the synthetic config

Start from [bench/configs/synthetic-corpus.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/synthetic-corpus.yaml).

Tune the knobs you need:

- `package_count`
- `file_count`
- `dependency_fanout`
- `route_count`
- `handler_count`
- `config_file_count`
- `test_file_count`
- `auth_flow_count`
- `edit_scenario_count`

### 2. Generate the corpus bundle

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora generate-synth \
  --config-path bench/configs/synthetic-corpus.yaml \
  --output-dir /tmp/hyperbench-new-bundle
```

### 3. Validate the generated bundle by running the harness

Smoke:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter fixture \
  --corpus-path /tmp/hyperbench-new-bundle \
  --output-dir /tmp/hyperbench-new-run \
  --mode smoke
```

Full:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench run \
  --adapter fixture \
  --corpus-path /tmp/hyperbench-new-bundle \
  --output-dir /tmp/hyperbench-new-run-full \
  --mode full
```

### 4. If the checked-in canonical synthetic corpus changes

If your task is to refresh the repo’s canonical synthetic artifacts, make sure all of these stay aligned:

- [bench/configs/synthetic-corpus.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/synthetic-corpus.yaml)
- [bench/configs/corpus-manifest.synthetic.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/corpus-manifest.synthetic.yaml)
- [bench/configs/query-packs](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs)
- [bench/configs/goldens](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens)

Re-run:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora validate
UV_CACHE_DIR=/tmp/uv-cache uv run pytest
```

## Option 2: Add A Real Repo Corpus Entry

Real repos are Phase 1-compatible, but they are intentionally more manual.

### 1. Add or update the repo selection entry

Edit [bench/configs/repos.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/repos.yaml).

Required details:

- rationale
- expected tier
- why it is useful for Repo Hyperindex
- license status
- clone strategy
- pinning policy
- risks
- manual verification steps when facts are not fully verified yet

If any fact is not verified yet, mark it clearly and add a manual verification note instead of guessing.

### 2. Dry-run bootstrap first

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora bootstrap --dry-run
```

This is the safe first step even when network access is unavailable.

### 3. Pin the repo before real bootstrap

Set `pinned_ref` in [bench/configs/repos.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/repos.yaml) before running a real bootstrap.

Then:

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora bootstrap --repo-id <repo-id>
```

### 4. Snapshot the local checkout

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora snapshot \
  --path bench/corpora/<repo-id>
```

Use the snapshot metadata to confirm:

- commit SHA
- file count
- LOC
- package count when derivable
- warnings about repo shape

### 5. Add curated query seeds and expectations

Use the existing manual templates:

- [bench/configs/query-packs/svelte-cli-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/svelte-cli-curated-seed-pack.yaml)
- [bench/configs/query-packs/vite-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/vite-curated-seed-pack.yaml)
- [bench/configs/query-packs/next-js-curated-seed-pack.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs/next-js-curated-seed-pack.yaml)

Replace placeholders only after local inspection:

- exact token
- symbol
- semantic prompt
- impact target
- expected paths / symbols in the golden set

### 6. Re-run validation

```bash
UV_CACHE_DIR=/tmp/uv-cache uv run hyperbench corpora validate
UV_CACHE_DIR=/tmp/uv-cache uv run pytest
```

## Budget And Compare Considerations

Any corpus added in Phase 1 should still produce outputs that work with:

- [bench/configs/budgets.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/budgets.yaml)
- `hyperbench report`
- `hyperbench compare`

You do not need a real engine to validate the pipeline. The `FixtureAdapter` remains the default way to verify that the harness surface is coherent.

## Recommended Review Checklist

- Configs validate.
- Query packs and goldens align.
- Synthetic changes remain deterministic.
- Real-repo facts are either verified or clearly marked as manual follow-up.
- Smoke benchmark still runs.
- Report and compare still generate readable artifacts.
