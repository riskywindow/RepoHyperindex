# Repo Hyperindex Phase 7 Status: Planner Route Adapters Landed

Phase 7 status: planner route adapters landed as of 2026-04-22.

Primary planning documents:

- [execution-plan.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase7/execution-plan.md)
- [acceptance.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase7/acceptance.md)
- [decisions.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase7/decisions.md)

## 2026-04-22 Phase 7 Auto Route-Planning Policy

### What Was Completed

- Added a dedicated planner route-policy module in
  [route_policy.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/route_policy.rs)
  so route execution policy is explicit instead of being inferred ad hoc inside the registry.
- Implemented deterministic internal policy kinds for:
  - single-route execution
  - staged fallbacks
  - multi-route candidate execution for mixed queries
  - seed-then-impact execution when impact needs a deterministic symbol/file anchor first
- Updated
  [route_registry.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/route_registry.rs)
  to execute selected routes sequentially with:
  - total-timeout budget pruning
  - early stop for satisfied fallback chains
  - partial-result diagnostics when selected routes are unavailable or skipped
  - deterministic seed extraction from normalized symbol/file candidates before impact
- Updated explicit override handling in
  [intent_router.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/intent_router.rs)
  so non-auto overrides collapse to one consulted route instead of retaining auto-style fallback
  plans.
- Updated
  [planner_engine.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/planner_engine.rs)
  so traces and no-answer payloads record:
  - selected-route availability
  - low-signal handling
  - budget exhaustion
  - early stop
  - partial-result state
- Added focused coverage for:
  - explicit single-route override behavior
  - staged fallback stop-after-success behavior
  - fallback when the primary route is unavailable
  - mixed-query multi-route planning
  - seed-then-impact execution
  - deterministic budget pruning
  - selected file context leading directly into impact planning
- Tightened the daemon planner regression in
  [crates/hyperindex-daemon/src/planner.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/planner.rs)
  so it asserts stable route-trace behavior instead of depending on one engine family always
  materializing candidates for a mixed query.

### Key Decisions

- Keep the new policy layer internal to the planner crate:
  no protocol, daemon front-door, CLI, or harness contract changes were required for this slice.
- Treat explicit non-auto mode overrides as operational routing instructions, not just heuristic
  hints.
- Let impact analysis run only from a deterministic selected symbol/file context or a concrete file
  seed; otherwise resolve a seed first or stay on retrieval routes and surface low-signal state.
- Prefer inspectable rule-based pruning and stop conditions over opaque heuristics or score-driven
  fan-out.

### Commands Run

```bash
cargo fmt --all
cargo test -p hyperindex-planner
cargo test -p hyperindex-daemon planner_service_
```

### Command Results

- `cargo fmt --all`
  - passed
- `cargo test -p hyperindex-planner`
  - passed
  - exercised:
    - planner IR routing coverage
    - route-policy unit tests
    - registry-level fallback, impact-seed, and budget behavior
- `cargo test -p hyperindex-daemon planner_service_`
  - passed
  - revalidated planner policy behavior through the daemon-backed planner service seam

### Remaining Risks / TODOs

- Fusion, deduplication, grouping, and trust payload shaping are still placeholder layers above the
  new route-policy seam.
- The low-signal and seed-resolution rules are intentionally simple and deterministic; they still
  need future evaluation against real planner benchmarks before they should be treated as tuned.
- Exact remains intentionally unavailable in the current repo.
- The Phase 1 harness still has no `daemon-planner` adapter path.

### Next Recommended Prompt

- Implement deterministic fusion and grouping on top of the new route-policy and normalized
  candidate seam, without widening into planner-native harness schema changes yet

## 2026-04-22 Phase 7 Route Registry And Normalized Engine Adapters

### What Was Completed

- Added a planner-owned normalized route-adapter layer in:
  - [route_adapters.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/route_adapters.rs)
  - [route_registry.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/route_registry.rs)
