use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use hyperindex_config::LoadedConfig;
use hyperindex_impact::{
    ImpactError, ImpactRebuildFallbackReason, ImpactRefreshResult, ImpactWorkspace,
    IncrementalImpactBuilder,
};
use hyperindex_impact_store::{ImpactStore, StoredImpactBuild};
use hyperindex_protocol::config::ImpactConfig;
use hyperindex_protocol::errors::ProtocolError;
use hyperindex_protocol::impact::{
    ImpactAnalysisState, ImpactAnalyzeParams, ImpactAnalyzeResponse, ImpactCapabilities,
    ImpactCertaintyCounts, ImpactCertaintyTier, ImpactChangeScenario, ImpactDiagnostic,
    ImpactDiagnosticSeverity, ImpactExplainParams, ImpactExplainResponse, ImpactManifest,
    ImpactResultGroup, ImpactStatusParams, ImpactStatusResponse, ImpactSummary, ImpactTargetKind,
    ImpactedEntityKind,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::status::ImpactRuntimeStatus;
use hyperindex_repo_store::RepoStore;
use hyperindex_symbol_store::{IndexedSnapshotState, SymbolStore};
use hyperindex_symbols::{SymbolGraph, SymbolGraphBuilder};

#[derive(Debug, Clone)]
struct ImpactBuildOutcome {
    build: StoredImpactBuild,
    loaded_from_existing_build: bool,
}

#[derive(Debug, Default, Clone)]
pub struct ImpactService;

impl ImpactService {
    pub fn status(
        &self,
        impact_store_root: &Path,
        symbol_store_root: &Path,
        impact_config: &ImpactConfig,
        params: &ImpactStatusParams,
    ) -> Result<ImpactStatusResponse, ProtocolError> {
        if !impact_config.enabled {
            return Ok(disabled_status_response(params));
        }
        let store = impact_store(impact_store_root, &params.repo_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let manifest = store.manifest_for(&params.repo_id, &params.snapshot_id, None);
        let indexed_state =
            load_indexed_state(symbol_store_root, &params.repo_id, &params.snapshot_id);
        let (materialized, build_corrupt) = match store.load_build(&params.snapshot_id) {
            Ok(build) => (build, false),
            Err(_) => (None, true),
        };
        let builder = IncrementalImpactBuilder::new(impact_config);
        let ready = indexed_state.is_some();
        let stale = build_corrupt
            || materialized
                .as_ref()
                .map(|build| {
                    build.schema_version != store.schema_version
                        || build.impact_config_digest != builder.config_digest()
                })
                .unwrap_or(false);

        Ok(ImpactStatusResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            state: if !ready {
                ImpactAnalysisState::NotReady
            } else if stale {
                ImpactAnalysisState::Stale
            } else {
                ImpactAnalysisState::Ready
            },
            capabilities: ImpactCapabilities {
                status: true,
                analyze: ready,
                explain: ready,
                materialized_store: true,
            },
            supported_targets: vec![ImpactTargetKind::Symbol, ImpactTargetKind::File],
            supported_change_scenarios: vec![
                ImpactChangeScenario::ModifyBehavior,
                ImpactChangeScenario::SignatureChange,
                ImpactChangeScenario::Rename,
                ImpactChangeScenario::Delete,
            ],
            supported_result_kinds: vec![
                ImpactedEntityKind::Symbol,
                ImpactedEntityKind::File,
                ImpactedEntityKind::Package,
                ImpactedEntityKind::Test,
            ],
            certainty_tiers: vec![
                ImpactCertaintyTier::Certain,
                ImpactCertaintyTier::Likely,
                ImpactCertaintyTier::Possible,
            ],
            manifest: materialized.as_ref().map(|build| {
                impact_manifest(
                    &manifest,
                    Some(build),
                    true,
                    impact_config.materialization_mode.clone(),
                )
            }),
            diagnostics: if !ready {
                vec![ImpactDiagnostic {
                    severity: ImpactDiagnosticSeverity::Warning,
                    code: "impact_not_ready".to_string(),
                    message: "impact analysis needs a ready symbol index for this snapshot"
                        .to_string(),
                }]
            } else if build_corrupt {
                vec![ImpactDiagnostic {
                    severity: ImpactDiagnosticSeverity::Warning,
                    code: "impact_build_corrupt".to_string(),
                    message:
                        "stored impact build is unreadable and will fall back to a full rebuild on analyze"
                            .to_string(),
                }]
            } else if stale {
                vec![ImpactDiagnostic {
                    severity: ImpactDiagnosticSeverity::Warning,
                    code: "impact_build_stale".to_string(),
                    message:
                        "stored impact build is stale and will fall back to a rebuild on analyze"
                            .to_string(),
                }]
            } else if materialized.is_none() {
                vec![ImpactDiagnostic {
                    severity: ImpactDiagnosticSeverity::Info,
                    code: "impact_build_missing".to_string(),
                    message:
                        "impact analysis is ready, but no persisted impact build exists yet; analyze will materialize one on demand"
                            .to_string(),
                }]
            } else {
                vec![ImpactDiagnostic {
                    severity: ImpactDiagnosticSeverity::Info,
                    code: "impact_traversal_ready".to_string(),
                    message:
                        "bounded direct and transitive impact analysis is ready for this snapshot"
                            .to_string(),
                }]
            },
        })
    }

    pub fn analyze(
        &self,
        impact_store_root: &Path,
        repo_store: &RepoStore,
        impact_config: &ImpactConfig,
        graph: &SymbolGraph,
        snapshot: &ComposedSnapshot,
        params: &ImpactAnalyzeParams,
    ) -> Result<ImpactAnalyzeResponse, ProtocolError> {
        ensure_impact_enabled(impact_config, "impact_analyze")?;
        let workspace = ImpactWorkspace::default();
        let build = self.ensure_build(
            impact_store_root,
            repo_store,
            impact_config,
            graph,
            snapshot,
            params,
        )?;
        let mut response = workspace
            .analyze_with_enrichment(graph, Some(snapshot), &build.build.state.plan, params)
            .map_err(map_impact_error)?;
        response = apply_analyze_policy(response, impact_config);
        let manifest = impact_store(impact_store_root, &params.repo_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?
            .manifest_for(
                &params.repo_id,
                &params.snapshot_id,
                build.build.symbol_build_id.as_deref(),
            );
        response.manifest = Some(impact_manifest(
            &manifest,
            Some(&build.build),
            build.loaded_from_existing_build,
            impact_config.materialization_mode.clone(),
        ));
        Ok(response)
    }

    pub fn explain(
        &self,
        impact_store_root: &Path,
        repo_store: &RepoStore,
        impact_config: &ImpactConfig,
        graph: &SymbolGraph,
        snapshot: &ComposedSnapshot,
        params: &ImpactExplainParams,
    ) -> Result<ImpactExplainResponse, ProtocolError> {
        ensure_impact_enabled(impact_config, "impact_explain")?;
        let analyze_params = ImpactAnalyzeParams {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            target: params.target.clone(),
            change_hint: params.change_hint.clone(),
            limit: impact_config.max_limit as u32,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: Some(impact_config.max_transitive_depth),
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        };
        let build = self.ensure_build(
            impact_store_root,
            repo_store,
            impact_config,
            graph,
            snapshot,
            &analyze_params,
        )?;
        let workspace = ImpactWorkspace::default();
        let response = workspace
            .analyze_with_enrichment(
                graph,
                Some(snapshot),
                &build.build.state.plan,
                &analyze_params,
            )
            .map_err(map_impact_error)?;
        let response = apply_analyze_policy(response, impact_config);
        let hit = response
            .groups
            .iter()
            .flat_map(|group| group.hits.iter())
            .find(|hit| hit.entity == params.impacted)
            .ok_or_else(|| {
                ProtocolError::impact_result_not_found(impact_entity_key(&params.impacted))
            })?;

        Ok(ImpactExplainResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            target: params.target.clone(),
            impacted: params.impacted.clone(),
            certainty: hit.certainty.clone(),
            direct: hit.direct,
            reason_paths: hit
                .reason_paths
                .iter()
                .take(
                    (params.max_reason_paths as usize).min(impact_config.max_reason_paths_per_hit),
                )
                .cloned()
                .collect(),
            diagnostics: response.diagnostics,
        })
    }

    fn ensure_build(
        &self,
        impact_store_root: &Path,
        repo_store: &RepoStore,
        impact_config: &ImpactConfig,
        graph: &SymbolGraph,
        snapshot: &ComposedSnapshot,
        params: &ImpactAnalyzeParams,
    ) -> Result<ImpactBuildOutcome, ProtocolError> {
        let store = impact_store(impact_store_root, &params.repo_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let builder = IncrementalImpactBuilder::new(impact_config);

        let current_build = store.load_build(&snapshot.snapshot_id);
        if let Ok(Some(build)) = current_build.as_ref() {
            if build.schema_version == store.schema_version
                && build.impact_config_digest == builder.config_digest()
            {
                return Ok(ImpactBuildOutcome {
                    build: build.clone(),
                    loaded_from_existing_build: true,
                });
            }
        }

        let refresh = match current_build {
            Err(_) => builder.build_full(
                snapshot,
                graph,
                hyperindex_protocol::impact::ImpactRefreshTrigger::Bootstrap,
                Some(ImpactRebuildFallbackReason::CacheOrIndexCorruption),
            ),
            Ok(_) => {
                self.refresh_from_previous(&store, repo_store, &builder, graph, snapshot, params)?
            }
        };
        let stored_build = stored_build(snapshot, &store, &refresh);
        store
            .persist_build(&stored_build)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        Ok(ImpactBuildOutcome {
            build: stored_build,
            loaded_from_existing_build: false,
        })
    }

    fn refresh_from_previous(
        &self,
        store: &ImpactStore,
        repo_store: &RepoStore,
        builder: &IncrementalImpactBuilder,
        graph: &SymbolGraph,
        snapshot: &ComposedSnapshot,
        params: &ImpactAnalyzeParams,
    ) -> Result<ImpactRefreshResult, ProtocolError> {
        let previous =
            previous_build_candidate(repo_store, store, &params.repo_id, &snapshot.snapshot_id)
                .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let Some((previous_snapshot, previous_build)) = previous else {
            return Ok(builder.build_full(
                snapshot,
                graph,
                hyperindex_protocol::impact::ImpactRefreshTrigger::Bootstrap,
                Some(ImpactRebuildFallbackReason::NoPriorSnapshot),
            ));
        };
        if previous_build.schema_version != store.schema_version {
            return Ok(builder.build_full(
                snapshot,
                graph,
                hyperindex_protocol::impact::ImpactRefreshTrigger::SnapshotDiff,
                Some(ImpactRebuildFallbackReason::SchemaVersionChanged),
            ));
        }
        if previous_build.impact_config_digest != builder.config_digest() {
            return Ok(builder.build_full(
                snapshot,
                graph,
                hyperindex_protocol::impact::ImpactRefreshTrigger::SnapshotDiff,
                Some(ImpactRebuildFallbackReason::IncompatibleConfigChange),
            ));
        }
        let diff = hyperindex_snapshot::SnapshotAssembler.diff(&previous_snapshot, snapshot);
        Ok(builder.build_incremental(
            &previous_snapshot,
            snapshot,
            &diff,
            graph,
            &previous_build.state,
        ))
    }
}

fn disabled_status_response(params: &ImpactStatusParams) -> ImpactStatusResponse {
    ImpactStatusResponse {
        repo_id: params.repo_id.clone(),
        snapshot_id: params.snapshot_id.clone(),
        state: ImpactAnalysisState::NotReady,
        capabilities: ImpactCapabilities {
            status: true,
            analyze: false,
            explain: false,
            materialized_store: false,
        },
        supported_targets: vec![ImpactTargetKind::Symbol, ImpactTargetKind::File],
        supported_change_scenarios: vec![
            ImpactChangeScenario::ModifyBehavior,
            ImpactChangeScenario::SignatureChange,
            ImpactChangeScenario::Rename,
            ImpactChangeScenario::Delete,
        ],
        supported_result_kinds: vec![
            ImpactedEntityKind::Symbol,
            ImpactedEntityKind::File,
            ImpactedEntityKind::Package,
            ImpactedEntityKind::Test,
        ],
        certainty_tiers: vec![
            ImpactCertaintyTier::Certain,
            ImpactCertaintyTier::Likely,
            ImpactCertaintyTier::Possible,
        ],
        manifest: None,
        diagnostics: vec![ImpactDiagnostic {
            severity: ImpactDiagnosticSeverity::Warning,
            code: "impact_disabled".to_string(),
            message: "impact analysis is disabled in runtime config".to_string(),
        }],
    }
}

pub fn scan_impact_runtime_status(
    loaded: &LoadedConfig,
) -> hyperindex_core::HyperindexResult<ImpactRuntimeStatus> {
    let store_dir = &loaded.config.impact.store_dir;
    if !store_dir.exists() {
        return Ok(ImpactRuntimeStatus {
            enabled: loaded.config.impact.enabled,
            store_dir: store_dir.display().to_string(),
            materialization_mode: loaded.config.impact.materialization_mode.clone(),
            repo_count: 0,
            materialized_snapshot_count: 0,
            ready_build_count: 0,
            stale_build_count: 0,
        });
    }

    let builder = IncrementalImpactBuilder::new(&loaded.config.impact);
    let mut repo_count = 0usize;
    let mut materialized_snapshot_count = 0usize;
    let mut ready_build_count = 0usize;
    let mut stale_build_count = 0usize;

    for entry in fs::read_dir(store_dir).map_err(|error| {
        hyperindex_core::HyperindexError::Message(format!(
            "failed to read {}: {error}",
            store_dir.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            hyperindex_core::HyperindexError::Message(format!(
                "failed to read impact store entry: {error}"
            ))
        })?;
        let repo_path = entry.path();
        if !repo_path.is_dir() {
            continue;
        }
        let store_path = repo_path.join("impact.sqlite3");
        if !store_path.is_file() {
            continue;
        }

        let Some(repo_id) = repo_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let store = ImpactStore {
            store_path: store_path.clone(),
            schema_version: hyperindex_impact_store::IMPACT_STORE_SCHEMA_VERSION,
        };
        repo_count += 1;
        match store.list_builds() {
            Ok(builds) => {
                materialized_snapshot_count += builds.len();
                let symbol_store =
                    SymbolStore::open(&loaded.config.symbol_index.store_dir, repo_id).ok();
                for build in builds {
                    let ready = build.schema_version == store.schema_version
                        && build.impact_config_digest == builder.config_digest()
                        && symbol_store
                            .as_ref()
                            .and_then(|store| {
                                store.load_indexed_snapshot_state(&build.snapshot_id).ok()
                            })
                            .flatten()
                            .is_some();
                    if ready {
                        ready_build_count += 1;
                    } else {
                        stale_build_count += 1;
                    }
                }
            }
            Err(_) => {
                stale_build_count += 1;
            }
        }
    }

    Ok(ImpactRuntimeStatus {
        enabled: loaded.config.impact.enabled,
        store_dir: store_dir.display().to_string(),
        materialization_mode: loaded.config.impact.materialization_mode.clone(),
        repo_count,
        materialized_snapshot_count,
        ready_build_count,
        stale_build_count,
    })
}

fn impact_store(
    impact_store_root: &Path,
    repo_id: &str,
) -> hyperindex_impact_store::ImpactStoreResult<ImpactStore> {
    ImpactStore::open_in_store_dir(impact_store_root, repo_id)
}

fn impact_entity_key(entity: &hyperindex_protocol::impact::ImpactEntityRef) -> String {
    match entity {
        hyperindex_protocol::impact::ImpactEntityRef::Symbol {
            symbol_id,
            path,
            display_name,
        } => format!("symbol:{}:{}:{display_name}", symbol_id.0, path),
        hyperindex_protocol::impact::ImpactEntityRef::File { path } => {
            format!("file:{path}")
        }
        hyperindex_protocol::impact::ImpactEntityRef::Package {
            package_name,
            package_root,
        } => format!("package:{package_name}:{package_root}"),
        hyperindex_protocol::impact::ImpactEntityRef::Test {
            path,
            display_name,
            symbol_id,
        } => format!(
            "test:{}:{}:{}",
            path,
            display_name,
            symbol_id
                .as_ref()
                .map(|value| value.0.as_str())
                .unwrap_or("-")
        ),
    }
}

fn previous_build_candidate(
    repo_store: &RepoStore,
    impact_store: &ImpactStore,
    repo_id: &str,
    current_snapshot_id: &str,
) -> hyperindex_impact_store::ImpactStoreResult<Option<(ComposedSnapshot, StoredImpactBuild)>> {
    let manifests = repo_store.list_manifests(repo_id, 32).map_err(|error| {
        hyperindex_impact_store::ImpactStoreError::InvalidRoot(error.to_string())
    })?;
    for summary in manifests {
        if summary.snapshot_id == current_snapshot_id {
            continue;
        }
        let build = match impact_store.load_build(&summary.snapshot_id) {
            Ok(Some(build)) => build,
            Ok(None) => continue,
            Err(_) => continue,
        };
        let Some(snapshot) = repo_store
            .load_manifest(&summary.snapshot_id)
            .map_err(|error| {
                hyperindex_impact_store::ImpactStoreError::InvalidRoot(error.to_string())
            })?
        else {
            continue;
        };
        return Ok(Some((snapshot, build)));
    }
    Ok(None)
}

pub fn build_graph_from_store(
    symbol_store_root: &Path,
    repo_id: &str,
    snapshot: &ComposedSnapshot,
) -> Result<SymbolGraph, ProtocolError> {
    let store = SymbolStore::open(symbol_store_root, repo_id)
        .map_err(|error| ProtocolError::storage(error.to_string()))?;
    let indexed_state = store
        .load_indexed_snapshot_state(&snapshot.snapshot_id)
        .map_err(|error| ProtocolError::storage(error.to_string()))?;
    if indexed_state.is_none() {
        return Err(ProtocolError::impact_not_ready(
            repo_id,
            &snapshot.snapshot_id,
            "symbol index facts are not available for this snapshot",
        ));
    }
    let extracted = store
        .load_snapshot_facts(&snapshot.snapshot_id)
        .map_err(|error| ProtocolError::storage(error.to_string()))?;
    Ok(SymbolGraphBuilder.build_with_snapshot(
        &hyperindex_symbols::FactsBatch {
            files: extracted.files,
        },
        snapshot,
    ))
}

fn load_indexed_state(
    symbol_store_root: &Path,
    repo_id: &str,
    snapshot_id: &str,
) -> Option<IndexedSnapshotState> {
    let store = SymbolStore::open(symbol_store_root, repo_id).ok()?;
    store
        .load_indexed_snapshot_state(snapshot_id)
        .ok()
        .flatten()
}

fn impact_manifest(
    manifest: &hyperindex_impact_store::ImpactBuildManifest,
    build: Option<&StoredImpactBuild>,
    loaded_from_existing_build: bool,
    materialization_mode: hyperindex_protocol::impact::ImpactMaterializationMode,
) -> ImpactManifest {
    ImpactManifest {
        build_id: hyperindex_protocol::impact::ImpactBuildId(format!(
            "impact-build-{}",
            manifest.snapshot_id
        )),
        repo_id: manifest.repo_id.clone(),
        snapshot_id: manifest.snapshot_id.clone(),
        symbol_index_build_id: manifest
            .symbol_build_id
            .as_ref()
            .map(|value| hyperindex_protocol::symbols::SymbolIndexBuildId(value.clone())),
        created_at: build
            .map(|stored| stored.created_at.clone())
            .unwrap_or_else(epoch_ms_string),
        enrichments: build
            .map(|stored| stored.state.plan.metadata.clone())
            .unwrap_or_default(),
        storage: Some(hyperindex_protocol::impact::ImpactStorageMetadata {
            format: hyperindex_protocol::impact::ImpactStorageFormat::Sqlite,
            path: manifest.store_path.display().to_string(),
            schema_version: manifest.schema_version,
            materialization_mode,
        }),
        refresh_stats: build.map(|stored| stored.refresh_stats.clone()),
        refresh_mode: build.map(|stored| stored.refresh_mode.clone()),
        fallback_reason: build.and_then(|stored| stored.fallback_reason.clone()),
        loaded_from_existing_build,
    }
}

fn stored_build(
    snapshot: &ComposedSnapshot,
    store: &ImpactStore,
    refresh: &ImpactRefreshResult,
) -> StoredImpactBuild {
    StoredImpactBuild {
        repo_id: snapshot.repo_id.clone(),
        snapshot_id: snapshot.snapshot_id.clone(),
        impact_config_digest: refresh.config_digest.clone(),
        schema_version: store.schema_version,
        symbol_build_id: None,
        created_at: epoch_ms_string(),
        refresh_mode: match refresh.stats.mode {
            hyperindex_protocol::impact::ImpactRefreshMode::FullRebuild => {
                "full_rebuild".to_string()
            }
            hyperindex_protocol::impact::ImpactRefreshMode::Incremental => {
                "incremental".to_string()
            }
        },
        refresh_stats: refresh.stats.clone(),
        fallback_reason: refresh
            .fallback_reason
            .as_ref()
            .map(|reason| reason.as_str().to_string()),
        state: refresh.state.clone(),
    }
}

fn epoch_ms_string() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("epoch-ms:{now}")
}

fn map_impact_error(error: ImpactError) -> ProtocolError {
    match error {
        ImpactError::NotImplemented(method) => ProtocolError::not_implemented(method),
        ImpactError::TargetNotFound(target) => ProtocolError::impact_target_not_found(target),
    }
}

fn ensure_impact_enabled(
    impact_config: &ImpactConfig,
    method: &'static str,
) -> Result<(), ProtocolError> {
    if impact_config.enabled {
        Ok(())
    } else {
        Err(ProtocolError::config_invalid(format!(
            "{method} requires impact.enabled = true"
        )))
    }
}

fn apply_analyze_policy(
    mut response: ImpactAnalyzeResponse,
    impact_config: &ImpactConfig,
) -> ImpactAnalyzeResponse {
    let mut groups = Vec::new();
    for mut group in response.groups {
        if !impact_config.include_possible_results
            && group.certainty == ImpactCertaintyTier::Possible
        {
            continue;
        }
        for hit in &mut group.hits {
            hit.reason_paths
                .truncate(impact_config.max_reason_paths_per_hit);
        }
        group.hit_count = group.hits.len() as u32;
        if !group.hits.is_empty() {
            groups.push(group);
        }
    }
    response.summary = summarize_groups(&groups);
    response.groups = groups;
    response
}

fn summarize_groups(groups: &[ImpactResultGroup]) -> ImpactSummary {
    let mut direct_count = 0u32;
    let mut transitive_count = 0u32;
    let mut certainty_counts = ImpactCertaintyCounts {
        certain: 0,
        likely: 0,
        possible: 0,
    };
    for group in groups {
        if group.direct {
            direct_count += group.hit_count;
        } else {
            transitive_count += group.hit_count;
        }
        match group.certainty {
            ImpactCertaintyTier::Certain => certainty_counts.certain += group.hit_count,
            ImpactCertaintyTier::Likely => certainty_counts.likely += group.hit_count,
            ImpactCertaintyTier::Possible => certainty_counts.possible += group.hit_count,
        }
    }
    ImpactSummary {
        direct_count,
        transitive_count,
        certainty_counts,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use hyperindex_protocol::config::ImpactConfig;
    use hyperindex_protocol::impact::{
        ImpactAnalysisState, ImpactAnalyzeParams, ImpactCertaintyTier, ImpactChangeScenario,
        ImpactExplainParams, ImpactRefreshMode, ImpactStatusParams, ImpactTargetRef,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, BufferOverlay, ComposedSnapshot, SnapshotFile,
        WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use hyperindex_repo_store::RepoStore;
    use hyperindex_symbol_store::{IndexedSnapshotState, SymbolStore};
    use hyperindex_symbols::SymbolWorkspace;
    use tempfile::TempDir;

    use super::{ImpactService, build_graph_from_store};

    fn snapshot(
        snapshot_id: &str,
        files: Vec<(&str, &str)>,
        buffers: Vec<BufferOverlay>,
    ) -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: snapshot_id.to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "abc123".to_string(),
                digest: format!("base-{snapshot_id}"),
                file_count: files.len(),
                files: files
                    .into_iter()
                    .map(|(path, contents)| SnapshotFile {
                        path: path.to_string(),
                        content_sha256: format!("sha-{snapshot_id}-{path}"),
                        content_bytes: contents.len(),
                        contents: contents.to_string(),
                    })
                    .collect(),
            },
            working_tree: WorkingTreeOverlay {
                digest: format!("work-{snapshot_id}"),
                entries: Vec::new(),
            },
            buffers,
        }
    }

    fn base_snapshot() -> ComposedSnapshot {
        snapshot(
            "snap-1",
            vec![
                ("package.json", r#"{ "name": "repo-root" }"#),
                (
                    "packages/auth/src/session/service.ts",
                    r#"
                    export function invalidateSession() {
                      return 1;
                    }
                    "#,
                ),
                (
                    "packages/api/src/routes/logout.ts",
                    r#"
                    import { invalidateSession } from "../../auth/src/session/service";

                    export function logout() {
                      return invalidateSession();
                    }
                    "#,
                ),
            ],
            Vec::new(),
        )
    }

    fn heuristic_snapshot() -> ComposedSnapshot {
        snapshot(
            "snap-heuristic",
            vec![
                ("package.json", r#"{ "name": "repo-root" }"#),
                (
                    "packages/auth/package.json",
                    r#"{ "name": "@hyperindex/auth" }"#,
                ),
                (
                    "packages/auth/src/session/service.ts",
                    r#"
                    export function invalidateSession() {
                      return 1;
                    }
                    "#,
                ),
                (
                    "packages/auth/tests/session/service.test.ts",
                    r#"
                    test("service", () => {
                      expect(true).toBe(true);
                    });
                    "#,
                ),
            ],
            Vec::new(),
        )
    }

    fn persist_snapshot_and_index(
        repo_store: &RepoStore,
        symbol_store_root: &Path,
        snapshot: &ComposedSnapshot,
    ) {
        repo_store.persist_manifest(snapshot).unwrap();
        let mut workspace = SymbolWorkspace::default();
        let index = workspace.prepare_snapshot(snapshot).unwrap();
        let store = SymbolStore::open(symbol_store_root, "repo-1").unwrap();
        store
            .persist_facts("repo-1", &snapshot.snapshot_id, &index.facts)
            .unwrap();
        store
            .record_indexed_snapshot_state(&IndexedSnapshotState {
                repo_id: "repo-1".to_string(),
                snapshot_id: snapshot.snapshot_id.clone(),
                parser_config_digest: "parser-config".to_string(),
                schema_version: 1,
                indexed_file_count: index.graph.indexed_files,
                refresh_mode: "incremental".to_string(),
            })
            .unwrap();
    }

    #[test]
    fn impact_service_returns_direct_analyze_results_and_persists_build_metadata() {
        let impact_store_root = TempDir::new().unwrap();
        let symbol_store_root = TempDir::new().unwrap();
        let repo_store = RepoStore::open_in_memory().unwrap();
        let snapshot = base_snapshot();
        persist_snapshot_and_index(&repo_store, symbol_store_root.path(), &snapshot);
        let graph = build_graph_from_store(symbol_store_root.path(), "repo-1", &snapshot).unwrap();

        let response = ImpactService
            .analyze(
                impact_store_root.path(),
                &repo_store,
                &ImpactConfig::default(),
                &graph,
                &snapshot,
                &ImpactAnalyzeParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    target: ImpactTargetRef::Symbol {
                        value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                        symbol_id: None,
                        path: Some("packages/auth/src/session/service.ts".to_string()),
                    },
                    change_hint: ImpactChangeScenario::ModifyBehavior,
                    limit: 20,
                    include_transitive: false,
                    include_reason_paths: true,
                    max_transitive_depth: None,
                    max_nodes_visited: None,
                    max_edges_traversed: None,
                    max_candidates_considered: None,
                },
            )
            .unwrap();

        assert!(response.summary.direct_count > 0);
        let manifest = response.manifest.unwrap();
        assert_eq!(manifest.refresh_mode.as_deref(), Some("full_rebuild"));
        assert_eq!(
            manifest.refresh_stats.unwrap().mode,
            ImpactRefreshMode::FullRebuild
        );
    }

    #[test]
    fn impact_service_uses_incremental_refresh_for_buffer_overlay_snapshot() {
        let impact_store_root = TempDir::new().unwrap();
        let symbol_store_root = TempDir::new().unwrap();
        let repo_store = RepoStore::open_in_memory().unwrap();
        let base = base_snapshot();
        let buffered = snapshot(
            "snap-2",
            vec![
                ("package.json", r#"{ "name": "repo-root" }"#),
                (
                    "packages/auth/src/session/service.ts",
                    r#"
                    export function invalidateSession() {
                      return 1;
                    }
                    "#,
                ),
                (
                    "packages/api/src/routes/logout.ts",
                    r#"
                    import { invalidateSessionBuffered } from "../../auth/src/session/service";

                    export function logout() {
                      return invalidateSessionBuffered();
                    }
                    "#,
                ),
            ],
            vec![BufferOverlay {
                buffer_id: "buffer-1".to_string(),
                path: "packages/auth/src/session/service.ts".to_string(),
                version: 1,
                content_sha256: "sha-buffer".to_string(),
                content_bytes: 99,
                contents: r#"
                export function invalidateSessionBuffered() {
                  return 2;
                }
                "#
                .to_string(),
            }],
        );
        persist_snapshot_and_index(&repo_store, symbol_store_root.path(), &base);
        persist_snapshot_and_index(&repo_store, symbol_store_root.path(), &buffered);

        let base_graph = build_graph_from_store(symbol_store_root.path(), "repo-1", &base).unwrap();
        ImpactService
            .analyze(
                impact_store_root.path(),
                &repo_store,
                &ImpactConfig::default(),
                &base_graph,
                &base,
                &ImpactAnalyzeParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: base.snapshot_id.clone(),
                    target: ImpactTargetRef::Symbol {
                        value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                        symbol_id: None,
                        path: Some("packages/auth/src/session/service.ts".to_string()),
                    },
                    change_hint: ImpactChangeScenario::ModifyBehavior,
                    limit: 20,
                    include_transitive: true,
                    include_reason_paths: true,
                    max_transitive_depth: None,
                    max_nodes_visited: None,
                    max_edges_traversed: None,
                    max_candidates_considered: None,
                },
            )
            .unwrap();

        let buffered_graph =
            build_graph_from_store(symbol_store_root.path(), "repo-1", &buffered).unwrap();
        let refreshed = ImpactService
            .analyze(
                impact_store_root.path(),
                &repo_store,
                &ImpactConfig::default(),
                &buffered_graph,
                &buffered,
                &ImpactAnalyzeParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: buffered.snapshot_id.clone(),
                    target: ImpactTargetRef::File {
                        path: "packages/auth/src/session/service.ts".to_string(),
                    },
                    change_hint: ImpactChangeScenario::ModifyBehavior,
                    limit: 20,
                    include_transitive: true,
                    include_reason_paths: true,
                    max_transitive_depth: None,
                    max_nodes_visited: None,
                    max_edges_traversed: None,
                    max_candidates_considered: None,
                },
            )
            .unwrap();

        let manifest = refreshed.manifest.unwrap();
        assert_eq!(manifest.refresh_mode.as_deref(), Some("incremental"));
        assert_eq!(
            manifest.refresh_stats.unwrap().mode,
            ImpactRefreshMode::Incremental
        );
    }

    #[test]
    fn impact_service_status_reports_ready_without_manifest_before_first_build() {
        let impact_store_root = TempDir::new().unwrap();
        let symbol_store_root = TempDir::new().unwrap();
        let repo_store = RepoStore::open_in_memory().unwrap();
        let snapshot = base_snapshot();
        persist_snapshot_and_index(&repo_store, symbol_store_root.path(), &snapshot);
        let status = ImpactService
            .status(
                impact_store_root.path(),
                symbol_store_root.path(),
                &ImpactConfig::default(),
                &ImpactStatusParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                },
            )
            .unwrap();

        assert_eq!(status.state, ImpactAnalysisState::Ready);
        assert!(status.capabilities.analyze);
        assert!(status.manifest.is_none());
        assert!(
            status
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "impact_build_missing")
        );
    }

    #[test]
    fn impact_service_explain_returns_reason_paths() {
        let impact_store_root = TempDir::new().unwrap();
        let symbol_store_root = TempDir::new().unwrap();
        let repo_store = RepoStore::open_in_memory().unwrap();
        let snapshot = base_snapshot();
        persist_snapshot_and_index(&repo_store, symbol_store_root.path(), &snapshot);
        let graph = build_graph_from_store(symbol_store_root.path(), "repo-1", &snapshot).unwrap();
        let analyze = ImpactService
            .analyze(
                impact_store_root.path(),
                &repo_store,
                &ImpactConfig::default(),
                &graph,
                &snapshot,
                &ImpactAnalyzeParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    target: ImpactTargetRef::Symbol {
                        value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                        symbol_id: None,
                        path: Some("packages/auth/src/session/service.ts".to_string()),
                    },
                    change_hint: ImpactChangeScenario::ModifyBehavior,
                    limit: 20,
                    include_transitive: true,
                    include_reason_paths: true,
                    max_transitive_depth: None,
                    max_nodes_visited: None,
                    max_edges_traversed: None,
                    max_candidates_considered: None,
                },
            )
            .unwrap();
        let impacted = analyze
            .groups
            .iter()
            .flat_map(|group| group.hits.iter())
            .next()
            .unwrap()
            .entity
            .clone();

        let response = ImpactService
            .explain(
                impact_store_root.path(),
                &repo_store,
                &ImpactConfig::default(),
                &graph,
                &snapshot,
                &ImpactExplainParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    target: ImpactTargetRef::Symbol {
                        value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                        symbol_id: None,
                        path: Some("packages/auth/src/session/service.ts".to_string()),
                    },
                    change_hint: ImpactChangeScenario::ModifyBehavior,
                    impacted: impacted.clone(),
                    max_reason_paths: 4,
                },
            )
            .unwrap();

        assert_eq!(response.impacted, impacted);
        assert!(!response.reason_paths.is_empty());
    }

    #[test]
    fn impact_service_applies_runtime_result_policies() {
        let impact_store_root = TempDir::new().unwrap();
        let symbol_store_root = TempDir::new().unwrap();
        let repo_store = RepoStore::open_in_memory().unwrap();
        let snapshot = heuristic_snapshot();
        persist_snapshot_and_index(&repo_store, symbol_store_root.path(), &snapshot);
        let graph = build_graph_from_store(symbol_store_root.path(), "repo-1", &snapshot).unwrap();
        let params = ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: snapshot.snapshot_id.clone(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            limit: 20,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        };

        let default_response = ImpactService
            .analyze(
                impact_store_root.path(),
                &repo_store,
                &ImpactConfig::default(),
                &graph,
                &snapshot,
                &params,
            )
            .unwrap();
        assert!(
            default_response
                .groups
                .iter()
                .any(|group| group.certainty == ImpactCertaintyTier::Possible)
        );

        let mut config = ImpactConfig::default();
        config.include_possible_results = false;
        config.max_reason_paths_per_hit = 0;
        let filtered_response = ImpactService
            .analyze(
                impact_store_root.path(),
                &repo_store,
                &config,
                &graph,
                &snapshot,
                &params,
            )
            .unwrap();

        assert!(
            !filtered_response
                .groups
                .iter()
                .any(|group| group.certainty == ImpactCertaintyTier::Possible)
        );
        assert_eq!(filtered_response.summary.certainty_counts.possible, 0);
        assert!(
            filtered_response
                .groups
                .iter()
                .flat_map(|group| group.hits.iter())
                .all(|hit| hit.reason_paths.is_empty())
        );
    }

    #[test]
    fn impact_service_status_reports_disabled_runtime_config() {
        let impact_store_root = TempDir::new().unwrap();
        let symbol_store_root = TempDir::new().unwrap();
        let mut config = ImpactConfig::default();
        config.enabled = false;

        let status = ImpactService
            .status(
                impact_store_root.path(),
                symbol_store_root.path(),
                &config,
                &ImpactStatusParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: "snap-1".to_string(),
                },
            )
            .unwrap();

        assert_eq!(status.state, ImpactAnalysisState::NotReady);
        assert!(!status.capabilities.analyze);
        assert!(!status.capabilities.explain);
        assert!(!status.capabilities.materialized_store);
        assert!(status.manifest.is_none());
        assert!(
            status
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "impact_disabled")
        );
    }

    #[test]
    fn impact_service_rejects_analyze_when_impact_is_disabled() {
        let impact_store_root = TempDir::new().unwrap();
        let symbol_store_root = TempDir::new().unwrap();
        let repo_store = RepoStore::open_in_memory().unwrap();
        let snapshot = base_snapshot();
        persist_snapshot_and_index(&repo_store, symbol_store_root.path(), &snapshot);
        let graph = build_graph_from_store(symbol_store_root.path(), "repo-1", &snapshot).unwrap();
        let mut config = ImpactConfig::default();
        config.enabled = false;

        let error = ImpactService
            .analyze(
                impact_store_root.path(),
                &repo_store,
                &config,
                &graph,
                &snapshot,
                &ImpactAnalyzeParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    target: ImpactTargetRef::Symbol {
                        value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                        symbol_id: None,
                        path: Some("packages/auth/src/session/service.ts".to_string()),
                    },
                    change_hint: ImpactChangeScenario::ModifyBehavior,
                    limit: 20,
                    include_transitive: true,
                    include_reason_paths: true,
                    max_transitive_depth: None,
                    max_nodes_visited: None,
                    max_edges_traversed: None,
                    max_candidates_considered: None,
                },
            )
            .unwrap_err();

        assert_eq!(
            error.code,
            hyperindex_protocol::errors::ErrorCode::ConfigInvalid
        );
        assert!(error.message.contains("impact.enabled = true"));
    }

    #[test]
    fn impact_service_rejects_explain_when_impact_is_disabled() {
        let impact_store_root = TempDir::new().unwrap();
        let symbol_store_root = TempDir::new().unwrap();
        let repo_store = RepoStore::open_in_memory().unwrap();
        let snapshot = base_snapshot();
        persist_snapshot_and_index(&repo_store, symbol_store_root.path(), &snapshot);
        let graph = build_graph_from_store(symbol_store_root.path(), "repo-1", &snapshot).unwrap();
        let mut config = ImpactConfig::default();
        config.enabled = false;

        let error = ImpactService
            .explain(
                impact_store_root.path(),
                &repo_store,
                &config,
                &graph,
                &snapshot,
                &ImpactExplainParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    target: ImpactTargetRef::Symbol {
                        value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                        symbol_id: None,
                        path: Some("packages/auth/src/session/service.ts".to_string()),
                    },
                    change_hint: ImpactChangeScenario::ModifyBehavior,
                    impacted: hyperindex_protocol::impact::ImpactEntityRef::File {
                        path: "packages/api/src/routes/logout.ts".to_string(),
                    },
                    max_reason_paths: 4,
                },
            )
            .unwrap_err();

        assert_eq!(
            error.code,
            hyperindex_protocol::errors::ErrorCode::ConfigInvalid
        );
        assert!(error.message.contains("impact.enabled = true"));
    }
}
