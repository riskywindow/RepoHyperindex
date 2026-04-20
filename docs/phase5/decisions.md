# Repo Hyperindex Phase 5 Decisions

## Decision 4G: Honor `impact.enabled` coherently across status and query flows

Date:

- 2026-04-13

Status:

- accepted

Context:

- The checked-in Phase 5 daemon exposed impact methods even when `impact.enabled = false` in
  runtime config.
- That made operator behavior misleading:
  - config said impact was disabled
  - query methods still acted as if the feature were available
- The fix needed to stay small and avoid widening the public contract.

Decision:

- Keep `impact_status` available even when the feature is disabled so operators can still inspect
  the boundary.
- When disabled:
  - `impact_status` reports `not_ready`
  - `analyze` and `explain` capabilities are `false`
  - diagnostics include `impact_disabled`
- Reject `impact_analyze` and `impact_explain` with `config_invalid` until
  `impact.enabled = true`.

Why:

- it makes the runtime honor the config that operators actually set
- it keeps the disabled-state behavior machine-readable
- it avoids widening the daemon surface or adding new maintenance methods

Consequences:

- Phase 5 closeout error handling is more coherent without reopening scope
- Phase 6 can treat disabled semantic/impact state as an explicit contract pattern rather than an
  accidental permissive path

## Decision 4F: Close Phase 5 on the shipped symbol/file contract and make persisted-state semantics explicit

Date:

- 2026-04-13

Status:

- accepted

Context:

- The checked-in Phase 5 engine, daemon, CLI, and benchmark adapter are coherent and
  benchmarkable for `symbol` and `file` targets, but the original planning docs still described
  `config_key` as a must-ship input kind.
- The checked-in fact model still has no first-class config-key anchors or usage edges, so the
  current harness path degrades config-backed benchmark queries to backing files instead of
  serving a native `config_key` contract.
- `impact_status` also synthesized manifest metadata even before any persisted impact build
  existed, which blurred the boundary between "ready to analyze" and "already materialized."

Decision:

- Close Phase 5 with `symbol` and `file` as the only shipped first-class input targets.
- Treat `config_key` as an explicit deferred compatibility boundary for Phase 6 rather than a
  partially shipped Phase 5 feature.
- Make persisted-state semantics explicit:
  - `impact_status` returns a manifest only when a stored impact build actually exists
  - a ready-but-unmaterialized snapshot reports `impact_build_missing`
  - impact manifests now surface the configured materialization mode instead of a hard-coded value
- Enforce the remaining output-policy knobs the daemon already advertises:
  - `include_possible_results`
  - `max_reason_paths_per_hit`

Why:

- it aligns the closeout docs with the code that is actually benchmarkable today
- it gives Phase 6 a clean starting point without pretending there is a native config-key slice
  already shipped
- it makes status/analyze responses easier for operators and future implementers to reason about

Consequences:

- Phase 5 is complete as a bounded impact-analysis slice over the current symbol graph and
  snapshot/runtime seams
- Phase 6 should add native config-key support only alongside a documented fact-model extension
- callers can now distinguish:
  - symbol-ready but not yet materialized
  - materialized and ready
  - stale/corrupt builds that will rebuild on analyze

## Decision 4D: Take profile-guided wins from borrowed analyze inputs and memoized enrichment helpers before changing the architecture

Date:

- 2026-04-13

Status:

- accepted

Context:

- The Phase 5 engine now has a real incremental/materialized path, but the hottest read/write
  paths still paid avoidable costs:
  - every `analyze_with_enrichment` cloned the full `ImpactEnrichmentPlan`
  - canonical alias collapse re-walked import/export chains per symbol
  - heuristic test affinity rescanned repo files per test file
  - incremental assembly rebuilt a global reference-occurrence lookup on every refresh
- The task explicitly asked for a focused performance/reliability hardening pass without reopening
  the core architecture.

Decision:

- Keep the current impact-store shape, traversal model, and daemon contract intact.
- Remove only high-leverage hot-path overhead:
  - analyze from borrowed enrichment inputs instead of cloning the full enrichment plan per query
  - precompute test-ranking bonuses once per query/plan
  - memoize canonical alias resolution during enrichment/global-context construction
  - reuse one package-root/stem source-candidate index for heuristic test affinity
  - reuse one reference-occurrence lookup during incremental assembly

Why:

- these changes improve the measured hot paths directly
- they do not require a new persistence model, new protocol methods, or a traversal redesign
- they keep the pass reviewable and correctness-focused

Consequences:

- hot-path profiling now reports:
  - enrichment build `2 ms`
  - direct impact `17 ms` for `200x` queries on the synthetic profile fixture
  - transitive propagation `81 ms` for `100x` queries
  - ranking plus reason-path generation `85 ms` for `100x` queries
  - incremental single-file update `4 ms`
