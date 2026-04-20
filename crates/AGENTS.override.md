# Runtime And Symbol Guardrails

This subtree contains the checked-in Rust workspace for the runtime spine and the Phase 4
parser/symbol scaffold.

## Scope

- local daemon/runtime spine
- versioned protocol and config contracts
- persistent local repo/store scaffolding
- scheduler/status scaffolding
- CLI and daemon smoke paths
- parser/symbol/store scaffolding for Phase 4
- deterministic placeholder query surfaces that stay inside the approved Phase 4 docs

## Do Not Add Here Yet

- real parsing or AST extraction logic beyond placeholder scaffolding
- exact search
- real symbol extraction
- semantic retrieval
- impact analysis
- VS Code extension logic
- cloud sync or remote service behavior

## Working Rules

- Keep runtime code under `crates/`, not `bench/`.
- Preserve the Phase 1 `hyperbench` CLI and artifact contracts.
- Prefer small typed modules with explicit crate boundaries.
- Keep outputs deterministic and local-first.
- For early slices, favor stubs and compile-safe scaffolding over speculative implementations.
- Keep Phase 4 parser/symbol work separate from the Phase 2 control-plane store.
- Update `docs/phase2/status.md` after meaningful runtime work.
- Update `docs/phase2/decisions.md` when a durable architecture choice is made.
