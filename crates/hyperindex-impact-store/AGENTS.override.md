# Phase 5 Impact Store Guardrails

This crate contains the Phase 5 impact-store scaffold.

## Scope

- impact-store path planning
- schema-version and migration placeholders
- deterministic metadata scaffolding for future rebuildable enrichments

## Do Not Add Here Yet

- real persistence tables
- transitive closure caches
- benchmark-specific data flows
- daemon-owned lifecycle orchestration

## Working Rules

- keep the store rebuildable from snapshot and symbol inputs
- keep schema state explicit and diff-friendly
- prefer typed manifests and path helpers over speculative database code
- update `docs/phase5/status.md` after meaningful store changes

