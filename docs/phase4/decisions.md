# Repo Hyperindex Phase 4 Decisions

## Decision 1: Use tree-sitter for the Phase 4 parser layer

Date:

- 2026-04-07

Status:

- accepted

Context:

- Phase 4 must support incremental TS/JS parsing for working-tree and buffer-overlay snapshots.
- The repo already established Rust as the product/runtime language in Phase 2.

Decision:

- Use `tree-sitter` plus the TypeScript/TSX grammars for the Phase 4 parser layer.

Why:

- incremental parsing is a first-order requirement for this phase
- it matches the local-first, unsaved-buffer-heavy product wedge
- it avoids pulling in a compiler or Node dependency for the initial symbol engine

Consequences:

- Phase 4 symbol behavior is intentionally syntax-plus-module-resolution based
- full compiler semantics remain out of scope for this phase

## Decision 2: Keep symbol persistence separate from the control-plane runtime store

Date:

- 2026-04-07

Status:

- accepted

Context:

- The existing Phase 2 SQLite store owns repo registry, buffers, manifests, and related runtime
  metadata.
- Phase 4 will introduce much larger fact and graph tables with different read/write patterns.

Decision:

- Store symbol data in a dedicated per-repo SQLite database under the runtime data root instead of
  extending the existing control-plane SQLite database.

Why:

- it keeps control-plane and symbol-plane concerns isolated
- it allows independent migrations, tuning, and maintenance
- it reduces the chance that large symbol workloads destabilize repo/snapshot metadata flows

Consequences:

- Phase 4 will add a new persistence crate and migration surface
- cross-store coordination will happen at the daemon/service layer

## Decision 3: Use a two-level symbol identity model

Date:

- 2026-04-07

Status:

- accepted

Context:

- Phase 4 needs stable-enough declaration identity for queries and persistence, plus precise
  snapshot-local occurrence identity for definitions and references.

Decision:

- Adopt:
  - a persistent declaration-oriented `symbol_id`
  - a snapshot-scoped `occurrence_id`

Why:

- path-or-span-only ids are too fragile
- name-only ids are too ambiguous
- the split keeps persistent query identity and precise evidence identity separate

Consequences:

- query APIs should resolve through `symbol_id`
- explain/debug output may expose `occurrence_id`
- cross-rename continuity remains out of scope unless explicitly added later

## Decision 4: Limit the Phase 4 graph to deterministic syntax-derived edges

Date:

- 2026-04-07

Status:

- accepted

Context:

- The approved Phase 4 scope includes symbol/reference/import/export/containment graph behavior,
  but explicitly excludes semantic retrieval and impact analysis.

Decision:

- Restrict Phase 4 graph edges to containment, imports, exports, and references that can be
  derived deterministically from syntax plus module resolution.

Why:

- it preserves a sharp line between symbol navigation and later impact-analysis work
- it keeps Phase 4 benchmarkable and incremental

Consequences:

- call graphs, data flow, and inferred impact remain out of scope
- Phase 4 query semantics stay evidence-first and exact

## Decision 5: Buffer overlays are indexed as immutable snapshot-specific overlay state

Date:

- 2026-04-07

Status:

- accepted

Context:

- The current runtime already models buffer-inclusive snapshots as immutable query inputs.
- The product wedge depends on local unsaved-edit freshness.

Decision:

- Index buffer overlays as facts attached to the target snapshot id instead of mutating the last
  committed snapshot’s durable facts in place.

Why:

- it matches the existing snapshot model
- it avoids corrupting committed snapshot state with editor-only changes
- it keeps query evidence aligned with the snapshot the user actually asked about

Consequences:

- Phase 4 must support snapshot-scoped overlay indexing and invalidation
- unchanged file facts should be reused aggressively to control churn

## Decision 6: Keep the initial Phase 4 workspace as three top-level crates under `crates/`

Date:

- 2026-04-07

Status:

- accepted

Context:

- The Phase 4 execution plan recommends `hyperindex-parser`, `hyperindex-symbols`, and
  `hyperindex-symbol-store` as the new implementation boundary.
- The checked-in repo already uses one crate per responsibility under `crates/`.
- Adding a nested Phase 4 workspace subtree would fight the existing workspace shape and duplicate
  guardrail docs.

Decision:

- Add the Phase 4 scaffold as:
  - `crates/hyperindex-parser`
  - `crates/hyperindex-symbols`
  - `crates/hyperindex-symbol-store`
