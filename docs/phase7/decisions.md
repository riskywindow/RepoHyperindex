# Repo Hyperindex Phase 7 Decisions

## 2026-04-21 Dedicated Planner Crate With A Callable Scaffold Front Door

### Status

- accepted

### Context

- The Phase 7 execution plan calls for a dedicated planner layer, one public planner query method,
  and a `hyperctl query` front door.
- This task is only for workspace scaffolding:
  no real route execution, score fusion, result grouping, daemon handler orchestration, or harness
  integration should ship yet.
- The repo already uses dedicated Rust crates for major capabilities, with local `AGENTS` override
  files for phase-specific guardrails.

### Decision

- Add a dedicated `crates/hyperindex-planner/` subtree with a local `AGENTS.override.md`.
- Keep the first Phase 7 crate layout explicit and phase-named:
  - `planner_model`
  - `query_ir`
  - `intent_router`
  - `route_registry`
  - `planner_engine`
  - `score_fusion`
  - `result_grouping`
  - `trust_payloads`
  - `daemon_integration`
  - `cli_integration`
  - `common`
  - `exact_route`
- Wire additive protocol, daemon, and CLI glue now:
  - `hyperindex-protocol/src/planner.rs`
  - `RequestBody::PlannerQuery`
  - `SuccessPayload::PlannerQuery`
  - `hyperindex-daemon/src/planner.rs`
  - top-level `hyperctl query`
- Keep the callable front door scaffold-only for this slice:
  - intent classification and IR building are real and deterministic
  - exact search is typed as unavailable
  - symbol, semantic, and impact routes appear only as planned traces
  - planner responses return explicit diagnostics and empty grouped results

### Why

- A dedicated crate matches the repo's existing workspace style and keeps planner ownership clear.
- Named modules make the Phase 7 scaffold easy to review against the execution plan.
- A callable but inert front door proves the end-to-end wiring without widening scope into real
  engine orchestration.

### Consequences

- The Phase 7 workspace now compiles and exposes planner-shaped contracts without destabilizing
  Phases 1–6.
- Later work can replace the placeholder route traces with real symbol, semantic, and impact
  execution without redesigning the planner surface.
- Benchmark and harness integration remain intentionally deferred to a later Phase 7 slice.

## 2026-04-20 Dedicated Planner Layer With One Public Query Method

### Status

- accepted

### Context

- The checked-in repository already has separate runtime, symbol, impact, and semantic layers.
- There is no checked-in exact-search runtime.
- Phase 7 must add a global planner without replacing the existing engines or adding a new planner
  persistence system.

### Decision

- Add a dedicated `hyperindex-planner` crate.
- Expose one public planner request and response pair through the daemon protocol:
  `PlannerQueryParams` and `PlannerQueryResponse`.
- Do not add planner build, planner rebuild, or planner status methods in the first public slice.

### Why

- The planner is an orchestration layer, not an index or store.
- Existing daemon status already reports readiness for the underlying engines.
- One public query method keeps the first Phase 7 slice reviewable and compatible.

### Consequences

- Planner runtime behavior will be visible through per-query traces rather than through a separate
  planner build lifecycle.
- Engine-specific CLI commands remain useful as operator and debug surfaces.
- Planner integration stays additive to the current daemon contract.

## 2026-04-20 Typed Planner IR And Deterministic Heuristic Classification

### Status

- accepted

### Context

- Phase 7 must classify one surface query into exact, symbol, semantic, impact, or hybrid planner
  behavior.
- The planner must remain deterministic, benchmarkable, and debuggable.
- The current benchmark harness already has typed query packs, but the Phase 7 front door should
  not depend on those pack types existing.

### Decision

- Use a typed planner IR rather than a freeform JSON plan blob.
- Use deterministic feature-based classification with:
  - optional explicit intent hints
  - an ambiguity band
  - one normalized IR before route execution

### Why

- A typed IR is stable enough for traces, routing, and tests.
- Deterministic heuristics fit the benchmark-first delivery style of the repo.
- This design can be evaluated against the current typed packs without requiring a new schema
  immediately.

### Consequences

- The planner will classify real surface queries directly instead of relying only on pretyped
  query models.
- The harness can measure planner intent accuracy additively against current pack intent.
- Phase 7 avoids premature learned routing or opaque classifier behavior.

## 2026-04-20 Primary Plus Fallback Route Graph With Optional Exact Capability

### Status

- accepted

### Context

- Phase 7 must route across exact, symbol, semantic, and impact capabilities.
- The current repo has symbol, impact, and semantic engines, but no checked-in exact engine.
- The planner must stay evidence-first, especially for impact queries.

### Decision

- Use a primary-route plus bounded-fallback route graph.
- Model exact search as an optional internal route capability with an unavailable implementation in
  the current repo.
- Require deterministic seed resolution before calling the impact engine.
- Return explicit ambiguity instead of guessing when an impact seed is not unique.

### Why

- This keeps latency bounded and traces understandable.
- It preserves the exact-search ownership boundary without inventing a fake exact runtime.
- It protects trust on impact queries, where speculative answers are the highest-risk failure mode.

### Consequences

- The planner can emit exact-route-unavailable traces today without widening scope into exact
  engine work.
- Impact analysis remains the source of truth for blast-radius answers.
- Ambiguous impact queries will surface candidate seeds instead of merged speculative impact
  results.

## 2026-04-20 Evidence-Anchored Fusion, Grouping, And Trace Payloads

### Status

- accepted

### Context

- Phase 7 must combine candidates from different engines.
- Raw engine scores are not directly comparable.
- The product requirement for this phase is evidence-first behavior with deterministic summaries and
  planner traces.

### Decision

- Normalize all planner scores into deterministic engine-calibrated bands.
- Group results by canonical anchor in this order:
  - `symbol_id`
  - `path + span`
  - `ImpactEntityRef`
  - `path`
- Attach route-specific supporting evidence underneath each grouped planner result.
- Use structured trust payloads and machine-readable planner traces.
- Do not add freeform prose answer generation.

### Why

- Grouping by anchor removes duplicate cross-engine hits without hiding provenance.
- Structured trust payloads are easier to benchmark and debug than prose.
- Deterministic score bands are transparent enough for review and stable enough for compare flows.

### Consequences

- Planner output will remain evidence-shaped rather than answer-shaped.
- Report and CLI surfaces can summarize planner behavior without introducing hallucination risk.
- Later quality work can refine weighting and grouping without changing the core evidence model.

## 2026-04-20 Planner Harness Integration Starts With A New `daemon-planner` Adapter

### Status

- accepted

### Context

- The current harness already supports exact, symbol, semantic, and impact query packs and
  normalized `QueryHit`.
- There are no checked-in planner-specific query packs or auto-query configs.
- Phase 7 needs a measurable planner path without breaking the current harness contract.

### Decision

- Add a new `daemon-planner` adapter instead of replacing existing daemon-backed adapters.
- Reuse the current query packs and goldens first.
- Add planner-specific measurements as additive metrics only.

### Why

- This preserves existing per-engine baselines and compatibility review.
- It avoids premature schema churn before the planner runtime is proven.
- The harness already has the additive metric plumbing needed for this path.

### Consequences

- Planner quality can be measured against the current symbol, semantic, and impact packs first.
- Exact-pack planner evaluation remains capability-gated until a real exact engine exists.
- Planner-native evaluation assets can be added later only if the reused pack strategy proves
  insufficient.