- The new planner adapter boundary now models:
  - per-route readiness
  - route-specific supported filters
  - route constraints for later fusion and trust shaping
  - normalized internal candidates with:
    - engine type
    - engine-local score
    - file, symbol, and span provenance when available
    - engine diagnostics and notes
- Replaced the old trace-only planner registry behavior with real selected-route execution for:
  - symbol
  - semantic
  - impact
- Kept exact as the explicit unavailable compatibility boundary through
  [exact_route.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/exact_route.rs).
- Wired daemon-backed real executors in
  [crates/hyperindex-daemon/src/planner.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/planner.rs)
  so planner status, capabilities, query, and explain now consume the same route registry shape.
- `planner_explain` now returns normalized candidates from the existing engines.
- `planner_query` now records real candidate counts and route execution traces while still staying
  grouping-deferred.
- Added focused coverage for:
  - normalized candidate filtering and metadata preservation
  - generic multi-engine registry execution without planner special-casing
  - real daemon-backed route readiness for exact, symbol, semantic, and impact
  - explicit non-destructive impact target failure behavior

### Key Decisions

- Keep the normalized adapter contract in the planner crate and the real engine executors in the
  daemon crate.
- Treat route capability detection and route execution as one registry concern so the planner does
  not duplicate per-route branching elsewhere.
- Let `planner_explain` surface normalized candidates now, while preserving the existing deferment
  of fusion, dedupe, grouping, and trust payload shaping.
- Keep exact explicitly unavailable rather than widening into exact-engine work.

### Commands Run

```bash
cargo fmt --all
cargo test -p hyperindex-planner -p hyperindex-daemon
git diff --check
```

### Command Results

- `cargo fmt --all`
  - passed
- targeted `cargo test`
  - passed
  - exercised:
    - planner route-adapter normalization and registry behavior
    - real daemon-backed planner route readiness and explain execution
    - existing daemon symbol, semantic, and impact regression coverage
- `git diff --check`
  - passed

### Remaining Risks / TODOs

- Score fusion, deduplication, result grouping, and trust payload shaping are still placeholder
  layers above the new normalized candidate seam.
- `planner_query` still returns grouping-deferred no-answer payloads because grouped planner output
  is not implemented yet.
- Route-specific package and symbol-shape filter support is explicit and testable now, but it is
  still not uniform across all routes.
- There is still no checked-in exact-search engine, so exact remains intentionally unavailable.
- The Phase 1 harness still has no `daemon-planner` adapter path.

### Next Recommended Prompt

- Implement the next Phase 7 slice on top of the normalized candidates:
  add deterministic fusion, deduplication, grouping, and trust payload shaping, or add the
  backward-compatible `daemon-planner` harness adapter if you want planner-mode benchmarking next

## 2026-04-21 Phase 7 Query IR And Deterministic Intent Classification

### What Was Completed

- Expanded the public planner IR in
  [crates/hyperindex-protocol/src/planner.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/planner.rs)
  so one normalized query can encode:
  - exact-style intent
  - symbol-style intent
  - semantic natural-language intent
  - impact-style intent
  - mixed-route candidate styles and planned routes
- Added typed planner IR fields for:
  - `primary_style`
  - `candidate_styles`
  - `planned_routes`
  - `intent_signals`
  - per-style exact, symbol, semantic, and impact subqueries
- Replaced the old single-branch heuristic in
  [crates/hyperindex-planner/src/intent_router.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/intent_router.rs)
  with deterministic feature-based classification using:
  - regex, quoted, glob, and path-like signals
  - identifier and qualified-symbol signals
  - natural-language question signals
  - impact/blast-radius wording and action verbs
  - selected context bias
  - explicit mode override
- Normalized planner filters and route hints in
  [crates/hyperindex-planner/src/query_ir.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/query_ir.rs)
  so deduped, stable IR leaves the planner front door.
