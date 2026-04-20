# Phase 6 Semantic Store Guardrails

This crate contains the Phase 6 semantic-store scaffold.

## Scope

- semantic store path planning
- schema-versioned build metadata scaffolding
- embedding-cache and vector-index placeholder components

## Do Not Add Here Yet

- real vector persistence or ANN behavior
- production embedding caches
- daemon-owned lifecycle orchestration
- benchmark-specific data flows

## Working Rules

- keep persisted state rebuildable from snapshot-derived inputs
- keep schema and cache identities explicit and diff-friendly
- prefer typed metadata rows over speculative index features
- update `docs/phase6/status.md` after meaningful store changes