- `impact_explain` still shares the normal analyze path; it was intentionally not widened into a
  separate stored lookup table in this pass

## Decision 4E: Keep impact recovery/operator flows local to the CLI and treat corrupted builds as stale and rebuildable

Date:

- 2026-04-13

Status:

- accepted

Context:

- Common Phase 5 failure modes now include corrupted impact-build JSON, stale config/schema
  digests, missing symbol prerequisites, and mismatches between stored enrichments and the current
  symbol graph.
- The public daemon protocol still intentionally exposes only `impact_status`, `impact_analyze`,
  and `impact_explain`.

Decision:

- Add CLI-local maintenance/debug commands:
  - `hyperctl impact doctor`
  - `hyperctl impact stats`
  - `hyperctl impact rebuild`
- Keep these as local operator flows instead of adding new public daemon methods.
- Degrade corrupted stored impact builds to stale/rebuildable state in status/runtime paths where
  possible instead of failing the whole repo/runtime summary.
- Keep `hyperctl reset-runtime` as the final escape hatch only when the impact store itself cannot
  be opened or repaired.

Why:

- it adds clean recovery paths for the most common operator failures without widening the daemon
  contract
- it preserves the existing protocol/document boundary around build/warm/rebuild methods
- it keeps durability behavior explicit and machine-inspectable

Consequences:

- `impact status` now survives unreadable stored builds and warns that analyze will rebuild
- runtime impact-status scanning skips or counts corrupted repo stores as stale instead of aborting
  the full runtime summary
- prior-build lookup skips corrupted historical builds during incremental refresh
- the CLI can now diagnose and repair impact materialization even when the daemon protocol stays
  unchanged

## Decision 4B: Refresh persisted impact enrichments from file-scoped contributions and explicit full-rebuild fallbacks

Date:

- 2026-04-12

Status:

- accepted

Context:

- Phase 5 now has a real bounded impact engine, but the prior daemon path rebuilt the full
  enrichment layer on every `impact_analyze`.
- Phase 2 already provides deterministic composed snapshots and typed snapshot diffs, including
  buffer-only changes.
- The implementation must stay debuggable and correctness-first while still proving real
  incremental behavior.

Decision:

- Persist one snapshot-scoped impact build record per repo/snapshot in the impact store.
- Persist file-scoped contribution records and signatures for the direct enrichment indexes that
  drive impact:
  - reverse references
  - reverse imports / exports
  - reverse file dependents
  - test associations
- Recompute only files whose contribution signatures changed between the previous and current
  snapshot, even when the underlying trigger is a buffer overlay or a dependency-resolution shift.
- Fall back to a full rebuild when:
  - there is no prior compatible snapshot/build pair
  - the impact-store schema version changed
  - the impact config digest changed incompatibly
  - persisted build records are corrupted
  - stored/current consistency cannot be trusted

Why:

- it gives Phase 5 a real, measurable incremental path without introducing speculative watcher or
  all-pairs cache logic
- it keeps the state rebuildable from snapshot and symbol inputs
- it makes fallback behavior explicit and reviewable instead of silently serving stale enrichments

Consequences:

- `impact_status` and `impact_analyze` now surface refresh metadata through the manifest
- the impact store now holds typed persisted build records instead of only a placeholder path
- additive protocol metadata must remain backward-compatible and optional

## Decision 1: Treat the existing symbol graph plus snapshot-derived file catalog as the Phase 5 source of truth

Date:

- 2026-04-09

Status:

- accepted

Context:

- Phase 5 must preserve the Phase 2 snapshot/runtime model and the Phase 4 symbol graph.
- There is no checked-in exact-search engine or Phase 3 handoff to rely on for core impact.

Decision:

- Build Phase 5 as a consumer of:
  - the existing Phase 4 `SymbolGraph`
  - the existing Phase 4 symbol-query behavior for canonical alias handling
  - a snapshot-derived file catalog built from the current Phase 2 snapshot inputs

Why:

- it preserves the current sources of truth
- it keeps Phase 5 local-first and snapshot-scoped
- it avoids widening scope into exact-search implementation work

Consequences:

- Phase 5 impact answers remain evidence-first and syntax-derived
- future exact-search integration can only be additive, not required

## Decision 2: Use a derived multi-layer impact projection instead of mutating Phase 4 graph semantics

Date:

- 2026-04-09

Status:

- accepted

Context:

- Phase 5 needs file, package, config-key, and test impact behavior that the Phase 4 graph does
  not directly model as first-class query outputs.
- The existing `GraphEdgeKind` meanings must remain preserved.

Decision:

- Build a separate derived Phase 5 impact projection with node kinds for:
  - `symbol`
  - `file`
  - `package`
  - `config_key`
  - `test`
