# Repo Hyperindex Phase 5 Impact Model

This document defines the typed public data model for Phase 5 impact analysis.

The source of truth is
[impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/impact.rs).

## Design Goals

- keep the model explicit enough to implement deterministically
- preserve the Phase 4 symbol graph as the source of syntax-derived evidence
- avoid promises that require semantic resolution or compiler participation
- keep future widening additive

## Target References

Input targets are modeled as `ImpactTargetRef`.

Supported variants:

- `symbol`
  - fields:
    - `value`
    - optional `symbol_id`
    - optional `path`
  - use for a user-facing symbol selector or a previously resolved symbol identity
- `file`
  - fields:
    - `path`
  - use for a repo-relative file target

The selector remains string-friendly for CLI and harness callers, but the type preserves room for
resolved ids when they exist.

## Supported Target Kinds

`ImpactTargetKind` currently includes only:

- `symbol`
- `file`

Deferred kinds are documented, not encoded as supported enum variants in this slice.

## Supported Change Scenarios

`ImpactChangeScenario` currently includes:

- `modify_behavior`
- `signature_change`
- `rename`
- `delete`

These are deterministic scenario labels used to steer traversal and ranking policy later. They are
not a claim that the engine understands semantics beyond the checked-in fact model.

## Certainty Tiers

`ImpactCertaintyTier` exposes three ordered buckets:

- `certain`
  - only explicit syntax-derived evidence
- `likely`
  - deterministic one-step expansion from explicit evidence
- `possible`
  - conservative fallback expansion where adjacency exists but direct consumption is not proven

The contract intentionally keeps certainty as a tier, not a float confidence value.

Deterministic tier mapping in the current engine:

- `certain`
  - the selected target itself
  - symbol/file ownership edges such as `declared_in_file` and `contains`
  - explicit symbol consumer edges such as `references`, `imports`, and `exports`
  - explicit test evidence such as symbol references or direct file imports
  - direct consumer files for `signature_change`, `rename`, and `delete`
- `likely`
  - package expansion from already-impacted symbols or files
  - direct consumer files for `modify_behavior`
  - deterministic heuristic test affinity for `signature_change`
- `possible`
  - deterministic heuristic test affinity for `modify_behavior`, `rename`, and `delete`

Path certainty is conservative:

- compute certainty per traversed edge
- combine a multi-edge path by keeping the weakest tier seen on that path
- collapse duplicate hits to the best-ranked path, not an average or blended score

## Impacted Entity Kinds

`ImpactEntityRef` and `ImpactedEntityKind` currently support result entities for:

- `symbol`
- `file`
- `package`
- `test`

Package and test are output kinds only in this slice. They are not first-class input targets yet.

## Reason Paths And Evidence Edges

Non-self impact explanations use:

- `ImpactReasonPath`
  - `summary`
  - `edges`
- `ImpactHitExplanation`
  - `why`
  - `change_effect`
  - `primary_path`
- `ImpactEvidenceEdge`
  - `edge_kind`
  - `from`
  - `to`
  - optional `metadata`

`ImpactReasonEdgeKind` currently covers deterministic evidence categories only:

- `seed`
- `contains`
- `defines`
- `references`
- `imports`
- `exports`
- `declared_in_file`
- `referenced_by_file`
- `imported_by_file`
- `package_member`
- `test_reference`
- `test_affinity`

This set is intentionally evidence-shaped. It avoids inferred call-graph or data-flow claims.

`ImpactHitExplanation` is intentionally narrow and deterministic:

- `why`
  - one inspectable sentence derived from the selected primary evidence edge
- `change_effect`
  - one inspectable sentence derived from the requested `ImpactChangeScenario`
- `primary_path`
  - the exact typed reason path used for ranking and explanation

Every returned `ImpactHit` now carries an explanation, even when the caller disables the larger
`reason_paths` array. This keeps the user-facing blast-radius output explainable without requiring
semantic summary generation.

## Stable Ranking

`ImpactHit` ordering is deterministic across runs.

The current engine ranks hits using these factors, in order:

- certainty tier
- shortest selected reason path
- primary edge-type priority
- target proximity in the traversed graph
- direct test relevance when the entity is a test
- path/package proximity to the requested target when a path-like anchor exists
- stable score and entity identity tie-breakers

Direct test relevance is conservative:

- explicit symbol-reference evidence outranks file-import evidence
- file-import evidence outranks heuristic test affinity

Path/package proximity is phase-appropriate:

- symbols, files, and tests use repo-relative paths
- packages use `package_root`
- the engine prefers smaller lexical distance to the requested target when all stronger ranking
  factors tie

## Result Groups

`ImpactAnalyzeResponse` returns grouped hits through `ImpactResultGroup`.

Each group is keyed by:

- `direct`
- `certainty`
- `entity_kind`

This lets future implementations:

- rank within a stable bucket
- preserve direct/transitive distinctions
- expose package and test output without inventing separate top-level response arrays

## Summary And Diagnostics

`ImpactSummary` contains:

- `direct_count`
- `transitive_count`
- `certainty_counts`

`ImpactTraversalStats` contains:

- `nodes_visited`
- `edges_traversed`
- `depth_reached`
- `candidates_considered`
- `elapsed_ms`
- `cutoffs_triggered`

`ImpactDiagnostic` contains:

- `severity`
- `code`
- `message`

Diagnostics are for explanatory notes, not transport failures. Transport failures remain in the
shared `ProtocolError` envelope.

## Config And Policy Knobs

`RuntimeConfig.impact` adds a small Phase 5 config surface:

- `enabled`
- `store_dir`
- `materialization_mode`
- `default_limit`
- `max_limit`
- `default_include_transitive`
- `default_include_reason_paths`
- `max_reason_paths_per_hit`
- `max_transitive_depth`
- `include_possible_results`

These are output-shaping and materialization-policy knobs only. They do not promise a specific
graph algorithm.

In the checked-in Phase 5 daemon path:

- `include_possible_results`
  - filters `possible` groups from the returned analyze result
- `max_reason_paths_per_hit`
  - caps returned `reason_paths` for analyze/explain while leaving the typed explanation payload
    intact
- `materialization_mode`
  - is surfaced in runtime and manifest metadata
  - `prefer_persisted` is the validated Phase 5 mode

`ImpactAnalyzeParams` also exposes optional per-request tightening overrides for:

- `max_transitive_depth`
- `max_nodes_visited`
- `max_edges_traversed`
- `max_candidates_considered`

These overrides are intentionally one-way. They can narrow the approved scenario policy, but they
must not widen it past the conservative defaults encoded by the engine.

## Graph Enrichment Metadata

`ImpactGraphEnrichmentMetadata` tracks optional derived layers when a build or status result wants
to surface them.

Kinds currently modeled:

- `canonical_alias`
- `file_adjacency`
- `package_membership`
- `test_affinity`

States currently modeled:

- `available`
- `deferred`

The contract does not require every enrichment to exist for every repo or snapshot.

## Manifest And Persistence Metadata

If persisted enrichment exists, it is summarized by `ImpactManifest`:

- `build_id`
- `repo_id`
- `snapshot_id`
- optional `symbol_index_build_id`
- `created_at`
- `enrichments`
- optional `storage`

`ImpactStorageMetadata` currently exposes:

- `format`
- `path`
- `schema_version`
- `materialization_mode`

This is enough for status/debug/reporting without freezing an operator-facing build API.

## Explicit Deferrals

This contract slice explicitly defers:

- config-key targets
- route targets
- API/endpoint targets
- public build/warm/rebuild methods
- semantic edge kinds
- certainty tiers stronger than syntax-derived evidence can support

Those can be added later, but only with a documented fact-model or implementation slice.

## Current Known Limits

The checked-in implementation is stable enough for Phase 5 operator use, but these limits remain
important:

- First-class input targets are still `symbol` and `file` only.
- `config_key`, route, and API/endpoint targets remain documented deferrals, not partially
  supported features.
- `impact_status` can be `ready` without a manifest when the symbol prerequisite is present but no
  persisted impact build has been materialized yet.
- `impact_explain` still recomputes through the normal analyze path instead of answering from a
  dedicated stored explain index.
- `hyperctl impact doctor`, `impact stats`, and `impact rebuild` are CLI-local operator commands;
  the daemon protocol still exposes only `impact_status`, `impact_analyze`, and
  `impact_explain`.
- The daemon-backed smoke benchmark is operational and incremental refresh is clean, but the
  checked-in impact adapter still misses fixture parity on current query-pass-rate and some
  latency budgets.