- Updated
  [crates/hyperindex-planner/src/route_registry.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/src/route_registry.rs)
  to consume planned routes from the IR and emit explicit skipped-route traces for
  mode-filtered and hint-filtered paths.
- Added representative planner coverage for:
  - identifier queries
  - regex/exact-looking queries
  - natural-language semantic queries
  - blast-radius/impact queries
  - ambiguous mixed queries
  - explicit override behavior
  - selected file context bias
  - the hero query shape `where do we invalidate sessions?`
- Refreshed the checked-in planner example catalog in
  [crates/hyperindex-protocol/fixtures/api/planner-examples.json](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/fixtures/api/planner-examples.json)
  so the public fixture matches the new IR contract and trace shape.

### Key Decisions

- Keep intent classification deterministic and explainable rather than adding learned routing or
  opaque scoring.
- Represent mixed queries as one selected primary style plus ordered candidate styles and planned
  routes instead of pretending the query is purely one mode.
- Keep route execution deferred:
  this slice only normalizes, classifies, and traces route selection.

### Commands Run

```bash
cargo fmt --all
cargo test -p hyperindex-planner -p hyperindex-protocol -p hyperindex-daemon -p hyperindex-cli
git diff --check
```

### Command Results

- `cargo fmt --all`
  - passed
- targeted `cargo test`
  - passed
  - exercised updated planner IR and routing coverage in:
    - `hyperindex-planner`
    - `hyperindex-protocol`
    - `hyperindex-daemon`
    - `hyperindex-cli`
- `git diff --check`
  - passed

### Remaining Risks / TODOs

- Planner execution is still deferred; no symbol, semantic, or impact backend calls happen yet.
- The richer IR now captures mixed-route cases, but explicit `ambiguity` payload shaping is still
  mostly a future execution-layer step.
- Fusion, dedupe, grouping, and harness integration are still intentionally unimplemented.

### Next Recommended Prompt

- Implement the next Phase 7 planner slice on top of this IR:
  execute real symbol, semantic, and impact routes against the existing engines while preserving
  exact as explicitly unavailable and keeping planner traces aligned with the new planned-route
  contract

## 2026-04-21 Phase 7 Planner Public Contract

### What Was Completed

- Replaced the old planner intent-hint scaffold with a contract-complete unified planner protocol
  in [crates/hyperindex-protocol/src/planner.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/planner.rs).
- Added additive daemon API surface for:
  - `planner_status`
  - `planner_capabilities`
  - `planner_query`
  - `planner_explain`
- Defined typed planner models for:
  - raw user query
  - explicit mode override
  - selected and target context
  - filters and route hints
  - budgets and timeouts
  - normalized candidates
  - grouped results
  - evidence items
  - trust and explanation payloads
  - planner traces and route diagnostics
  - no-answer and ambiguity reasons
- Added a public planner config surface under
  [RuntimeConfig.planner](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/config.rs)
  with default mode, limits, route toggles, and budget policy.
- Updated the planner, daemon, and CLI scaffolds so the widened public contract compiles and
  returns truthful contract-only responses:
  deferred traces,
  empty candidate and group payloads, and
  explicit `execution_deferred` no-answer payloads.
- Added a dedicated planner fixture catalog at
  [crates/hyperindex-protocol/fixtures/api/planner-examples.json](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/fixtures/api/planner-examples.json)
  plus roundtrip serialization coverage.
- Wrote durable Phase 7 docs:
  - [protocol.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase7/protocol.md)
  - [trust-model.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase7/trust-model.md)

### Key Decisions

- Widen the first public planner surface beyond the original one-method scaffold because this task
  explicitly requires status, capabilities, and explain/trace contracts.
- Keep the widened API contract-only:
  no live route execution,
  no answer generation,
  no benchmark integration, and
  no exact-engine work landed in this slice.
- Make planner defaults public through config instead of burying mode, limit, and timeout policy in
  planner-only constants.

