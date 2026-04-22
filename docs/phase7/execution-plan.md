# Repo Hyperindex Phase 7 Execution Plan

## Purpose

Phase 7 adds the global query planner for Repo Hyperindex.

This phase is about deterministic orchestration over the existing engines, not about replacing
them. The planner must classify intent, build a unified query IR, choose routes across exact,
symbol, semantic, and impact capabilities, and fuse evidence-backed candidates through one
front-door query path.

This plan is based on an audit of:

- [repo_hyperindex_phase0.md](/Users/rishivinodkumar/RepoHyperindex/repo_hyperindex_phase0.md)
- [docs/phase1/benchmark-spec.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase1/benchmark-spec.md)
- [docs/phase2/phase2-handoff.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase2/phase2-handoff.md)
- [docs/phase4/phase4-handoff.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase4/phase4-handoff.md)
- [docs/phase5/phase5-handoff.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase5/phase5-handoff.md)
- [docs/phase6/phase6-handoff.md](/Users/rishivinodkumar/RepoHyperindex/docs/phase6/phase6-handoff.md)
- the checked-in harness and runtime source under `bench/` and `crates/`

Important current-state fact:

- there is still no checked-in `docs/phase3/phase3-handoff.md`
- there is still no checked-in exact-search crate or exact-search daemon method

Phase 7 must therefore preserve an exact-search compatibility boundary without pretending that a
real exact engine exists in the current repo.

## Final Phase 7 Scope

Phase 7 should ship the following:

- a dedicated Rust planner layer that sits above the current symbol, semantic, and impact engines,
  plus an optional exact route boundary
- deterministic intent classification from one surface query plus optional explicit hints
- a unified query IR that can represent lookup, navigation, and impact-oriented requests without
  mutating existing engine contracts
- route selection across exact, symbol, semantic, and impact capabilities with explicit route
  budgets and fallback rules
- deterministic score normalization, candidate fusion, deduplication, and result grouping
- evidence-first trust payloads where every returned result maps back to concrete engine evidence
  with file, symbol, and span provenance where available
- planner traces that show classification, routes considered, routes executed, fallback triggers,
  budget usage, and dedupe decisions
- daemon integration through one planner query method that composes existing engine services
  directly rather than reimplementing them
- CLI integration through one front-door query command while preserving engine-specific commands as
  debug and operator surfaces
- benchmark harness integration through a backward-compatible planner mode that reuses the current
  Phase 1 run, report, and compare flow

## Explicit Non-Goals

Phase 7 must not ship any of the following:

- a new exact-search engine
- a replacement symbol engine, semantic engine, or impact engine
- a new persistence or indexing subsystem beyond what the planner minimally needs for typed traces
- answer-generation UI, chat UI, editor UI, browser UI, or VS Code product work
- freeform LLM summaries, answer cards, or prose explanations not grounded in deterministic
  templates
- codemods, auto-refactors, or any write-side workflow
- cloud sync, team sharing, multi-user coordination, or remote planner execution
- learned routing, learned score fusion, or cross-encoder / LLM reranking
- Phase 8 work such as auto-query generation, self-improving planner feedback loops, or retrieval
  training pipelines
- backward-incompatible changes to checked-in Phase 1 query packs, goldens, run artifacts, or
  compare output

## Preservation Audit

This is the minimum stable surface Phase 7 must preserve.

### Phase 0 wedge to preserve

The product wedge from [repo_hyperindex_phase0.md](/Users/rishivinodkumar/RepoHyperindex/repo_hyperindex_phase0.md)
remains unchanged:

- local-first behavior
- TypeScript and JavaScript only
- freshness from snapshots, working tree overlays, and unsaved buffers
- evidence-first answers
- the hero path:
  `where do we invalidate sessions?`
- impact analysis remains the differentiating product wedge, not generic chat

Planning implication:

- the planner is a local orchestration layer for the existing TypeScript impact product, not a
  generalized repository assistant.

### Phase 1 benchmark harness contracts to preserve

The Phase 1 harness remains the source of truth for measurable evaluation.

Primary seams:

- [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  - `EngineAdapter`
  - `PreparedCorpus`
  - `QueryExecutionResult`
  - `RefreshExecutionResult`
  - normalized `QueryHit { path, symbol, rank, reason, score }`
- [runner.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/runner.py)
  - `run_benchmark(...)`
  - unchanged `summary.json`, `events.jsonl`, `metrics.jsonl`, `query_results.csv`,
    `refresh_results.csv`, `metric_summaries.csv`
- [schemas.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/schemas.py)
  - `ExactQuery`
  - `SymbolQuery`
  - `SemanticQuery`
  - `ImpactQuery`
  - `QueryPack`
  - `GoldenSet`
- [metrics.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/metrics.py)
  - additive `run_metric_rows` support already exists
- [compare.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/compare.py)
  - compare extraction is additive and metric-name-based

Current checked-in benchmark assets already present:

- exact, symbol, impact, and semantic synthetic packs under
  [bench/configs/query-packs](/Users/rishivinodkumar/RepoHyperindex/bench/configs/query-packs)
- matching goldens under
  [bench/configs/goldens](/Users/rishivinodkumar/RepoHyperindex/bench/configs/goldens)
- compare budgets under
  [bench/configs/budgets.yaml](/Users/rishivinodkumar/RepoHyperindex/bench/configs/budgets.yaml)

Current planner-specific state:

- there are no checked-in planner-specific query packs
- there are no checked-in auto-query configs
- there is no checked-in `daemon-planner` adapter yet

Planning implications:

- Phase 7 planner benchmarking must be a backward-compatible extension over the current harness
- normalized `QueryHit` remains the evaluation unit for planner-mode runs
- run, report, and compare artifact names stay unchanged
- planner-specific metrics may be added only as additive metric rows and additive compare metrics

### Phase 2 runtime and snapshot interfaces to preserve

The Phase 2 runtime remains the source of truth for repo identity, snapshot composition, transport,
and overlay precedence.

Primary seams:

- [snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
  - `ComposedSnapshot`
  - `SnapshotDiffResponse`
  - `SnapshotReadFileResponse`
- [manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
  - `SnapshotAssembler::resolve_file(...)`
  - `SnapshotAssembler::diff(...)`
- [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - `DaemonStateManager::create_snapshot(...)`
  - `DaemonStateManager::runtime_status(...)`
- [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
  - `RequestBody`
  - `SuccessPayload`
- [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
  - `HandlerRegistry`

Planning implications:

- every planner route must execute against a specific `repo_id` and `snapshot_id`
- buffer overlays and working-tree overlays are first-class planner inputs
- planner code must not probe repo roots directly for fresh content
- planner traces should carry the snapshot id used for every route execution

### Phase 3 exact-search ownership boundary to preserve

There is no checked-in Phase 3 exact-search implementation in this repository.

What does exist today:

- the harness-level `ExactQuery` contract in
  [schemas.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/schemas.py)
- the harness adapter seam
  `EngineAdapter.execute_exact_query(...)`
- snapshot-derived file eligibility work under
  [snapshot_catalog.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-parser/src/snapshot_catalog.rs)

What does not exist today:

- a checked-in exact-search daemon method
- a checked-in exact-search crate
- a checked-in Phase 3 handoff or status doc

Planning implications:

- Phase 7 must model exact search as an optional route capability, not as a guaranteed runtime
- the planner must emit deterministic `route_unavailable` diagnostics and traces when exact search
  is absent
- exact search remains the future source of truth for exact lexical retrieval
- Phase 7 must not silently absorb exact-search implementation work

### Phase 4 parser and symbol interfaces to preserve

The Phase 4 symbol engine remains the source of syntax-derived structure.

Primary seams:

- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/symbols.rs)
  - `SymbolId`
  - `SymbolRecord`
  - `SymbolOccurrence`
  - `GraphEdgeKind::{Contains, Defines, References, Imports, Exports}`
  - `SymbolIndexBuildId`
- [symbol_query.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-symbols/src/symbol_query.rs)
  - `search_hits(...)`
  - `show(...)`
  - `definition_occurrences(...)`
  - `reference_occurrences(...)`
  - `resolve(...)`
- [symbols.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/symbols.rs)
  - `ParserSymbolService`

Planning implications:

- symbol search and symbol resolution remain the primary route for structured code navigation
- symbol ids and spans are the preferred dedupe anchors for non-impact planner results
- Phase 7 must not change the meaning of Phase 4 graph edges

### Phase 5 impact interfaces to preserve

The Phase 5 impact engine remains the source of truth for blast-radius answers.

Primary seams:

- [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/impact.rs)
  - `ImpactAnalyzeParams`
  - `ImpactAnalyzeResponse`
  - `ImpactExplainResponse`
  - `ImpactEntityRef`
  - `ImpactReasonPath`
  - `ImpactManifest`
- [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/impact.rs)
  - `ImpactService::{status, analyze, explain}`

Current checked-in Phase 5 target reality:

- public impact targets are still `symbol` and `file`
- package and test are output kinds, not request target kinds
- planner must not invent unsupported impact target kinds

Planning implications:

- the planner may invoke impact analysis only after deterministic target resolution to one checked-in
  target kind
- if the planner cannot resolve a unique symbol or file seed, it must return ambiguity rather than
  guessing and calling `impact_analyze`
- impact results remain authoritative for impact intent

### Phase 6 semantic interfaces to preserve

The Phase 6 semantic engine remains the source of natural-language retrieval.

Primary seams:

- [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/semantic.rs)
  - `SemanticQueryParams`
  - `SemanticQueryResponse`
  - `SemanticChunkMetadata`
  - `SemanticRetrievalHit`
  - `SemanticRetrievalExplanation`
- [semantic_query.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-semantic/src/semantic_query.rs)
  - `SemanticSearchEngine::{search, search_loaded}`
- [semantic.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/semantic.rs)
  - `SemanticService::{status, build, query, inspect_chunk}`

Planning implications:

- semantic retrieval remains the primary route for behavior-oriented natural-language queries
- semantic explanation signals remain deterministic evidence, not generated prose
- planner fusion may consume semantic hits additively, but Phase 7 must not replace semantic
  retrieval with a new semantic path

### Planner / auto-query config state to preserve

Current audited state:

- no `docs/phase7/` directory existed before this task
- no checked-in planner config or planner evaluation pack exists under `bench/configs/`
- no auto-query config exists under `bench/`, `docs/`, `crates/`, or `tests/`

Planning implication:

- the first Phase 7 harness slice should reuse existing query packs and goldens instead of
  introducing a new planner-only schema immediately

## Proposed Runtime / Planner File Tree

This is the target Phase 7 runtime shape. The first implementation can stay smaller as long as the
ownership boundaries remain clear.

```text
docs/
  phase7/
    execution-plan.md
    acceptance.md
    decisions.md
    status.md

crates/
  hyperindex-planner/
    src/
      lib.rs
      intent.rs
      ir.rs
      routes.rs
      budgets.rs
      normalize.rs
      fuse.rs
      dedupe.rs
      grouping.rs
      evidence.rs
      trace.rs
      planner.rs
      exact_route.rs
  hyperindex-protocol/
    src/
      planner.rs
      api.rs
  hyperindex-daemon/
    src/
      planner.rs
      handlers.rs
  hyperindex-cli/
    src/
      commands/
        query.rs

bench/
  hyperbench/
    adapter.py
    compare.py

tests/
  test_daemon_planner_adapter.py
```

Recommended ownership:

- `hyperindex-planner`
  - intent classification
  - unified query IR
  - route planning
  - per-engine score normalization
  - fusion, dedupe, grouping
  - trust payloads and planner traces
  - optional exact-route boundary
- `hyperindex-protocol`
  - planner request and response types only
- `hyperindex-daemon`
  - planner query service that composes existing engine services directly
- `hyperindex-cli`
  - front-door query command
- `bench/hyperbench`
  - planner adapter and additive compare metrics only

## Integration Points

### Phase 2 daemon and snapshot system integration

Phase 7 should integrate with the daemon exactly once, through a new planner query method.

Final recommendation:

- add one new public request/response pair:
  - `PlannerQueryParams`
  - `PlannerQueryResponse`
- add one new protocol envelope method in
  [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
  - `RequestBody::PlannerQuery`
  - `SuccessPayload::PlannerQuery`
- implement one focused daemon service under `crates/hyperindex-daemon/src/planner.rs`
- keep planner execution snapshot-scoped by loading the checked-in `ComposedSnapshot` once per
  request and passing that snapshot into underlying services

What Phase 7 should not add:

- no planner build method
- no planner persistence method
- no second snapshot composition path
- no daemon loopback where the planner calls the daemon through its own transport

Rationale:

- the planner has no independent index to build
- existing daemon status already reports the readiness of the underlying engines
- composing direct Rust service calls preserves determinism and keeps traces debuggable

### CLI integration

Phase 7 should add one front-door query command:

- recommended command:
  `hyperctl query`

Why this is the right CLI shape:

- it matches the product wedge better than a developer-facing `planner` noun
- it preserves `hyperctl symbol`, `hyperctl semantic`, and `hyperctl impact` as explicit operator
  and debugging surfaces
- it lets the planner accept one surface query plus optional route or intent hints for debugging

Recommended Phase 7 CLI rules:

- keep engine-specific commands unchanged
- add optional debug flags for:
  - `--intent-hint`
  - `--limit`
  - `--path-glob`
  - `--json`
  - `--include-trace`
- keep trace rendering deterministic and template-based

### Phase 3 exact-search system integration

Because no checked-in exact-search runtime exists, Phase 7 must plan for exact search without
claiming it is available.

Candidate A: no exact route at all

- simplest today
- wrong architectural signal
- would make future exact integration invasive

Candidate B: planner calls a future exact daemon method directly by name

- works once a method exists
- couples Phase 7 to a public exact transport that does not exist yet

Candidate C: internal exact-route provider boundary with an unavailable implementation today

- preserves the ownership boundary
- keeps the exact route explicit in traces and budgets
- avoids inventing a public exact daemon contract in this phase

Final recommendation:

- use Candidate C
- define an internal exact-route interface in `hyperindex-planner`
- ship a default `UnavailableExactRouteProvider` in the current repo
- planner traces must record:
  - `route_kind = exact`
  - `available = false`
  - `skip_reason = exact_engine_unavailable`

### Phase 4 symbol query system integration

Phase 7 should use the existing symbol engine as the primary route for structured code navigation.

Primary integration points:

- `ParserSymbolService::search(...)`
- `ParserSymbolService::show(...)`
- `ParserSymbolService::definitions(...)`
- `ParserSymbolService::references(...)`
- `ParserSymbolService::resolve(...)`

Recommended symbol-route responsibilities:

- identifier lookup
- canonical definition discovery
- symbol-backed dedupe keys
- impact seed resolution when the target looks like a symbol
- supporting evidence for semantic or hybrid queries

Phase 7 must preserve:

- current `SymbolId` identity
- current `GraphEdgeKind` meanings
- current repo-scoped symbol search path

### Phase 5 impact system integration

Phase 7 should use the current impact engine only for impact-intent queries and only after the
planner has a deterministic target.

Candidate A: planner may guess one best target and call impact immediately

- lowest latency
- risks fabricated blast-radius answers
- violates evidence-first behavior when seed resolution is ambiguous

Candidate B: planner may call impact on multiple possible targets and merge the results

- more recall
- high latency and high ambiguity
- makes traces and trust worse

Candidate C: planner resolves one unique checked-in target first, otherwise returns ambiguity

- preserves impact as the source of truth
- keeps output explainable
- avoids speculative blast-radius claims

Final recommendation:

- use Candidate C
- only call `ImpactService::analyze(...)` after resolving exactly one supported target
- if multiple viable symbol or file seeds remain after bounded resolution, return an
  `ambiguous_target` planner result instead of guessing

### Phase 6 semantic system integration

Phase 7 should use the current semantic engine as the primary route for behavior-oriented natural
language queries and as a fallback corroboration route for unresolved lookup queries.

Primary integration points:

- `SemanticService::query(...)`
- existing `SemanticQueryFilters`
- existing `SemanticRetrievalExplanation`
- existing `SemanticChunkMetadata`

Recommended semantic-route responsibilities:

- natural-language behavior lookup
- fallback candidate generation when symbol lookup is sparse
- supporting evidence for impact seed resolution when symbol matches are ambiguous
- supporting evidence for grouped planner trust payloads

Phase 7 must preserve:

- semantic reranking remains owned by the semantic engine
- semantic explanations remain deterministic evidence, not generated prose
- planner does not bypass or replace the Phase 6 query service

### Benchmark harness integration

Phase 7 must integrate planner-mode evaluation into the Phase 1 harness without changing the
public harness contract incompatibly.

Current audited harness reality:

- the runner already supports additive metric rows
- compare extraction is metric-name based and can grow additively
- there is no planner-specific query schema today

Candidate A: add a new top-level `planner` query type immediately

- planner-native
- widens the Phase 1 schema surface right away
- requires new packs and goldens before planner behavior is even measurable

Candidate B: replace existing `daemon`, `daemon-impact`, or `daemon-semantic` adapters

- minimal file count
- destroys per-engine baselines
- muddies compatibility review

Candidate C: add a new `daemon-planner` adapter that reuses current packs first

- preserves per-engine baselines
- preserves current pack schemas
- lets the planner be compared against current symbol, semantic, and impact expectations

Final recommendation:

- use Candidate C
- add `daemon-planner` as a new harness adapter
- keep the current exact, symbol, semantic, and impact packs and goldens unchanged for the first
  planner slice

Recommended first planner-eval behavior over existing packs:

- exact pack:
  - optional in the current repo because no exact engine exists
  - planner should still classify exact intent and emit exact-route unavailability traces
- symbol pack:
  - planner input is the symbol text
  - expected intent is `symbol_lookup`
- semantic pack:
  - planner input is the existing semantic query text
  - expected intent is `semantic_lookup`
- impact pack:
  - planner input is a deterministic surface template derived from the structured impact query
  - expected intent is `impact_analysis`

Recommended additive planner metrics:

- `planner-classification-latency`
- `planner-fusion-latency`
- `planner-route-count`
- `planner-fallback-count`
- `planner-budget-exhausted-count`
- `planner-intent-accuracy`
- `planner-primary-route-pass-rate`
- `planner-no-answer-count`

## Query IR Candidates And Final Recommendation

### Candidate A: Thin tagged union over current engine params

- easy to implement
- preserves existing request shapes exactly
- too weak for ambiguity handling, route budgets, and unified traces

### Candidate B: Freeform planner JSON blob

- flexible
- easy to evolve quickly
- poor for determinism, validation, and benchmark-driven work

### Candidate C: Typed planner IR above engine-specific subrequests

- explicit enough for route planning and traces
- lets Phase 7 stay additive to current engine contracts
- deterministic and testable

### Final Recommendation

Use Candidate C.

Recommended IR shape:

- `PlannerSurfaceQuery`
  - raw text
  - optional explicit intent hint
  - optional path globs
  - requested limit
  - include-trace flag
- `PlannerIntent`
  - `exact_lookup`
  - `symbol_lookup`
  - `semantic_lookup`
  - `impact_analysis`
  - `hybrid_lookup`
  - `ambiguous`
- `PlannerTargetHints`
  - candidate symbol text
  - candidate path text
  - optional impact change hint
  - optional normalized identifier tokens
- `PlannerRouteRequest`
  - route kind
  - engine-local params
  - route budget
  - route priority
- `PlannerEvidenceRequirements`
  - required provenance kinds
  - whether impact must have reason paths
  - whether grouping needs symbol or span anchors

Why this is the right IR:

- it gives the planner one stable internal representation
- it preserves the underlying engine contracts as subrequests rather than rewriting them
- it provides enough structure for deterministic traces, no-answer states, and harness evaluation

## Intent-Routing Candidates And Final Recommendation

### Candidate A: keyword-only classification

- cheap
- easy to explain
- too brittle for symbol-like identifiers and mixed behavior queries

### Candidate B: source-query-type-driven classification only

- works in the harness
- does not solve the real Phase 7 front-door problem
- cannot classify one freeform CLI or daemon query

### Candidate C: deterministic feature-based classification with explicit override and ambiguity band

- still deterministic
- handles real surface queries
- can be benchmarked against the current typed packs by comparing predicted intent to expected pack
  type

### Final Recommendation

Use Candidate C.

Recommended classification signals:

- exact-intent signals:
  - quoted strings
  - regex-like delimiters
  - path separators or filename extensions
  - path glob syntax
- symbol-intent signals:
  - identifier casing
  - dot-qualified or hash-qualified names
  - words like `definition`, `reference`, `caller`, `method`, `class`, `symbol`
- semantic-intent signals:
  - natural-language `where`, `how`, `which file`, `implementation`
  - multi-token behavior descriptions without a clear identifier
- impact-intent signals:
  - `what breaks`
  - `blast radius`
  - `who depends on`
  - `if I rename`, `if I delete`, `if I change`

Override rules:

- explicit CLI or harness hints win over classification heuristics
- if the top two intent scores are too close, classify as `hybrid_lookup` or `ambiguous`
- impact classification without a seed target is allowed, but impact execution is not allowed until
  seed resolution succeeds uniquely

## Route-Planning Policy Candidates And Final Recommendation

### Candidate A: one fixed route per intent

- simplest
- lowest overhead
- weak fallback behavior

### Candidate B: fan out to every engine every time

- high recall
- worst latency
- poor debuggability and duplicated evidence

### Candidate C: primary route plus bounded fallback graph with per-engine budgets

- explicit and testable
- keeps latency bounded
- supports ambiguity and capability gaps

### Final Recommendation

Use Candidate C.

Recommended route graph:

- `exact_lookup`
  - primary:
    exact if available
  - fallback:
    symbol, then semantic
- `symbol_lookup`
  - primary:
    symbol
  - fallback:
    exact if available for path-like tokens, then semantic
- `semantic_lookup`
  - primary:
    semantic
  - fallback:
    symbol, then exact if available for quoted or path-like subterms
- `impact_analysis`
  - primary:
    symbol seed resolution
  - fallback:
    semantic seed resolution
  - final:
    impact analyze on one uniquely resolved symbol or file target
- `hybrid_lookup`
  - primary:
    symbol and semantic in parallel
  - optional:
    exact when capability is present and budget remains

Recommended first budget policy:

- planner classification plus IR build:
  - target budget `<= 5 ms`
- planner fusion plus grouping:
  - target budget `<= 15 ms`
- exact route:
  - target budget `<= 60 ms`
- symbol route:
  - target budget `<= 90 ms`
- semantic route:
  - target budget `<= 220 ms`
- impact route after seed resolution:
  - target budget `<= 330 ms`
- maximum executed routes per query:
  - `2` for lookup queries
  - `3` for impact queries that require seed resolution plus impact analyze

## Score-Normalization / Fusion Candidates And Final Recommendation

### Candidate A: raw engine scores only

- minimal implementation
- scores are not comparable across engines
- encourages route bias from whichever engine returns the largest integer

### Candidate B: learned calibration and fusion

- potentially higher quality later
- too early for this repo
- poor fit for deterministic benchmark-first delivery

### Candidate C: deterministic engine-calibrated score bands plus rank decay and evidence bonuses

- comparable across engines
- deterministic
- transparent enough for traces

### Final Recommendation

Use Candidate C.

Recommended normalization model:

- normalize planner scores into one integer band such as `0..=1000`
- assign engine-local score bands before rank decay:
  - exact:
    `900..1000` when available
  - symbol:
    `800..950` for exact or canonical identifier matches
  - semantic:
    `650..900` for reranked semantic hits
  - impact:
    `500..1000`, with certainty-tier bands:
    - `certain` -> `900..1000`
    - `likely` -> `700..850`
    - `possible` -> `500..650`
- apply deterministic decay by returned rank within the same route
- add small bounded bonuses for:
  - primary-route affinity
  - multiple corroborating routes
  - richer evidence anchors such as symbol id plus span
- apply bounded penalties for:
  - ambiguity
  - missing symbol or span anchors
  - fallback-only routes

Why this is the right first fusion policy:

- it keeps ordering stable and reviewable
- it does not require learning infrastructure
- it lets the planner explain exactly why one group outranked another

## Grouping / Result-Shaping Candidates And Final Recommendation

### Candidate A: flat mixed result list

- smallest payload
- duplicates evidence heavily
- hard to trust

### Candidate B: group strictly by engine

- easy to debug
- poor user-facing shape
- pushes fusion burden onto the user

### Candidate C: group by canonical evidence anchor and attach route-specific support underneath

- evidence-first
- deduplicates cross-engine matches
- preserves route provenance

### Final Recommendation

Use Candidate C.

Recommended grouping keys:

- first choice:
  `symbol_id`
- second choice:
  `path + span`
- impact entities:
  `ImpactEntityRef`
- fallback:
  `path`

Recommended result-group kinds:

- `symbol`
- `file`
- `package`
- `test`
- `ambiguous_target`
- `no_answer`

Recommended result shape:

- one ranked `PlannerResultGroup`
- one representative top candidate
- zero or more supporting candidates from other routes
- one trust payload
- one deterministic short template summary
- concrete provenance anchors for every route contribution

## Trust / Explanation Payload Model And Rationale

### Candidate A: prose summary only

- readable
- not benchmarkable
- violates the evidence-first requirement

### Candidate B: raw passthrough of every engine payload

- complete
- noisy and hard to consume
- weak for grouped planner results

### Candidate C: structured trust payload plus planner trace plus explicit provenance anchors

- evidence-first
- deterministic
- debuggable through the daemon, CLI, and harness

### Final Recommendation

Use Candidate C.

Recommended payload model:

- `PlannerEvidenceAnchor`
  - route kind
  - path
  - optional symbol id
  - optional span
  - source rank
  - source reason code
  - source build or manifest id when available
- `PlannerTrustPayload`
  - `verdict`:
    `direct`, `corroborated`, `fallback_only`, `ambiguous`, `unavailable`
  - deterministic reason codes
  - template summary string built from those reason codes
- `PlannerTrace`
  - classified intent
  - route requests considered
  - executed routes
  - skipped routes with reasons
  - per-route elapsed time
  - fallback triggers
  - budget usage
  - dedupe and suppression actions

Rationale:

- every top-level result remains anchored to underlying engine evidence
- traces make planner behavior reviewable without shipping answer generation
- template summaries give operator usability without hallucination risk

## No-Answer / Ambiguity Policy And Rationale

### Candidate A: always return the best guess

- simple
- highest trust risk
- unacceptable for impact intent

### Candidate B: ask the user to clarify whenever confidence is low

- safe
- too interruptive
- not benchmark-friendly

### Candidate C: bounded multi-route attempt plus explicit `ambiguous` or `no_answer` states

- preserves momentum
- preserves trust
- benchmarkable and deterministic

### Final Recommendation

Use Candidate C.

Recommended policy:

- if no route is available:
  return `no_answer` with `route_unavailable` diagnostics and trace
- if routes execute but produce no evidence-backed hits:
  return `no_answer`
- if multiple seed targets remain plausible for an impact query:
  return `ambiguous_target` groups listing the candidate seeds and do not call impact analyze
- if a budget is exhausted after returning at least one evidence-backed group:
  return the groups plus a `budget_exhausted` trace entry
- never generate a prose answer or blast-radius claim without a concrete engine-backed result

## Validation Matrix

Phase 7 validation must cover all of the following.

### Rust unit and integration coverage

- intent-classification determinism
- query IR normalization
- route selection and fallback behavior
- exact-route unavailability behavior
- normalization, fusion, dedupe, and grouping determinism
- planner trace stability
- daemon planner handler coverage

### Runtime compatibility coverage

- Phase 2 snapshot creation, read-file, and diff behavior still passes
- Phase 4 symbol build and query flows still pass
- Phase 5 impact status, analyze, and explain still pass
- Phase 6 semantic status, build, query, and inspect-chunk still pass

### Harness coverage

- `daemon-planner` smoke run through `hyperbench`
- planner-mode runs over the checked-in symbol, semantic, and impact packs
- additive planner metrics emitted into the existing artifact set
- report and compare outputs remain readable and machine-compatible

### CLI and operator coverage

- `hyperctl query` human-readable output
- `hyperctl query --json` output
- trace rendering for ambiguous and no-answer cases
- deterministic exact-route-unavailable behavior in the current repo

## Performance Targets And How They Will Be Measured

Primary measurement path:

- `hyperbench run --adapter daemon-planner`
- existing Phase 1 report and compare flow
- additive planner metric rows

Recommended first targets on `synthetic-saas-medium`:

- planner classification latency p95:
  `<= 5 ms`
- planner fusion and grouping latency p95:
  `<= 15 ms`
- symbol-intent planner query latency p95:
  `<= 125 ms`
- semantic-intent planner query latency p95:
  `<= 250 ms`
- impact-intent planner query latency p95 when a unique seed is found without fallback:
  `<= 380 ms`
- impact-intent planner query latency p95 when seed resolution requires a semantic fallback:
  `<= 500 ms`
- exact-route-unavailable detection:
  deterministic and sub-`5 ms` in the current repo

Recommended first quality targets:

- planner intent accuracy over current typed packs:
  `>= 0.90` for symbol, semantic, and impact packs
- planner top-hit pass rate:
  no worse than the best underlying route on the same evaluated pack by more than a compatible,
  documented budget
- zero planner results without concrete engine provenance

Measurement notes:

- exact-pack planner quality is not a Phase 7 ship gate in the current repo because the exact
  engine is not checked in
- symbol, semantic, and impact packs are the current shippable planner benchmark surface
- planner traces should be emitted in a way that can be sampled or summarized without changing the
  current artifact names

## Risks And Mitigations

### Risk: the missing exact engine creates pressure to widen Phase 7 scope

Mitigation:

- keep exact as an internal optional route boundary
- ship deterministic unavailable-route traces
- do not add a fake exact daemon API in Phase 7

### Risk: planner classification overfits to current typed benchmark packs

Mitigation:

- use real surface-query heuristics rather than only pack type
- evaluate predicted intent against current typed packs first
- add planner-native query assets only after the base planner path is stable

### Risk: fusion hides which engine actually produced the answer

Mitigation:

- group by anchor but preserve per-route supporting evidence
- keep planner traces explicit
- keep template trust payloads route-aware

### Risk: impact seed resolution guesses incorrectly

Mitigation:

- require one unique resolved seed before calling impact analyze
- return `ambiguous_target` results instead of speculative impact answers
- preserve impact as the authoritative output for impact intent

### Risk: planner fan-out blows latency budgets

Mitigation:

- bound the number of executed routes
- assign explicit per-route budgets
- record fallback counts and budget exhaustion in traces and benchmark metrics

### Risk: planner-mode benchmarking triggers unnecessary schema churn

Mitigation:

- start with `daemon-planner` over existing packs and goldens
- add only additive benchmark metrics first
- defer planner-native pack schema work until current evaluation proves insufficient

## Definition Of Done

Phase 7 is done only when all of the following are true:

1. A real planner layer exists in the Rust workspace on stable Rust.
2. The planner accepts one surface query and produces a typed unified IR deterministically.
3. The planner can classify intent and choose bounded routes across the checked-in symbol,
   semantic, and impact engines plus an optional exact capability boundary.
4. Exact-route absence is handled deterministically in the current repo without pretending an exact
   engine exists.
5. Planner results are fused, deduplicated, grouped, and backed by concrete engine evidence with
   file or symbol provenance.
6. Planner traces are machine-readable and deterministic.
7. The daemon exposes a planner query method and the CLI exposes one front-door query command.
8. Planner-mode evaluation runs through the existing harness as a backward-compatible extension.
9. Phase 1, Phase 2, Phase 4, Phase 5, and Phase 6 compatibility validations still pass.
10. Docs, decisions, and status files are updated with results, risks, and next steps.

## Assumptions That Do Not Require User Input Right Now

- Phase 7 planner runtime code should be written in Rust, consistent with the checked-in runtime
  workspace.
- The first public planner surface should be one query method, not a build or persistence API.
- `hyperctl query` is the right CLI front door for the planner.
- Exact search remains an optional capability boundary in the current repo because no checked-in
  exact engine exists.
- The first planner benchmark slice should reuse the current typed query packs and goldens instead
  of introducing a new schema immediately.
- Impact seed resolution in the first planner slice should resolve only to checked-in `symbol` and
  `file` targets.
- Explanations and summaries must remain deterministic and template-based; no freeform answer
  generation is required or desired in this phase.