- Update the existing `crates/AGENTS.override.md` guardrails instead of introducing a second local
  override file.

Why:

- it matches the current workspace style
- it keeps protocol, daemon, CLI, and new Phase 4 crates in one consistent Rust workspace
- it avoids inventing a second crate layout before the parser/symbol surface has proven itself

Consequences:

- Phase 4 crate boundaries stay explicit without restructuring the rest of the repo
- future implementation slices can add real parser and symbol internals without moving crates

## Decision 7: Expose Phase 4 symbol transport as success-shaped placeholder responses first

Date:

- 2026-04-07

Status:

- accepted

Context:

- This slice needs protocol, daemon, and CLI integration glue, but real parsing, extraction, and
  graph construction are explicitly out of scope.
- Returning transport-level `not_implemented` errors would leave the public surface unproven.

Decision:

- Add the public symbol request/response methods now and have them return deterministic empty
  result sets plus scaffold metadata while the engine internals are still placeholders.

Why:

- it proves the Phase 2 transport can carry the future Phase 4 query surface
- it allows smoke tests to validate store/bootstrap wiring without pretending to ship query logic
- it keeps the distinction between transport scaffolding and engine behavior explicit

Consequences:

- CLI and daemon integration are testable today without overselling the engine
- later Phase 4 slices can replace the placeholder internals without changing method names

## Decision 8: Split the public Phase 4 contract into parse, index, and query methods

Date:

- 2026-04-07

Status:

- accepted

Context:

- The earlier scaffold had a narrow `symbols_*` shape oriented around lookup, definitions,
  references, and explain output.
- The approved next task needs a broader public contract covering parser lifecycle, index lifecycle,
  symbol search/show, and location-based resolution.
- Real parsing and indexing are still out of scope for this slice, so the transport must be
  explicit without overpromising semantics.

Decision:

- Expose the public Phase 4 contract as:
  - `parse_build`
  - `parse_status`
  - `parse_inspect_file`
  - `symbol_index_build`
  - `symbol_index_status`
  - `symbol_search`
  - `symbol_show`
  - `definition_lookup`
  - `reference_lookup`
  - `symbol_resolve`

## Decision 9: Keep initial module resolution repo-local and metadata-backed

Date:

- 2026-04-08

Status:

- accepted

Context:

- This slice needs exact-enough cross-file symbol linkage for imports, re-exports, definitions,
  and references without pulling in compiler-grade TypeScript semantics.
- Snapshot inputs already contain deterministic repo-local file contents plus any checked-in
  `package.json` metadata.

Decision:

- Resolve only the smallest reliable subset in this phase:
  - relative in-repo specifiers with extension and `index.*` fallbacks
  - `.js` / `.jsx` / `.mjs` / `.cjs` specifiers mapped back to TS/JS source files when the target
    source file exists in the repo snapshot
  - direct workspace package-name imports backed by in-snapshot `package.json` `name` fields
- Do not resolve:
  - `tsconfig` / `jsconfig` path aliases
  - package `exports` maps
  - `node_modules` installs or registry packages

Why:

- it covers the repo-local linkage needed for Phase 4 queries
- it keeps the graph deterministic and explainable
- it avoids overstating certainty where compiler or package-manager semantics would be required

Consequences:

- import/export linkage is useful for direct in-repo navigation now
- alias-heavy monorepos remain partially analyzed until a later slice adds explicit config-backed
  resolution

Why:

- it keeps parser progress, durable index state, and query behavior separate
- it gives implementation slices explicit seams for build scheduling, inspection, and query serving
- it avoids a generic symbol endpoint that would become ambiguous once real parsing lands

Consequences:

- daemon and CLI glue must translate to the new method names
- fixture examples and docs must cover more than symbol search alone
- the public contract stays intentionally syntax-oriented until real engine behavior lands

## Decision 9: Keep parser artifacts metadata-first but tree-backed

Date:

- 2026-04-08

Status:

- accepted

Context:

- The Phase 4 parser slice needs to support snapshot parsing, reparsing after edits, and later
  extraction passes without committing the protocol to tree-sitter-specific node handles.
- Later extraction work still needs direct access to the raw syntax tree.

Decision:

- Represent one parsed file as:
  - protocol-facing `FileParseArtifactMetadata`
  - a `LineIndex` for byte/line translation
  - a normalized root summary handle
  - the raw tree-sitter `Tree` kept inside the parser crate

Why:

- it keeps the public/debug surface deterministic and portable
- it gives later extraction passes access to the real tree without forcing tree-sitter types into
  transport models
- it keeps incremental reparse state attached to one reusable file artifact

Consequences:

- parser callers can inspect spans and diagnostics without understanding grammar internals
- later extraction code can stay inside Rust crate boundaries and consume the retained tree
- daemon and CLI surfaces can expose parser evidence without leaking raw syntax-tree structures

## Decision 10: Persist exact file-fact JSON alongside normalized symbol rows

Date:

- 2026-04-08

Status:

- accepted

Context:

- The current extraction slice needs exact persistence/reload for `FileFacts` bundles.
- The next slices will also need row-level symbol/import/export/occurrence/edge data for targeted
  queries without reparsing whole JSON blobs.

Decision:

- Persist both:
  - exact protocol-facing file/artifact JSON per indexed file
  - normalized row-level symbol/import/export/occurrence/edge records in the same per-repo
    SQLite store

Why:

- exact JSON keeps roundtrip reload deterministic and debuggable
- normalized rows keep the store query-friendly for later daemon integration
- this avoids choosing prematurely between “JSON only” and “rows only”

Consequences:

- the symbol store schema version moves to `2` in this slice and is later bumped to `3` for
  incremental refresh metadata
- reload paths can prove exact persistence now
- later query slices can consume rows without changing the stored file-fact contract

## Decision 11: Reuse unchanged file facts by rebinding snapshot-local occurrence identity, and fall back aggressively when reuse is unsafe

Date:

- 2026-04-08

Status:

- accepted

Context:

- Phase 4 now needs real incremental refresh from Phase 2 snapshot diffs rather than only full
  snapshot indexing.
- `symbol_id` is intentionally stable across equivalent file content, but `occurrence_id` is
  snapshot-scoped and therefore cannot be copied across snapshots byte-for-byte.
- The store is rebuildable, so correctness and debuggability matter more than squeezing every last
  write out of the first refresh path.

Decision:

- Drive refresh from `SnapshotDiffResponse` and reparse only changed, added, or buffer-only changed
  eligible files.
- Reuse unchanged file facts without reparsing or re-extracting symbols, but rebind their
  snapshot-local occurrence ids and occurrence-backed edge ids to the target snapshot before
  persistence.
- Require a full rebuild when incremental preconditions fail, including:
  - symbol-store schema-version mismatch
  - incompatible parser/index config digest changes
  - corrupted persisted snapshot facts
  - unresolved consistency mismatches between the prior indexed snapshot and the Phase 2 snapshot
    inputs

Why:

- it keeps unchanged-file reuse real rather than cosmetic
- it preserves the documented snapshot-local occurrence model
- it makes fallback behavior explicit and easy to inspect when incremental reuse is unsafe
- it favors correctness and instrumentation over fragile cleverness

Consequences:

- the symbol store schema version moves to `3`
- incremental metrics can report reparsed vs reused files plus updated symbol/edge counts
- one-file edits can now be demonstrated as materially cheaper than a full rebuild in file-level
  smoke measurements without weakening snapshot equivalence

## Decision 12: Treat parse and symbol persistence as rebuildable operator-facing runtime state

Date:

- 2026-04-09

Status:

- accepted

Context:

- By the end of Phase 4, the parser cache, parse build records, and symbol store are valuable for
  warm loads and incremental refresh, but they are still derived from snapshots and are therefore
  rebuildable.
- The Phase 5 handoff needed stronger failure handling for corruption, stale manifests, schema
  drift, and daemon-unavailable debugging.

Decision:

- Treat persisted parse/symbol artifacts as rebuildable runtime state:
  - unreadable or incompatible parse cache/build entries are discarded and rebuilt on demand
  - symbol-store corruption or schema mismatch is surfaced explicitly through operator tooling
    rather than guessed through
  - expose local CLI recovery/inspection commands:
    - `hyperctl symbol rebuild`
    - `hyperctl symbol doctor`
    - `hyperctl symbol stats`

Why:

- it keeps failure handling explicit and easy to reason about
- it avoids letting corrupted runtime artifacts block normal development flows
- it gives Phase 5 engineers a supported path to inspect or recover symbol state even when the
  daemon is not available

Consequences:

- runtime artifacts prefer discard-and-rebuild over in-place repair
- operator docs now treat rebuild and doctor flows as part of normal Phase 4 maintenance
- later phases can add richer migrations or repair logic only if the rebuildable-state assumption
  stops being sufficient
