# Repo Hyperindex Phase 7 Planner Protocol

## Purpose

This document defines the public Phase 7 unified query planner contract.

It covers the public protocol and runtime-config surface only. It does not authorize live route
execution, answer generation, chat-style summaries, or benchmark schema changes.

## Design Rules

- Keep the planner front door snapshot-scoped and local-only.
- Keep the public API small:
  - `planner_status`
  - `planner_capabilities`
  - `planner_query`
  - `planner_explain`
- Keep the contract deterministic and engine-agnostic.
- Reuse established Phase 4, Phase 5, and Phase 6 wire types where they already exist.
- Treat exact search as a typed compatibility boundary:
  the contract may name it, but the current repo still does not ship a live exact engine.
- Do not add answer-generation contracts in this phase.

## Public Daemon Methods

### `planner_status`

Purpose:

- report whether the planner front door is enabled for one repo and snapshot
- surface current route availability and diagnostics without executing any query

Request:

- `PlannerStatusParams`
  - `repo_id`
  - `snapshot_id`

Response:

- `PlannerStatusResponse`
  - `repo_id`
  - `snapshot_id`
  - `state`
  - `capabilities`
  - `diagnostics`

### `planner_capabilities`

Purpose:

- report the planner's supported modes, filters, route boundaries, and default budgets
- separate stable contract support from per-query traces

Request:

- `PlannerCapabilitiesParams`
  - `repo_id`
  - `snapshot_id`

Response:

- `PlannerCapabilitiesResponse`
  - `repo_id`
  - `snapshot_id`
  - `default_mode`
  - `default_limit`
  - `max_limit`
  - `budgets`
  - `capabilities`
  - `diagnostics`

### `planner_query`

Purpose:

- accept one unified raw user query against one repo and snapshot
- normalize the request into typed planner IR
- return grouped evidence-backed planner results when available
- surface explicit `no_answer` or `ambiguity` payloads instead of guessing

Request:

- `PlannerQueryParams`
  - `repo_id`
  - `snapshot_id`
  - `query`
  - `mode_override` optional
  - `selected_context` optional
  - `target_context` optional
  - `filters`
  - `route_hints`
  - `budgets` optional
  - `limit`
  - `explain`
  - `include_trace`

Response:

- `PlannerQueryResponse`
  - `repo_id`
  - `snapshot_id`
  - `mode`
  - `ir`
  - `groups`
  - `diagnostics`
  - `trace` optional
  - `no_answer` optional
  - `ambiguity` optional
  - `stats`

### `planner_explain`

Purpose:

- expose the same unified query contract with a richer debugging payload
- return normalized candidates plus grouped results and a planner trace
- keep explain data machine-readable and deterministic

Request:

- `PlannerExplainParams`
  - uses the same flattened request fields as `PlannerQueryParams`

Response:

- `PlannerExplainResponse`
  - `repo_id`
  - `snapshot_id`
  - `mode`
  - `ir`
  - `candidates`
  - `groups`
  - `diagnostics`
  - `trace`
  - `no_answer` optional
  - `ambiguity` optional
  - `stats`

## Core Planner Types

### Query input types

- `PlannerUserQuery`
  - raw user query text
- `PlannerMode`
  - `auto`
  - `exact`
  - `symbol`
  - `semantic`
  - `impact`
- `PlannerContextRef`
  - `symbol`
  - `span`
  - `file`
  - `package`
  - `workspace`
  - `impact`
- `PlannerQueryFilters`
  - `path_globs`
  - `package_names`
  - `package_roots`
  - `workspace_roots`
  - `languages`
  - `extensions`
  - `symbol_kinds`
- `PlannerRouteHints`
  - `preferred_routes`
  - `disabled_routes`
  - `require_exact_seed`
- `PlannerBudgetHints`
  - optional overrides for total timeout, max groups, and per-route budgets

### Normalized planner types

- `PlannerModeDecision`
  - requested mode
  - selected mode
  - selection source
  - reasons
- `PlannerQueryIr`
  - surface query
  - normalized query
  - selected mode
  - primary query style
  - ordered candidate styles for mixed queries
  - ordered planned routes after deterministic hint normalization
  - ordered intent signals
  - normalized exact, symbol, semantic, and impact subqueries when applicable
  - selected or target context when supplied
  - normalized filters
  - normalized route hints
  - resolved budget policy

### Result and evidence types

- `PlannerAnchor`
  - canonical grouping anchor
- `PlannerCandidate`
  - normalized route-level candidate shape used by `planner_explain`
- `PlannerResultGroup`
  - grouped planner result shape used by `planner_query` and `planner_explain`
- `PlannerEvidenceItem`
  - route-specific provenance payload
- `PlannerTrustPayload`
  - trust tier plus deterministic rationale
- `PlannerExplanationPayload`
  - deterministic template id, summary, and structured details