- Keep the current Phase 4 graph unchanged and authoritative.

Why:

- it supports the needed Phase 5 ranking behavior without redefining Phase 4
- it keeps the impact layer reviewable and benchmarkable

Consequences:

- Phase 5 adds new derived enrichment logic
- Phase 4 edge semantics remain stable and reusable

## Decision 3: Persist only direct impact enrichments and build metadata, not transitive closure

Date:

- 2026-04-09

Status:

- accepted

Context:

- Phase 5 must support warm queries and incremental refresh, but precomputing all target closures
  would expand storage and invalidation cost quickly.

Decision:

- Use a dedicated per-repo impact store for:
  - build metadata
  - direct derived enrichments
  - debug/status stats
- Do not persist all-pairs transitive closure or per-query results.

Why:

- it balances warm-query speed with bounded incremental invalidation
- it keeps the impact layer rebuildable from the current symbol build and snapshot

Consequences:

- query-time traversal remains part of the normal read path
- Phase 5 must implement explicit full-rebuild fallbacks when invariants break

## Decision 4: Use three deterministic certainty tiers with typed reason paths

Date:

- 2026-04-09

Status:

- accepted

Context:

- The Phase 0 product wedge depends on trustworthy impact, not one opaque relevance score.

Decision:

- Expose three certainty tiers:
  - `certain`
  - `likely`
  - `possible`
- Require typed reason paths for non-self hits.

Why:

- it makes impact confidence visible and reviewable
- it preserves an evidence-first product posture

Consequences:

- ranking must account for certainty and path depth explicitly
- Phase 5 should avoid returning unsupported or speculative exactness claims

## Decision 4A: Every ranked hit carries one deterministic explanation payload

Date:

- 2026-04-12

Status:

- accepted

Context:

- The grouped `ImpactHit` model now drives user-facing blast-radius output directly.
- Raw `reason_paths` are useful, but callers still need a stable per-hit answer for why the
  result is present and what kind of change propagation it reflects.

Decision:

- Add one `ImpactHitExplanation` payload to every returned `ImpactHit`.
- Keep the payload narrow:
  - `why`
  - `change_effect`
  - `primary_path`
- Continue to expose the larger `reason_paths` array separately when callers request it.

Why:

- it keeps every result inspectable without semantic or LLM-generated summarization
- it lets ranking, duplicate collapse, and explanation all point at the same selected path

Consequences:

- the engine must choose one stable primary path per hit
- explanation text must stay evidence-first and deterministic
- callers can render understandable results even when they omit the larger `reason_paths` array

## Decision 5: Keep the first-class Phase 5 query targets narrow

Date:

- 2026-04-09

Status:

- accepted

Context:

- The checked-in synthetic impact benchmark can be satisfied without adding every future target
  kind at once.
- Package and test behavior are still needed, but mainly as ranked outputs.

Decision:

- Support these first-class Phase 5 input targets first:
  - `symbol`
  - `file`
  - `config_key`
- Treat package and test as ranked output kinds.
- Keep route handling file-backed in this phase instead of introducing a framework-specific route
  registry.

Why:

- it keeps the first implementation slice coherent
- it matches the strongest existing benchmark artifacts

Consequences:

- Phase 5 does not need route-framework semantics to start delivering value
- future widening to route or package targets must be explicit and documented

## Decision 6: Scaffold Phase 5 as two flat workspace crates plus thin protocol, daemon, and CLI glue

Date:

- 2026-04-09

Status:

- accepted

Context:

- The existing Rust workspace uses flat crates under `crates/`.
- Phase 5 needs a reviewable layout that is easy to compile and test without destabilizing the
  earlier phases.

Decision:

- Add two new flat workspace crates:
  - `crates/hyperindex-impact`
  - `crates/hyperindex-impact-store`
- Keep public transport and operator surfaces in the existing crates:
  - `hyperindex-protocol`
  - `hyperindex-daemon`
  - `hyperindex-cli`
- Add local `AGENTS.override.md` files in the new impact crates so future work stays scoped to
  Phase 5.

Why:

- it matches the repo's established workspace style
- it keeps the impact engine and store reviewable without creating a new top-level runtime subtree
- it lets the daemon and CLI adopt Phase 5 surfaces compatibly instead of forking control-plane
  ownership

Consequences:

- Phase 5 implementation work can stay isolated in dedicated crates while preserving the existing
  runtime spine
- daemon and CLI changes remain thin glue until real impact behavior lands

## Decision 7: Keep the first daemon and CLI impact surface explicitly scaffold-only

Date:

- 2026-04-09

Status:

- accepted

Context:

- This task requires the Phase 5 workspace to compile and prove wiring, but it explicitly forbids
  real impact analysis, graph enrichment, daemon handlers, and benchmark integration.

