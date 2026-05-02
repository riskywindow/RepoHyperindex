# Repo Hyperindex Phase 7 Decisions

## 2026-04-22 Make Auto Route Planning Explicit And Deterministic Before Fusion

### Status

- accepted

### Context

- The previous Phase 7 planner slice could classify intent and execute normalized route adapters,
  but the registry still treated selected routes too mechanically.
- This task explicitly requires:
  - single-route plans
  - staged fallback plans
  - multi-route candidate plans for mixed queries
  - explicit mode override that bypasses auto routing where appropriate
  - deterministic handling for route budgets, early stop, partial results, and low-signal impact
    queries
- Fusion, grouping, daemon front-door changes, and harness integration remain out of scope for this
  slice.

### Decision

- Add a planner-owned route-policy layer in `crates/hyperindex-planner/src/route_policy.rs`.
- Make the registry execute one explicit internal policy kind per query:
  - `single_route`
  - `staged_fallback`
  - `multi_route_candidates`
  - `seed_then_impact`
- Collapse explicit non-auto mode overrides to one consulted route unless route hints disable that
  route entirely.
- Keep impact routing evidence-first:
  - go directly to impact only when a selected symbol/file context or concrete file seed already
    exists
  - otherwise resolve one deterministic symbol/file seed first
  - if no deterministic seed exists, keep to retrieval routes and surface low-signal or ambiguity
    state instead of guessing
- Apply total timeout budgets deterministically by pruning lower-priority routes before execution
  and recording that in planner traces and diagnostics.

### Why

- The planner needed a real execution policy boundary before fusion and grouping could be layered on
  top safely.
- Impact analysis is only trustworthy when it starts from one deterministic checked-in symbol or
  file anchor.
- Explicit mode override should mean something operational, not just a classifier hint.
- Budget-aware pruning and early stop keep the policy inspectable and benchmarkable without adding
  learned heuristics.

### Consequences

- Planner traces now explain:
  - which route policy was chosen
  - whether low-signal handling, early stop, budget pruning, or partial results applied
- `planner_query` no-answer details are now based on selected-route availability rather than any
  globally available route.
- Real daemon-backed planner tests can now assert route-policy behavior through traces without
  depending on future fusion or grouped-result semantics.

## 2026-04-22 Add Planner-Side Normalized Route Adapters Over Existing Engines

### Status

- accepted

### Context

- The prior Phase 7 planner work had:
  - deterministic intent classification
  - normalized planner IR
  - route traces
  - public planner status, capabilities, query, and explain contracts
- It did not yet have:
  - a registry that could describe per-route readiness and constraints explicitly
  - a normalized internal candidate shape for later fusion and trust payloads
  - a clean execution seam for the checked-in symbol, semantic, and impact engines
- This task explicitly requires:
  - capability detection for exact, symbol, semantic, and impact routes
  - normalized planner-side adapters over the existing engines
  - graceful handling for unavailable, unbuilt, degraded, or failing routes
  - multi-engine candidate retrieval without hard-coding route-specific execution logic all over
    the planner

### Decision

- Add a planner-owned route-adapter boundary in `hyperindex-planner` with:
  - `PlannerRouteAdapter`
  - `PlannerRouteCapabilityReport`
  - `PlannerRouteConstraints`
  - `NormalizedPlannerCandidate`
  - `PlannerRouteExecution`
- Make the route registry own both:
  - per-route capability inspection
  - selected-route execution and normalized candidate collection
- Keep exact as the existing unavailable boundary through
  `UnavailableExactRouteProvider`.
- Implement the real symbol, semantic, and impact executors in
  `crates/hyperindex-daemon/src/planner.rs`, not in the planner crate itself.
- Let `planner_explain` expose normalized candidates now, while `planner_query` stays
  grouping-deferred until fusion and grouping land.

### Why

- The planner crate should own the normalized boundary and candidate model used by later fusion and
  trust payload work.
- The daemon crate already owns the checked-in engine services and runtime config, so it is the
  right place to implement the actual symbol, semantic, and impact executors.
- Separating route capability reports from route execution keeps readiness explicit and testable
  without forcing planner callers to scrape traces.
- Keeping `planner_query` grouping-deferred preserves scope:
  this slice is about adapters and route execution, not final ranking or grouping policy.

