# Repo Hyperindex Phase 5 Handoff

## What Phase 5 Built

Phase 5 shipped the first real local impact-analysis engine for Repo Hyperindex on top of the
existing snapshot/runtime and symbol-graph stack.

The checked-in implementation now includes:

- deterministic `symbol` and `file` target resolution
- bounded direct and transitive impact traversal for:
  - `modify_behavior`
  - `signature_change`
  - `rename`
  - `delete`
- ranked `symbol`, `file`, `package`, and `test` impact outputs
- explicit `certain` / `likely` / `possible` tiers with typed reason paths and explanation payloads
- derived enrichments for:
  - canonical alias collapse
  - reverse references / imports / exports
  - reverse file dependents
  - package membership
  - test affinity
- persisted impact builds plus incremental refresh from `SnapshotDiffResponse`, including
  buffer-only snapshots
- daemon and CLI integration for:
  - `impact_status`
  - `impact_analyze`
  - `impact_explain`
  - local operator flows: `impact doctor`, `impact stats`, `impact rebuild`
- Phase 1 harness integration through the daemon-backed impact adapter

## Intentionally Out Of Scope

Phase 5 still does not ship:

- semantic retrieval, embeddings, reranking, or answer generation
- a checked-in exact-search engine
- compiler-grade TypeScript semantic resolution
- inferred call graphs, inheritance graphs, or data-flow reasoning sold as exact impact
- codemods or any write-side editor workflow
- a VS Code extension or browser UI
- cloud, team-sharing, or multi-user runtime behavior
- native `config_key` / route / API target support

Important closeout boundary:

- the checked-in harness preserves config-backed benchmark compatibility by degrading
  `config_key` queries to their backing files in
  [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  via `_impact_request_payload(...)`
- Phase 6 should remove that degrade only after adding a real fact-model slice for config anchors

## Phase 6 Plug-In Interfaces

### Impact result retrieval

- Protocol:
  [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/impact.rs)
  - `ImpactAnalyzeParams`
  - `ImpactAnalyzeResponse`
  - `ImpactHit`
  - `ImpactResultGroup`
  - `ImpactManifest`
- Library:
  [lib.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-impact/src/lib.rs)
  - `ImpactWorkspace::analyze_with_snapshot(...) -> ImpactResult<ImpactAnalyzeResponse>`
  - `ImpactWorkspace::analyze_with_enrichment(...) -> ImpactResult<ImpactAnalyzeResponse>`
- Daemon:
  [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/impact.rs)
  - `ImpactService::analyze(...) -> Result<ImpactAnalyzeResponse, ProtocolError>`
- Persisted retrieval/debug seam:
  [impact_store.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-impact-store/src/impact_store.rs)
  - `ImpactStore::load_build(snapshot_id) -> ImpactStoreResult<Option<StoredImpactBuild>>`
  - `StoredImpactBuild.state.plan`

Phase 6 should keep `ImpactAnalyzeResponse` as the transport read model unless a compatibility
break is explicitly approved.

### Certainty tiers and reason paths

- Protocol:
  [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/impact.rs)
  - `ImpactCertaintyTier`
  - `ImpactReasonEdgeKind`
  - `ImpactReasonPath`
  - `ImpactHitExplanation`
  - `ImpactExplainParams`
  - `ImpactExplainResponse`
- Engine:
  [impact_engine.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-impact/src/impact_engine.rs)
  - traversal, ranking, tier assignment, explanation construction
- Daemon:
  [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/impact.rs)
  - `ImpactService::explain(...)`
  - daemon-side enforcement of:
    - `include_possible_results`
    - `max_reason_paths_per_hit`

Phase 6 can widen path richness, but it should keep the existing typed certainty and reason-path
shapes additive.

### Graph enrichment access

- Enrichment builder:
  [impact_enrichment.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-impact/src/impact_enrichment.rs)
  - `ImpactEnrichmentPlanner::build(graph, snapshot) -> ImpactEnrichmentPlan`
- Enrichment read model:
  [impact_enrichment.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-impact/src/impact_enrichment.rs)
  - `ImpactEnrichmentPlan`
  - important Phase 6 fields:
    - `canonical_symbol_by_symbol`
    - `aliases_by_canonical_symbol`
    - `reverse_references_by_symbol`
    - `reverse_reference_files_by_symbol`
    - `reverse_imports_by_symbol`
    - `reverse_exports_by_symbol`
    - `reverse_dependents_by_file`
    - `symbol_to_file`
    - `file_to_symbols`
    - `package_by_file`
    - `package_by_symbol`
    - `tests_by_file`
    - `tests_by_symbol`
    - `metadata`
- Persisted enrichment access:
  [impact_store.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-impact-store/src/impact_store.rs)
  - `StoredImpactBuild.state.plan`

Phase 6 should extend `ImpactEnrichmentPlan` rather than mutating the meaning of Phase 4 graph
edges.

### Snapshot and file-content access

- Snapshot contract:
  [snapshot.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/snapshot.rs)
  - `ComposedSnapshot`
  - `SnapshotDiffResponse`
  - `SnapshotReadFileResponse`
- Runtime assembler:
  [manifest.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-snapshot/src/manifest.rs)
  - `SnapshotAssembler::resolve_file(...)`
  - `SnapshotAssembler::diff(...)`
- Daemon/runtime seam:
  [state.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/state.rs)
  - `DaemonStateManager::create_snapshot(...)`

Phase 6 should continue to consume file content through `ComposedSnapshot` plus
`SnapshotAssembler`, not by probing repo roots directly.

### Daemon query flow

- Request handling:
  [handlers.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/handlers.rs)
  - `HandlerRegistry::impact_status(...)`
  - `HandlerRegistry::impact_analyze(...)`
  - `HandlerRegistry::impact_explain(...)`
- Daemon service:
  [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-daemon/src/impact.rs)
  - `ImpactService::{status, analyze, explain}`
  - `build_graph_from_store(...)`
- Protocol envelope:
  [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
  - existing `RequestBody` / `SuccessPayload` impact variants

Current behavior worth preserving:

- `impact_status` can be `ready` with `manifest = null` when symbol prerequisites exist but no
  persisted impact build has been materialized yet
- `impact_status` stays available even when `impact.enabled = false`; in that case it reports
  `not_ready`, disables analyze/explain capabilities, and emits `impact_disabled`
- `impact_analyze` materializes or refreshes the build and echoes the manifest used
- `impact_analyze` and `impact_explain` reject requests when `impact.enabled = false`
- `impact_explain` still recomputes through the normal analyze path

### Benchmark harness integration

- Adapter seam:
  [adapter.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/adapter.py)
  - `DaemonImpactAdapter.prepare_corpus(...)`
  - `DaemonImpactAdapter.execute_impact_query(...)`
  - `DaemonImpactAdapter.run_incremental_refresh(...)`
  - `_impact_request_payload(...)`
  - `_normalize_impact_hits(...)`
- Runner/report/compare remain unchanged:
  [runner.py](/Users/rishivinodkumar/RepoHyperindex/bench/hyperbench/runner.py)
  - `run_benchmark(...)`

Phase 6 should keep using the existing adapter seam instead of moving benchmark logic into the
daemon.

## Current Tech Debt And Risks

- `symbol_index_build_id` is still `null` in impact manifests because the impact store does not
  persist a stable Phase 4 symbol-build identity yet.
- `impact_explain` recomputes through `analyze` rather than serving a stored explain lookup.
- `materialization_mode` is surfaced in runtime/manifest metadata, but `prefer_persisted` is the
  only validated Phase 5 mode.
- Native `config_key`, route, and API targets are still absent because the checked-in fact model
  has no first-class anchors or edges for them.
- The daemon-backed impact benchmark is operational and self-validating, but the checked-in smoke
  compare still fails current fixture-relative query-pass-rate and latency budgets.
- Package membership still depends on snapshot-visible `package.json` discovery; there is no
  tsconfig/path-alias or package-exports interpretation in this phase.

## Recommended First Milestones For Phase 6

1. Add a real config-anchor fact-model slice, then ship native `config_key` targets end to end:
   protocol, enrichment, analyze, explain, and harness normalization.
2. Persist and surface the Phase 4 symbol-build identity inside impact builds so manifests can
   prove which graph/enrichment snapshot they were derived from.
3. Decide whether `materialization_mode = live_only` should become a real supported mode or remain
   a reserved policy knob.
4. Improve benchmark parity for the synthetic impact pack before widening target kinds further:
   fix the current `query-pass-rate` miss first, then latency budgets.
5. Only after the above is stable, consider stored explain-path indexing or richer graph
   enrichments.
