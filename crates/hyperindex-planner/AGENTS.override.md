# Phase 7 Planner Guardrails

This crate contains the Phase 7 global planner scaffold.

## Scope

- typed planner model and query IR scaffolding
- deterministic intent routing and route-registry placeholders
- explicit exact-route capability boundaries
- deterministic trust payload and trace shaping
- daemon and CLI helper glue for the planner front door

## Do Not Add Here Yet

- real route execution against symbol, semantic, or impact services
- real score fusion, deduplication, or grouping logic
- exact-search engine work
- daemon-owned orchestration that bypasses the snapshot model
- benchmark or harness integration
- answer generation, summaries, or chat UX

## Working Rules

- keep planner behavior local-first, snapshot-scoped, and deterministic
- treat symbol, semantic, and impact engines as upstream sources of truth
- emit explicit scaffold diagnostics instead of fake planner answers
- preserve the exact-route boundary through typed unavailable traces
- update `docs/phase7/status.md` after meaningful planner work
- update `docs/phase7/decisions.md` when the planner layout changes durably