### Consequences

- Planner traces can now distinguish:
  - route disabled or unavailable
  - route skipped due to unsupported requested filters
  - route executed with normalized candidates
- `planner_explain` is now the primary bring-up surface for normalized multi-engine candidates.
- `planner_query` remains honest:
  it may report `execution_deferred` when candidates exist but fusion and grouping do not yet.
- Future Phase 7 work can build fusion, dedupe, grouping, trust payloads, and a harness adapter on
  top of the new normalized candidate seam instead of bypassing it.

## 2026-04-21 Encode Deterministic Route Planning In The Query IR

### Status

- accepted

### Context

- The initial planner contract only carried `selected_mode` plus the raw normalized query text.
- This task explicitly requires:
  - a normalized IR that can represent exact, symbol, semantic, impact, and mixed queries
  - deterministic feature-based intent classification
  - normalized filters and planner hints
  - route-selection behavior that remains understandable before live execution exists
- A single selected mode was not enough to preserve mixed-route cases such as:
  - qualified-symbol natural-language lookups
  - the hero query shape `where do we invalidate sessions?`

### Decision

- Expand `PlannerQueryIr` additively with:
  - `primary_style`
  - ordered `candidate_styles`
  - ordered `planned_routes`
  - ordered `intent_signals`
  - per-style subqueries for:
    - exact
    - symbol
    - semantic
    - impact
- Normalize planner filters and route hints before they enter the IR.
- Keep classification deterministic and rule-based:
  - explicit mode override wins
  - regex, quoted, glob, and path-like text drive exact intent
  - identifier and qualified-symbol seeds drive symbol intent
  - natural-language question wording drives semantic intent
  - blast-radius wording and action verbs drive impact intent
  - selected context biases routing deterministically without becoming an opaque score source
- Make the route registry consume `planned_routes` from the IR rather than inferring everything
  from `selected_mode`.

### Why

- Mixed queries need a stable internal representation before live orchestration exists.
- The IR now explains not just which mode won, but which secondary styles and routes were kept.
- Tests can assert route-selection behavior directly without depending on future engine execution.

### Consequences

- Planner traces now expose omitted routes explicitly with `filtered_by_mode` or
  `filtered_by_route_hint`.
- Future live symbol, semantic, and impact execution can consume typed per-style subqueries
  directly instead of reparsing the raw text.
- Ambiguity handling is better represented in the IR now, but explicit ambiguity result payloads
  still remain a later execution-layer slice.

## 2026-04-21 Widen The First Public Planner Surface To A Contract-Complete Front Door

### Status

- accepted

### Context

- The original Phase 7 scaffold decision kept the first public surface to one `planner_query`
  method.
- This task explicitly requires the public contract for:
  - query status
  - unified query
  - explain or trace
  - planner capabilities
- The task is still contract-only:
  no live route execution, no answer generation, and no benchmark schema changes are allowed yet.

### Decision

- Widen the public planner daemon surface additively to:
  - `planner_status`
  - `planner_capabilities`
  - `planner_query`
  - `planner_explain`
- Replace the old planner intent-hint scaffold with a unified query request model centered on:
  - raw user query text
  - explicit `mode_override`
  - selected and target context
  - typed filters
  - typed route hints
  - typed budget and timeout hints
  - explicit `no_answer` and `ambiguity` payloads
- Keep the current runtime truthful by returning deferred traces and explicit
  `execution_deferred` no-answer payloads until real route execution lands.
- Add a dedicated `RuntimeConfig.planner` section so default mode, limits, route toggles, and
  timeout budgets are public config, not hidden implementation constants.

### Why

- The widened surface makes the public contract explicit enough for implementation without forcing
  callers to infer behavior from one overloaded query method.
- Status and capability endpoints keep operator and harness code from scraping per-query traces for
  static support information.
- A typed trust model and explicit no-answer states preserve the evidence-first product wedge
  without widening scope into answer generation.

### Consequences

- The public planner API is now slightly broader than the original scaffold plan, but it remains
  phase-appropriate and contract-only.
- Later implementation work can add live symbol, semantic, and impact routing without redesigning
  the request or response shapes.
- The repo still does not ship a real exact-search engine, and the widened contract continues to
  surface that gap explicitly.

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
