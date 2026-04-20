# Repo Hyperindex Phase 5 Impact Index Format

This document records the persisted impact-materialization shape that the public contract is
allowed to reference.

The source of truth for the public metadata is:

- [impact.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/impact.rs)
- [config.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-protocol/src/config.rs)
- [impact_store.rs](/Users/rishivinodkumar/RepoHyperindex/crates/hyperindex-impact-store/src/impact_store.rs)

## What Is Stable In This Slice

The contract now reserves a dedicated impact-store location under the runtime root:

```text
.hyperindex/data/impact/<repo_id>/impact.sqlite3
```

`RuntimeConfig.impact` points at the parent directory through `impact.store_dir`.

The public manifest/storage metadata currently treats the persisted format as:

- `format = sqlite`
- `path = .hyperindex/data/impact/<repo_id>/impact.sqlite3`
- `schema_version`
- `materialization_mode`

## What The Persisted Store Is For

If present, the store is intended only for rebuildable impact metadata such as:

- impact build identity
- source snapshot identity
- source symbol-index build identity
- direct graph-enrichment metadata
- status/debug counters

The store is not a source of truth independent from:

- the Phase 2 snapshot model
- the Phase 4 symbol graph

## What Is Explicitly Not Stable Yet

This contract slice does not freeze:

- sqlite table names
- column names
- migration order beyond the surfaced `schema_version`
- internal row layouts
- indexes or query plans
- cache eviction rules

Those details remain implementation-private until a real materializer exists.

## Public Lifecycle Rule

The public API does not expose build, warm, or rebuild methods yet.

Instead:

- `impact_status` surfaces an `ImpactManifest` only when a persisted build already exists
- `impact_analyze` may echo the manifest used for a query
- a ready snapshot without a stored build reports `impact_build_missing` and materializes on the
  first analyze call in the default Phase 5 path
- `materialization_mode` is surfaced in runtime/manifest metadata, but the checked-in Phase 5
  validation path is `prefer_persisted`

This keeps the contract compatible with both:

- live computation over existing stores
- future persisted direct-enrichment reuse

## Current Implementation State

As of 2026-04-13:

- the dedicated impact-store crate owns a real SQLite schema with an `impact_builds` table
- the daemon persists one snapshot-scoped `StoredImpactBuild` JSON blob per repo/snapshot
- the stored build includes:
  - snapshot and repo identity
  - impact-config digest
  - schema version
  - refresh mode and fallback reason
  - refresh stats
  - the materialized enrichment plan plus file-scoped contributions
- runtime status scans stored builds and classifies them as ready or stale against the current
  symbol index and impact config

What is still intentionally not frozen:

- table/index details beyond the surfaced `schema_version`
- build JSON layout beyond the protocol-visible manifest fields and durability behavior
- a separately validated `live_only` operator mode