Decision:

- Add a typed `impact_analyze` protocol surface now.
- Wire daemon and CLI entry points through that surface.
- Return explicit `not_implemented` errors from the daemon glue until the real engine slice lands.

Why:

- it proves the transport seam and crate boundaries early
- it avoids fake behavior that would be mistaken for real impact analysis
- it preserves room for a later implementation without breaking the request shape again

## Decision 8: Use scenario-bounded query-time traversal with terminal package/test outputs

Date:

- 2026-04-12

Status:

- accepted

Context:

- Phase 5 now needs real transitive blast-radius traversal, but precomputing closures or allowing
  unconstrained walks would make invalidation and reviewability worse quickly.
- The current enrichments can support package and test outputs, but traversing through them would
  broaden the graph and explanation space aggressively.

Decision:

- Run transitive traversal at query time over symbol and file nodes only.
- Emit `package` and `test` entities as terminal results, not recursive traversal seeds.
- Apply scenario-default limits for:
  - max depth
  - max visited nodes
  - max traversed edges
  - max considered candidates
- Allow request-level overrides to tighten those limits only.

Why:

- it keeps the blast radius bounded and measurable
- it makes explanation paths deterministic
- it preserves conservative correctness over broad but shaky reach

Consequences:

- callers can narrow traversal budgets without widening the approved policy
- response stats and diagnostics now need to expose cutoff behavior
- future incremental work can continue to treat transitive closure as a query-time concern

Consequences:

- the Phase 5 workspace compiles and tests cleanly before the engine exists
- operators get a clear scaffold-only signal instead of partial or misleading impact answers

## Decision 8: Keep the public impact contract narrower than the longer-term Phase 5 target matrix

Date:

- 2026-04-09

Status:

- accepted

Context:

- The longer-term Phase 5 plan discusses `config_key` inputs and a dedicated impact store.
- This prompt is contract-only and explicitly requires route/config/API targets to land only if the
  checked-in fact model already supports them conservatively.
- The current checked-in public fact model does not yet expose first-class config-key, route, or
  API anchors.

Decision:

- Support only `symbol` and `file` as first-class public input targets in this contract slice.
- Add `impact_status`, `impact_analyze`, and `impact_explain` as the public methods.
- Keep persisted impact materialization visible only through config and manifest metadata.
- Do not expose public `impact_build`, `impact_warm`, or `impact_rebuild` methods yet.

Why:

- it keeps the public API phase-appropriate and implementable against the current repo state
- it avoids reopening Phase 4 fact-model scope just to satisfy future-facing target kinds
- it leaves room for either live or persisted execution without freezing premature operator
  controls

Consequences:

- config-key, route, and API targets are explicitly deferred in docs for now
- the public contract remains small while still reserving typed status/manifest hooks for a future
  materialized store

## Decision 9: Build the first impact enrichment slice as a live, deterministic projection with explicit unsupported-edge deferrals

Date:

- 2026-04-10

Status:

- accepted

Context:

- Impact analysis needs direct reverse indexes, ownership maps, package grouping, and test
  affinity before the direct engine can rank outputs coherently.
- The checked-in Phase 4 graph already provides deterministic `references`, `imports`, `exports`,
  containment, symbol spans, and file ownership signals.
- The checked-in repo still does not provide first-class config-key, route, or API/endpoint edge
  facts.
- The dedicated impact-store crate is still scaffold-only, and this slice does not yet need
  incremental invalidation or warm-query reuse.

Decision:

- Add a live enrichment layer in `hyperindex-impact` that derives only direct, snapshot-scoped
  support structures:
  - canonical alias collapse over symbol-level import/export chains
  - reverse references keyed by canonical symbol id
  - reverse import/export indexes for symbol dependents
  - reverse file dependents from file-level import edges
  - explicit symbol-to-file and file-to-symbol ownership maps
  - package membership from in-snapshot `package.json` discovery
  - conservative test associations from:
    - direct file imports
    - direct symbol references/imports/exports observed in test files
    - a unique same-package filename heuristic for `*.test.*` / `*.spec.*` files
- Keep config-key, route, and API/endpoint evidence explicitly deferred in the enrichment audit
  until the fact model grows real anchors for them.
- Do not add persistence for this slice.

Why:

- it gives the upcoming direct impact engine the indexes it actually needs without redefining
  Phase 4 edge semantics
- it keeps the enrichment explainable and deterministic
- it avoids widening into speculative path-search, framework routing, or premature store design

Consequences:

- impact planning can now consume prebuilt reverse lookups, package ownership, and conservative
  test affinity from one projection layer
- route/config/API behavior remains documented as unsupported instead of silently inferred
- persistence and reload stability stay deferred until a later slice proves that materialization is
  worth the invalidation complexity
