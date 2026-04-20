# Phase 5 Impact Guardrails

This crate contains the Phase 5 impact-engine scaffold.

## Scope

- typed impact-model scaffolding
- deterministic ranking, reason-path, and enrichment placeholders
- compile-safe engine entry points over the existing symbol graph
- explicit error surfaces for work that is still intentionally unimplemented

## Do Not Add Here Yet

- real impact traversal or ranking logic
- graph enrichment implementations
- daemon request handling logic beyond typed glue
- benchmark adapter integration
- semantic retrieval, embeddings, or answer generation
- framework-specific routing behavior

## Working Rules

- keep Phase 5 code evidence-first and deterministic
- treat the Phase 4 symbol graph and Phase 2 snapshot model as upstream inputs
- prefer small typed modules over speculative abstractions
- use explicit diagnostics or `NotImplemented` errors instead of fake impact answers
- update `docs/phase5/status.md` after meaningful impact-work changes
- update `docs/phase5/decisions.md` when the Phase 5 layout or contracts change durably