### Commands Run

```bash
cargo fmt --all
cargo test -p hyperindex-protocol -p hyperindex-planner -p hyperindex-daemon -p hyperindex-cli
git diff --check
```

### Command Results

- `cargo fmt --all`
  - passed
- targeted `cargo test`
  - passed
  - exercised:
    - `hyperindex-protocol`
    - `hyperindex-planner`
    - `hyperindex-daemon`
    - `hyperindex-cli`
- `git diff --check`
  - passed
  - no whitespace or patch-format issues in the planner contract changes

### Remaining Risks / TODOs

- The public contract is now implementation-ready, but live route execution is still deferred.
- `planner_query` and `planner_explain` remain truthful no-answer stubs until real symbol,
  semantic, and impact orchestration lands.
- CLI support is still limited to the unified `hyperctl query` front door; dedicated CLI surfaces
  for planner status or explain are still optional future work.
- Harness integration is still unimplemented and must remain backward-compatible when added.

### Next Recommended Prompt

- Implement the next Phase 7 slice on top of the new contract:
  execute real symbol, semantic, and impact routes behind `planner_query` and `planner_explain`,
  keep exact explicitly unavailable, populate trust payloads from real evidence, and then add
  backward-compatible harness integration

## 2026-04-21 Phase 7 Planner Workspace Scaffold

### What Was Completed

- Added a dedicated `hyperindex-planner` crate under `crates/` with a local
  [AGENTS.override.md](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-planner/AGENTS.override.md)
  for Phase 7 guardrails.
- Scaffolded the requested Phase 7 planner modules in the repo's existing Rust crate style:
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
- Added additive protocol glue in
  [crates/hyperindex-protocol/src/planner.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/planner.rs)
  and extended the daemon API enums with `PlannerQuery`.
- Added additive daemon glue in
  [crates/hyperindex-daemon/src/planner.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/planner.rs)
  plus handler wiring that keeps planner responses snapshot-scoped and deterministic.
- Added additive CLI glue through a top-level
  [hyperctl query](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/bin/hyperctl.rs)
  command and a dedicated renderer in
  [crates/hyperindex-cli/src/commands/query.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-cli/src/commands/query.rs).
- Added compile-safe tests proving the scaffold is wired end to end:
  - planner crate unit tests
  - protocol roundtrip test coverage for planner responses
  - daemon planner service unit coverage
  - CLI planner command/unit renderer coverage

### Key Decisions

- Keep Phase 7 in one dedicated planner crate rather than spreading the first scaffold across
  existing engine crates.
- Make `planner_query` callable now, but keep it scaffold-only:
  deterministic intent classification, typed IR generation, explicit exact-route unavailability,
  planned route traces, empty grouped results, and explicit diagnostics.
- Preserve Phase 1–6 stability by avoiding:
  real route execution,
  real fusion/grouping,
  daemon-owned engine orchestration, and
  benchmark integration in this slice.

### Commands Run

```bash
cargo fmt --all
cargo test -p hyperindex-planner -p hyperindex-protocol -p hyperindex-daemon -p hyperindex-cli
git diff --check
```

### Command Results

- `cargo fmt --all`
  - passed
- targeted `cargo test`
  - passed
  - exercised `72` unit tests across:
    - `hyperindex-planner`
    - `hyperindex-protocol`
    - `hyperindex-daemon`
    - `hyperindex-cli`
- `git diff --check`
  - passed
  - no whitespace or patch-format issues in the scaffold changes

## 2026-04-20 Phase 7 Planning Baseline And Preservation Audit

### What Was Completed

- Created the initial durable Phase 7 planning document set:
  - [execution-plan.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase7/execution-plan.md)
  - [acceptance.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase7/acceptance.md)
  - [decisions.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase7/decisions.md)
  - this status document
