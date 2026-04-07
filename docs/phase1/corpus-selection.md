# Repo Hyperindex Phase 1 Real-Corpus Selection

## Purpose

This document records the initial real-repo benchmark candidates for Phase 1. The goal is to keep Phase 1 unblocked while still being honest about what has and has not been verified yet.

The selection strategy is:

- choose one real repo for each target tier: small, medium, and large
- prefer repositories with visible TypeScript/JavaScript workspace structure
- prefer repositories that are useful for evidence-backed search, symbol navigation, and change-impact exploration
- pin by commit SHA later, after local clone and measurement
- mark any unverified fact as unverified instead of guessing

The machine-readable companion file is [bench/configs/repos.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/repos.yaml).

## Selection Rubric

Each candidate is scored qualitatively on:

- TypeScript/JavaScript relevance
- visible monorepo or workspace structure
- usefulness for Repo Hyperindex workflows
- operational risk for local benchmarking
- ease of pinning and repeatability

Because this pass did not clone or measure repositories, tier placement is recorded as an expected tier, not a measured tier.

## Selected Repos

### Small: `sveltejs/cli`

- Expected tier: `S`
- Why selected:
  The GitHub repository page shows a compact workspace with `packages`, `scripts`, `documentation/docs`, `community-addons`, and `pnpm-workspace.yaml`, which makes it a plausible small-tier benchmark candidate.
- Why useful for Repo Hyperindex:
  It gives the harness a small real repo with multiple packages and CLI/config surfaces, which is helpful for exact/symbol navigation and smaller-scope impact checks.
- License:
  MIT, based on the GitHub repository page.
- Clone strategy:
  Start with a shallow clone for initial local measurement; switch to a pinned commit SHA once the corpus manifest is created.
- Pinning policy:
  Pin a specific commit SHA after local clone and measurement.
- Risks:
  Tier fit is still unmeasured.
  Docs and community-addon/template content may need ignore rules.
- Manual verification still required:
  Measure TS/JS LOC and package count.
  Decide whether docs and add-on/template folders belong in the benchmark scope.

### Medium: `vitejs/vite`

- Expected tier: `M`
- Why selected:
  The GitHub repository page shows `packages`, `playground`, `docs`, `scripts`, and `pnpm-workspace.yaml`, plus a TypeScript-heavy language mix.
- Why useful for Repo Hyperindex:
  It is a realistic medium-complexity workspace for testing cross-package navigation, config discovery, and impact-style reasoning in a toolchain-centric codebase.
- License:
  MIT, based on the GitHub repository page.
- Clone strategy:
  Prefer a blobless filtered clone for initial benchmarking to reduce transfer cost, then pin a commit SHA.
- Pinning policy:
  Pin a specific commit SHA after local smoke validation.
- Risks:
  Tier fit is still unmeasured.
  Playground and docs content may distort retrieval if included without policy.
- Manual verification still required:
  Measure TS/JS LOC and package count.
  Decide whether playground and docs content should be in benchmark scope.

### Large: `vercel/next.js`

- Expected tier: `L`
- Why selected:
  The GitHub repository page shows a large monorepo-style layout with `apps`, `packages`, `examples`, `test`, `crates`, `bench`, and `pnpm-workspace.yaml`.
- Why useful for Repo Hyperindex:
  It is a strong large-tier stress candidate for evidence-backed search and impact-style workflows because it spans packages, apps, examples, tests, and non-TS subsystems.
- License:
  MIT, based on the GitHub repository page.
- Clone strategy:
  Prefer a blobless filtered clone for initial measurement, then pin a commit SHA for repeatability.
- Pinning policy:
  Pin a specific commit SHA in the corpus manifest and re-measure when the corpus is refreshed.
- Risks:
  Tier fit is still unmeasured.
  Non-TS content such as Rust crates may require ignore rules for TS/JS-focused runs.
  Examples and fixtures may add retrieval noise.
- Manual verification still required:
  Measure TS/JS LOC and package count.
  Decide whether Rust, examples, bench, and test-fixture content should be excluded.

## Why These Repos Are Useful For Phase 1

Together, these candidates give Phase 1:

- one compact workspace for fast local harness runs
- one medium TypeScript-heavy monorepo for day-to-day benchmark development
- one large framework repo for scale credibility

That balance matches the Phase 0 benchmark intent: a real small tier, a practical development tier, and a headline larger repo for stress and credibility.

## What Was Verified In This Pass

Verified from publicly visible GitHub repository pages during this task:

- repository owner/name
- visible top-level workspace structure
- license as shown by GitHub
- visible presence of package/workspace indicators such as `packages` or `pnpm-workspace.yaml`

Not verified in this pass:

- exact TS/JS LOC
- exact package counts
- actual clone size on disk
- whether generated files or fixtures should be excluded
- whether any repo has hidden operational issues during local benchmark setup

## Manual Verification Checklist

These checks must happen later, after cloning:

1. Measure TS/JS LOC and package counts using the benchmark inclusion/exclusion policy.
2. Confirm that each selected repo still fits its expected tier on the measurement date.
3. Inspect generated content, examples, playgrounds, docs, fixtures, and non-TS subtrees to decide what belongs in benchmark scope.
4. Pin each selected repo to a specific commit SHA in the future corpus manifest.
5. Record any repo-specific bootstrap or ignore rules before benchmark execution begins.

## Unblocked Phase 1 Outcome

Phase 1 is not blocked by incomplete remote verification because:

- the selection logic is documented here
- the machine-readable catalog exists in [bench/configs/repos.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/repos.yaml)
- every unverified item is explicitly labeled for later manual confirmation

That means the schema layer, planning docs, and future manifest/bootstrap work can continue without pretending the repo corpus has already been fully operationalized.
