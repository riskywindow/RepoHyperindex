# Repo Hyperindex Phase 7 Trust Model

## Purpose

This document defines how the Phase 7 planner expresses trust without answer generation.

The trust model is evidence-first. It exists to tell the caller why a grouped planner result is
worth inspecting, not to generate prose answers on the planner's behalf.

## Non-Goals

This phase does not add:

- LLM confidence estimates
- freeform summaries
- answer cards
- speculative impact claims
- trust scores detached from concrete evidence

## Core Trust Objects

### `PlannerEvidenceItem`

This is the atomic provenance payload.

Each evidence item records:

- `evidence_kind`
- `route_kind`
- `label`
- `path` optional
- `span` optional
- `symbol_id` optional
- `impact_entity` optional
- `snippet` optional
- `score` optional
- `notes`

An evidence item must point back to a real upstream engine artifact when live execution exists.

### `PlannerAnchor`

This is the canonical grouping key.

The planner should group by the most precise stable anchor available in this order:

1. symbol anchor
2. span anchor
3. impact entity anchor
4. file anchor
5. package anchor
6. workspace anchor

The anchor is what makes grouped results diff-friendly and reviewable.

### `PlannerTrustPayload`

This is the machine-readable trust summary attached to a grouped result.

It records:

- `tier`
- `deterministic`
- `evidence_count`
- `route_agreement_count`
- `template_id`
- `reasons`
- `warnings`

### `PlannerExplanationPayload`

This is the deterministic explanation companion to the trust payload.

It records:

- `template_id`
- `summary`
- `details`

The explanation payload must be derived from the evidence and trust payload only.
It must not invent facts that are not already present in planner evidence.

## Trust Tiers

`PlannerTrustTier` has four levels.

### `high`

Use when:

- two or more precise routes agree on the same anchor
- or one authoritative route returns direct evidence with no competing anchors

Expected shape:

- strong anchor precision
- multiple confirming evidence items or one highly authoritative one
- no major warnings

### `medium`

Use when:

- one strong route produced a precise anchor
- evidence is grounded but cross-route confirmation is limited

Expected shape:

- one grounded anchor
- at least one strong evidence item
- warnings allowed only if they do not undermine the anchor itself

### `low`

Use when:

- the anchor is still grounded, but the evidence is thin
- or the planner had to rely on a weaker route or weaker fallback anchor

Expected shape:

- evidence exists
- the result is still inspectable
- the user should not treat it as highly confirmed

### `needs_review`

Use when:

- ambiguity is unresolved
- route signals conflict
- grouping required a weak fallback anchor
- exact seed resolution is required but unavailable
- or route execution produced warnings that materially weaken confidence

Expected shape:

- warnings should explain what needs human review
- the planner should prefer an explicit ambiguity payload over returning weak grouped results

## Deterministic Template Rule

Both trust and explanation payloads are template-based.

Examples of valid template ids:

- `planner.trust.single_route`
- `planner.trust.cross_route`
- `planner.trust.impact_direct`
- `planner.group.symbol`
- `planner.group.semantic`
- `planner.group.impact`

Template ids must stay stable enough for:

- fixture review
- CLI/operator debugging
- future benchmark assertions

## Route Agreement

`route_agreement_count` is the count of distinct route families contributing supporting evidence to
the grouped anchor.

Examples:

- semantic only:
  - `route_agreement_count = 1`
- symbol plus impact on the same anchor:
  - `route_agreement_count = 2`

The planner should not inflate this count by counting duplicate evidence from the same route
family more than once.

## Warning Model

`PlannerTrustPayload.warnings` exists so the planner can stay honest without suppressing grounded
results.

Expected warning cases include:

- exact route unavailable
- route execution deferred
- fallback grouping used a weaker anchor than `symbol` or `span`
- route conflict remained unresolved

Warnings are not generic diagnostics. They should explain why trust is limited for that result.

## No-Answer And Ambiguity Handling

The trust model is incomplete without explicit failure states.

Use `PlannerNoAnswer` when:

- the planner is disabled
- no route is available
- no candidates survive filters
- route execution is intentionally deferred in this phase

Use `PlannerAmbiguity` when:

- multiple candidate seeds remain
- different route families imply materially different anchors
- multiple anchors still look plausible after normalization

The planner must prefer these explicit payloads over speculative trust tiers.

## Current Phase 7 Slice

The current checked-in slice is contract-first.

That means:

- the trust payload and explanation payload shapes are final enough to implement against
- grouped results are still empty while live planning is deferred
- `execution_deferred` is currently the truthful no-answer path for the unified planner front door

This is acceptable for the current slice because the goal is to lock the public contract and trust
model before real route execution starts.
