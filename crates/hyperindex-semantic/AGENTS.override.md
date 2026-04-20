# Phase 6 Semantic Guardrails

This crate contains the Phase 6 semantic-retrieval scaffold.

## Scope

- semantic model and chunk metadata scaffolding
- deterministic placeholder chunking contracts
- embedding-provider interfaces without real model execution
- semantic query and rerank glue that stays retrieval-only
- daemon and CLI helper glue for Phase 6 transport wiring

## Do Not Add Here Yet

- real chunk extraction
- real embedding generation
- real vector similarity math
- answer generation, summaries, or chat flows
- benchmark adapter integration
- daemon-owned request orchestration

## Working Rules

- keep semantic code retrieval-only and deterministic
- treat the Phase 2 snapshot model as the only content source
- keep symbol-backed ownership additive until real chunking lands
- prefer explicit placeholder diagnostics over fake semantic hits
- update `docs/phase6/status.md` after meaningful semantic work
- update `docs/phase6/decisions.md` when the Phase 6 layout changes durably
