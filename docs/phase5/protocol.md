# Repo Hyperindex Phase 5 Protocol Contract

This document defines the public Phase 5 transport contract for impact analysis.

Scope:

- contract plus the current live analyze surface
- small, phase-appropriate API
- no semantic or typechecker-grade certainty claims

The Rust source of truth is:

- [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/impact.rs)
- [api.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/api.rs)
- [config.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/config.rs)
- [errors.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/errors.rs)

## Conventions

- Every impact request is snapshot-scoped and must include both `repo_id` and `snapshot_id`.
- The contract describes evidence-first impact analysis over existing Phase 4 symbol facts.
- `certain` means explicit syntax-derived evidence only. It does not mean compiler-verified truth.
- Public build/warm/rebuild methods are intentionally omitted. Persisted enrichment, if present, is
  an internal optimization surfaced only through status/manifest metadata.

## Public Methods

### `impact_status`

Use this to discover the current contract state for a repo/snapshot pair.

Returns:

- readiness state
- method capabilities
- supported target kinds
- supported change scenarios
- supported result kinds
- certainty tiers
- optional impact manifest and storage metadata
- diagnostics

If `impact.enabled = false` in runtime config:

- `impact_status` still returns successfully
- `state` is `not_ready`
- `analyze` and `explain` capabilities are `false`
- diagnostics include `impact_disabled`

### `impact_analyze`

Use this to request ranked impact results for one target and one change scenario.

Request supports optional cutoff-tightening overrides for:

- max transitive depth
- max visited nodes
- max traversed edges
- max considered candidates

Returns:

- analyzed target
- requested change scenario
- grouped impact hits
- certainty counts
- traversal stats
- optional impact manifest
- diagnostics

If `impact.enabled = false`, this method returns `config_invalid`.

When a persisted impact build is present, the manifest can also expose additive refresh metadata:

- `refresh_mode`
- optional `fallback_reason`
- optional `refresh_stats`
  - `mode`
  - `trigger`
  - `files_touched`
  - `entities_recomputed`
  - `edges_refreshed`
  - `elapsed_ms`
- `loaded_from_existing_build`

If the symbol prerequisites are ready but no persisted impact build exists yet:

- `impact_status` stays `ready`
- `manifest` is absent
- diagnostics include `impact_build_missing`
- the first successful `impact_analyze` materializes the build and then echoes its manifest

### `impact_explain`

Use this to request reason-path evidence for one impacted entity.

Returns:

- the original target
- the impacted entity being explained
- certainty and direct/transitive flags
- one or more typed reason paths
- diagnostics

If `impact.enabled = false`, this method returns `config_invalid`.

## Deferred Methods

The public contract does not expose:

- `impact_build`
- `impact_warm`
- `impact_rebuild`

Rationale:

- the design may use a persisted store, but build materialization is still an internal lifecycle
  concern
- exposing it now would overfit the scaffold before the real engine proves which operator controls
  are necessary

## Request And Response Envelope

Impact methods use the existing daemon envelope from Phase 2 and Phase 4.

Request:

```json
{
  "protocol_version": "repo-hyperindex.local/v1",
  "request_id": "req-impact-analyze-001",
  "method": "impact_analyze",
  "params": {}
}
```

Success:

```json
{
  "protocol_version": "repo-hyperindex.local/v1",
  "request_id": "req-impact-analyze-001",
  "status": "success",
  "method": "impact_analyze",
  "result": {}
}
```

Error:

```json
{
  "protocol_version": "repo-hyperindex.local/v1",
  "request_id": "req-impact-explain-001",
  "status": "error",
  "method": "impact_explain",
  "error": {}
}
```

Concrete fixtures live in
[examples.json](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/fixtures/api/examples.json).

## Supported Input Targets In This Slice

Supported:

- `symbol`
- `file`

Deferred:

- `config_key`
- `route`
- API/endpoint targets
- `package`
- arbitrary AST node or text-span targets

Why they are deferred:

- the current checked-in fact model does not yet expose conservative first-class config-key,
  route, or API anchors
- adding them in this contract slice would reopen Phase 4 fact-model scope

File targets are included because they are already phase-appropriate and map cleanly to the
existing snapshot and symbol ownership model.

## Supported Change Scenarios

Phase 5 exposes these deterministic change scenarios:

- `modify_behavior`
- `signature_change`
- `rename`
- `delete`

These are request hints, not guarantees of semantic precision.

The current implementation applies deterministic per-scenario traversal policies and bounded
cutoffs to those scenarios.

## Result Shape

`impact_analyze` groups hits by:

- direct vs transitive
- certainty tier
- impacted entity kind

Each `ImpactHit` contains:

- stable rank and score
- a typed impacted entity reference
- the primary reason edge kind
- path depth
- direct/transitive flag
- one additive explanation payload:
  - `why`
  - `change_effect`
  - `primary_path`
- optional reason paths

`impact_analyze` also returns traversal stats for:

- nodes visited
- edges traversed
- depth reached
- candidates considered
- elapsed time
- triggered cutoffs

The build-refresh metadata stays on the manifest instead of widening the public method surface
with explicit `impact_build` or `impact_rebuild` operations.

`impact_explain` returns the same reason-path model without requiring callers to reparse analyze
results ad hoc.

## Status Shape

`impact_status` reports:

- `state`
  - `not_ready`
  - `ready`
  - `stale`
- `capabilities`
  - `status`
  - `analyze`
  - `explain`
  - `materialized_store`
- optional `manifest`

In the current implementation:

- `impact_status` returns typed contract metadata
- `impact_status` does not fabricate persisted-build metadata before a build exists
- `impact_analyze` is daemon-backed and returns real bounded results
- `impact_explain` is daemon-backed and returns stable reason paths for one impacted entity

## Error Contract

Impact errors still use the shared `ProtocolError` envelope.

Phase 5 adds these machine-readable impact codes:

- `impact_not_ready`
- `impact_target_not_found`
- `impact_target_unsupported`
- `impact_result_not_found`

Impact-specific subjects are also typed:

- `impact_build`
- `impact_target`
- `impact_result`

Callers should treat `payload.subject`, `payload.validation`, and `payload.context` as the stable
machine-readable detail channel instead of parsing `message`.

## Compatibility Rule

The Phase 5 impact contract may add optional fields and enum variants.

It should not:

- remove or rename the public impact methods defined here
- widen `certain` into a semantic-confidence claim the implementation cannot prove
- silently add first-class config, route, or API targets without a documented fact-model slice
- expose a persisted-store lifecycle API unless the real engine proves it is necessary