- Audited the checked-in preserved surfaces that Phase 7 must build on:
  - Phase 0 product wedge and hero query
  - Phase 1 harness schemas, adapters, runner, metrics, report, and compare flow
  - Phase 2 daemon, transport, repo, snapshot, and overlay model
  - Phase 4 symbol protocol and query engine
  - Phase 5 impact protocol and daemon service
  - Phase 6 semantic protocol and daemon service
- Recorded the current exact-search reality explicitly:
  - there is no checked-in Phase 3 handoff doc
  - there is no checked-in exact-search runtime or daemon method
- Recorded the current planner-eval reality explicitly:
  - there are no checked-in planner-specific configs
  - there are no checked-in auto-query configs
  - there is no checked-in planner adapter path in the harness
- Chose the Phase 7 planning baseline:
  - dedicated planner crate
  - one public planner query method
  - typed IR
  - deterministic heuristic classification
  - primary plus fallback route graph
  - evidence-anchored fusion and planner traces
  - backward-compatible `daemon-planner` harness path

### Key Decisions

- Use one dedicated planner layer rather than widening existing engine crates to own global query
  orchestration.
- Keep the first public planner surface minimal:
  one planner query method, not a planner build or planner status surface.
- Preserve exact search as an optional route capability boundary with explicit unavailable-route
  traces in the current repo.
- Require deterministic seed resolution before calling the impact engine; ambiguous impact queries
  must not guess.
- Keep planner-mode harness integration additive:
  add `daemon-planner` and planner metrics first instead of changing the current query-pack schema
  immediately.

### Commands Run

```bash
rg --files docs repo_hyperindex_phase0.md AGENTS.md
find docs -maxdepth 2 -type f | sort
rg -n "planner|auto-query|auto_query|planner-mode|query planner|intent" bench docs crates tests
sed -n '1,220p' repo_hyperindex_phase0.md
sed -n '1,240p' docs/phase1/benchmark-spec.md
sed -n '1,220p' docs/phase2/phase2-handoff.md
sed -n '1,260p' docs/phase4/phase4-handoff.md
sed -n '1,260p' docs/phase5/phase5-handoff.md
sed -n '1,280p' docs/phase6/phase6-handoff.md
sed -n '1,260p' crates/hyperindex-protocol/src/api.rs
sed -n '1,220p' crates/hyperindex-protocol/src/snapshot.rs
sed -n '1,560p' crates/hyperindex-protocol/src/symbols.rs
sed -n '1,760p' crates/hyperindex-protocol/src/impact.rs
sed -n '1,460p' crates/hyperindex-protocol/src/semantic.rs
sed -n '1,760p' crates/hyperindex-daemon/src/handlers.rs
sed -n '1,420p' crates/hyperindex-daemon/src/state.rs
sed -n '1,260p' bench/hyperbench/metrics.py
sed -n '1,260p' bench/hyperbench/compare.py
git diff --check -- docs/phase7
```

### Command Results

- repository and docs inventory inspection:
  passed
- preservation audit reads:
  completed
- planner and auto-query config search:
  confirmed that no checked-in planner-specific or auto-query config currently exists
- `git diff --check -- docs/phase7`:
  passed
  - no whitespace or patch-format issues in the new Phase 7 docs

### Remaining Risks / TODOs

- The front door is scaffolded, but no real symbol, semantic, or impact route execution happens
  through the planner yet.
- Score fusion, result grouping, and trust payload shaping are still placeholder components that
  return empty grouped results.
- Exact search is still a capability gap in the current repo and remains surfaced as explicitly
  unavailable.
- Phase 7 benchmark and harness integration is still unimplemented and must be added in a later
  slice without changing existing artifact names or pack schemas.

### Next Recommended Prompt

- Implement the next Phase 7 planner slice on top of the scaffold:
  execute real symbol, semantic, and impact routes behind `planner_query`, keep exact explicitly
  unavailable, preserve snapshot-scoped traces, and only then add backward-compatible harness
  integration