### Trace and fallback types

- `PlannerTrace`
  - planner version
  - selected mode
  - structured steps
  - per-route traces
- `PlannerRouteTrace`
  - route kind
  - availability
  - whether the route was selected
  - route status
  - skip reason when applicable
  - resolved route budget
  - candidate and group counts when applicable
  - elapsed time when applicable
- `PlannerNoAnswer`
  - explicit no-answer reason and details
- `PlannerAmbiguity`
  - explicit ambiguity reason, details, and candidate contexts when available

## Unified Query Contract

Phase 7 unified query input supports:

- raw user query text:
  - `PlannerUserQuery.text`
- explicit mode override:
  - `PlannerQueryParams.mode_override`
- auto mode:
  - `PlannerMode::Auto`
- selected context when the caller already knows a starting anchor:
  - `PlannerQueryParams.selected_context`
- target context when the caller wants the planner to bias toward a specific scope:
  - `PlannerQueryParams.target_context`
- path filters and globs:
  - `PlannerQueryFilters.path_globs`
- package filters:
  - `PlannerQueryFilters.package_names`
  - `PlannerQueryFilters.package_roots`
- workspace filters:
  - `PlannerQueryFilters.workspace_roots`
- language and symbol-shape filters:
  - `PlannerQueryFilters.languages`
  - `PlannerQueryFilters.extensions`
  - `PlannerQueryFilters.symbol_kinds`
- route hints:
  - `PlannerRouteHints.preferred_routes`
  - `PlannerRouteHints.disabled_routes`
  - `PlannerRouteHints.require_exact_seed`
- top-k control:
  - `PlannerQueryParams.limit`
- explain and trace controls:
  - `PlannerQueryParams.explain`
  - `PlannerQueryParams.include_trace`

The planner contract intentionally does not add:

- answer text
- prose summaries detached from evidence
- generated remediation or edit plans

## Status And Capability Model

`PlannerStatusResponse.state` is intentionally lightweight:

- `disabled`
  - planner is turned off by config
- `ready`
  - planner front door is enabled and at least one upstream route family is available
- `degraded`
  - planner front door exists but no usable route family is currently available

`PlannerCapabilitiesResponse` is the stable implementation contract. It records:

- supported modes
- supported filters
- supported route families
- default limit and max limit
- default planner budget policy

Route capabilities must separate:

- `enabled`
  - allowed by runtime config
- `available`
  - actually usable in the current runtime

Exact route behavior in the current repo is therefore:

- `enabled = true` by default
- `available = false`
- explicit reason present

## Budget And Timeout Contract

`PlannerBudgetPolicy` is the resolved budget model used by the planner IR and capabilities surface.

It contains:

- `total_timeout_ms`
- `max_groups`
- `route_budgets`

Each `PlannerRouteBudget` contains:

- `route_kind`
- `max_candidates`
- `max_groups`
- `timeout_ms`

Request-side `PlannerBudgetHints` may override the defaults, but the planner must normalize those
into one resolved `PlannerBudgetPolicy` in the IR before route execution.

## No-Answer And Ambiguity Contract

The planner must be explicit when it cannot answer.

`PlannerNoAnswerReason` currently supports:

- `planner_disabled`
- `no_route_available`
- `no_candidate_matched`
- `filters_excluded_all_candidates`
- `execution_deferred`

`PlannerAmbiguityReason` currently supports:

- `multiple_candidate_seeds`
- `mixed_route_signals`
- `multiple_anchors_remain`

The planner must not hide these states behind empty prose or speculative grouped results.

## Runtime Config Surface

`RuntimeConfig.planner` is the public config entry for this phase.

It contains:

- `enabled`
- `default_mode`
- `default_limit`
- `max_limit`
- `default_include_trace`
- `routes`
  - `exact_enabled`
  - `symbol_enabled`
  - `semantic_enabled`
  - `impact_enabled`
- `budgets`
  - `total_timeout_ms`
  - `max_groups`
  - `route_budgets`

The config surface is contract-first:

- it defines defaults
- it does not imply fused planner answers exist yet
- it keeps exact search typed without pretending the engine ships today

## Current Slice Note

The checked-in Phase 7 slice now implements the public contract plus a planner-side capability
registry and normalized route adapters over the existing symbol, semantic, and impact engines.

Current truthful behavior is therefore:

- the exact route may appear in traces as explicitly unavailable
- symbol, semantic, and impact routes may appear in traces as `executed` when they are ready and
  selected
- `planner_explain` may return normalized candidates from those executed routes
- `planner_query` may still return `no_answer.reason = execution_deferred` while score fusion and
  grouping remain deferred
- grouped results remain empty until fusion, deduplication, and grouping land

That is intentional. Route execution now exists behind a normalized boundary, while final
route-planning policy, score fusion, deduplication, grouping, and trust shaping remain later
Phase 7 slices.
