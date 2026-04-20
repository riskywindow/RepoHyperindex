use std::fs;
use std::path::Path;

use hyperindex_config::LoadedConfig;
use hyperindex_core::HyperindexResult;
use hyperindex_protocol::config::SemanticConfig;
use hyperindex_protocol::errors::ProtocolError;
use hyperindex_protocol::semantic::{
    SemanticAnalysisState, SemanticBuildParams, SemanticBuildRecord, SemanticBuildResponse,
    SemanticBuildState, SemanticInspectChunkParams, SemanticInspectChunkResponse,
    SemanticQueryParams, SemanticQueryResponse, SemanticStatusParams, SemanticStatusResponse,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::status::SemanticRuntimeStatus;
use hyperindex_repo_store::RepoStore;
use hyperindex_semantic::daemon_integration::{
    info_diagnostic, scaffold_status_response, warning_diagnostic,
};
use hyperindex_semantic::{
    IncrementalSemanticIndexer, SemanticScaffoldBuilder, SemanticSearchEngine,
    SemanticSearchScaffold, provider_from_config,
};
use hyperindex_semantic_store::{SemanticStore, StoredSemanticBuild};
use hyperindex_snapshot::SnapshotAssembler;
use hyperindex_symbol_store::SymbolStore;
use hyperindex_symbols::{FactsBatch, SymbolGraph, SymbolGraphBuilder};

#[derive(Debug, Default, Clone)]
pub struct SemanticService;

impl SemanticService {
    pub fn status(
        &self,
        semantic_store_root: &Path,
        symbol_store_root: &Path,
        semantic_config: &SemanticConfig,
        params: &SemanticStatusParams,
    ) -> Result<SemanticStatusResponse, ProtocolError> {
        if !semantic_config.enabled {
            return Ok(scaffold_status_response(
                &params.repo_id,
                &params.snapshot_id,
                SemanticAnalysisState::Disabled,
                Vec::new(),
                vec![info_diagnostic(
                    "semantic_disabled",
                    "semantic retrieval is disabled in runtime config",
                )],
            ));
        }

        let symbol_ready =
            load_symbol_state(symbol_store_root, &params.repo_id, &params.snapshot_id)
                .map_err(|error| ProtocolError::storage(error.to_string()))?
                .is_some();
        let store = SemanticStore::open_in_store_dir(semantic_store_root, &params.repo_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let build = store
            .load_build(&params.snapshot_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let index_metadata = if build.is_some() {
            store
                .load_vector_index_metadata(&params.snapshot_id)
                .map_err(|error| ProtocolError::storage(error.to_string()))?
        } else {
            None
        };
        let builder = SemanticScaffoldBuilder::from_config(semantic_config);
        let build = build.filter(|build| {
            params
                .build_id
                .as_ref()
                .map(|build_id| build.semantic_build_id == *build_id)
                .unwrap_or(true)
        });
        let stale = build
            .as_ref()
            .map(|build| {
                build.schema_version != store.schema_version
                    || build.semantic_config_digest != builder.config_digest()
            })
            .unwrap_or(false);
        let state = if build.is_none() || index_metadata.is_none() {
            SemanticAnalysisState::NotReady
        } else if stale {
            SemanticAnalysisState::Stale
        } else {
            SemanticAnalysisState::Ready
        };
        let diagnostics = status_diagnostics(
            symbol_ready,
            build.as_ref(),
            index_metadata.is_some(),
            stale,
        );
        Ok(scaffold_status_response(
            &params.repo_id,
            &params.snapshot_id,
            state,
            build
                .as_ref()
                .map(|build| build_record(&store, build))
                .into_iter()
                .collect(),
            diagnostics,
        ))
    }

    pub fn build(
        &self,
        repo_store: &RepoStore,
        semantic_store_root: &Path,
        symbol_store_root: &Path,
        semantic_config: &SemanticConfig,
        snapshot: &ComposedSnapshot,
        params: &SemanticBuildParams,
    ) -> Result<SemanticBuildResponse, ProtocolError> {
        ensure_semantic_enabled(semantic_config, &params.repo_id, &params.snapshot_id)?;
        let store = SemanticStore::open_in_store_dir(semantic_store_root, &params.repo_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let maybe_symbol_index =
            load_symbol_index_materialization(symbol_store_root, &params.repo_id, snapshot)
                .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let draft = if let Some((facts, graph, symbol_index_build_id)) = maybe_symbol_index {
            let indexer = IncrementalSemanticIndexer::new(semantic_config);
            if params.force {
                indexer
                    .full_rebuild(
                        &store,
                        snapshot,
                        &facts,
                        &graph,
                        Some(symbol_index_build_id),
                    )
                    .map(|result| result.build)
            } else {
                let previous_snapshot = load_previous_semantic_snapshot(
                    repo_store,
                    &store,
                    &params.repo_id,
                    &params.snapshot_id,
                )
                .map_err(|error| ProtocolError::storage(error.to_string()))?;
                let diff = previous_snapshot
                    .as_ref()
                    .map(|previous| SnapshotAssembler.diff(previous, snapshot));
                indexer
                    .refresh(
                        &store,
                        previous_snapshot.as_ref(),
                        snapshot,
                        diff.as_ref(),
                        &facts,
                        &graph,
                        Some(symbol_index_build_id),
                    )
                    .map(|result| result.build)
            }
        } else {
            SemanticScaffoldBuilder::from_config(semantic_config).build(snapshot, None, &store)
        }
        .map_err(|error| ProtocolError::internal(error.to_string()))?;
        let build = StoredSemanticBuild {
            repo_id: draft.repo_id.clone(),
            snapshot_id: draft.snapshot_id.clone(),
            semantic_build_id: draft.semantic_build_id.clone(),
            semantic_config_digest: draft.semantic_config_digest.clone(),
            schema_version: store.schema_version,
            chunk_schema_version: draft.chunk_schema_version,
            embedding_provider: draft.embedding_provider.clone(),
            chunk_text: draft.chunk_text.clone(),
            symbol_index_build_id: draft.symbol_index_build_id.clone(),
            created_at: draft.created_at.clone(),
            refresh_mode: draft.refresh_mode.clone(),
            chunk_count: draft.chunk_count,
            indexed_file_count: draft.indexed_file_count,
            embedding_count: draft.embedding_count,
            embedding_cache_hits: draft.embedding_stats.cache_hits,
            embedding_cache_misses: draft.embedding_stats.cache_misses,
            embedding_cache_writes: draft.embedding_stats.cache_writes,
            embedding_provider_batches: draft.embedding_stats.provider_batches,
            refresh_stats: draft.refresh_stats.clone(),
            fallback_reason: draft.fallback_reason.clone(),
            diagnostics: draft.diagnostics.clone(),
        };
        store
            .persist_build_with_chunks_and_vectors(&build, &draft.chunks, &draft.chunk_vectors)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let mut record = build_record(&store, &build);
        record.loaded_from_existing_build = false;
        Ok(SemanticBuildResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            build: record,
        })
    }

    pub fn query(
        &self,
        semantic_store_root: &Path,
        _symbol_store_root: &Path,
        semantic_config: &SemanticConfig,
        params: &SemanticQueryParams,
    ) -> Result<SemanticQueryResponse, ProtocolError> {
        if params.query.text.trim().is_empty() {
            return Err(ProtocolError::invalid_field(
                "query.text",
                "semantic query text must not be empty",
                Some("non-empty natural-language query".to_string()),
            ));
        }

        let store = SemanticStore::open_in_store_dir(semantic_store_root, &params.repo_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let build = store
            .load_build(&params.snapshot_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let build = build.ok_or_else(|| {
            ProtocolError::semantic_not_ready(&params.repo_id, &params.snapshot_id)
        })?;
        let builder = SemanticScaffoldBuilder::from_config(semantic_config);
        if build.schema_version != store.schema_version
            || build.semantic_config_digest != builder.config_digest()
        {
            return Err(ProtocolError::storage(
                "semantic build is stale for the current config; rebuild required",
            ));
        }
        let provider = provider_from_config(semantic_config)
            .map_err(|error| ProtocolError::internal(error.to_string()))?;
        let index = store
            .load_vector_index(&params.snapshot_id, &build)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let chunks = store
            .list_chunks(&params.snapshot_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let engine = SemanticSearchEngine::default();

        engine
            .search(
                &SemanticSearchScaffold {
                    manifest: store.manifest_for(&build),
                    chunks,
                    index,
                    diagnostics: build.diagnostics.clone(),
                },
                params,
                &store,
                provider.as_ref(),
            )
            .map_err(|error| ProtocolError::internal(error.to_string()))
    }

    pub fn inspect_chunk(
        &self,
        semantic_store_root: &Path,
        _semantic_config: &SemanticConfig,
        params: &SemanticInspectChunkParams,
    ) -> Result<SemanticInspectChunkResponse, ProtocolError> {
        let store = SemanticStore::open_in_store_dir(semantic_store_root, &params.repo_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?;
        let build = store
            .load_build(&params.snapshot_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?
            .ok_or_else(|| {
                ProtocolError::semantic_not_ready(&params.repo_id, &params.snapshot_id)
            })?;
        if let Some(build_id) = &params.build_id {
            if build.semantic_build_id != *build_id {
                return Err(ProtocolError::semantic_build_not_found(build_id.0.clone()));
            }
        }
        let chunk = store
            .load_chunk(&params.snapshot_id, &params.chunk_id)
            .map_err(|error| ProtocolError::storage(error.to_string()))?
            .ok_or_else(|| ProtocolError::semantic_chunk_not_found(params.chunk_id.0.clone()))?;
        Ok(SemanticInspectChunkResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            manifest: Some(store.manifest_for(&build)),
            chunk,
            diagnostics: vec![info_diagnostic(
                "semantic_chunk_loaded",
                format!("loaded semantic chunk {}", params.chunk_id.0),
            )],
        })
    }
}

pub fn scan_semantic_runtime_status(
    loaded: &LoadedConfig,
) -> HyperindexResult<SemanticRuntimeStatus> {
    let config = &loaded.config.semantic;
    if !config.enabled {
        return Ok(SemanticRuntimeStatus {
            enabled: false,
            store_dir: config.store_dir.display().to_string(),
            embedding_model_id: config.embedding_provider.model_id.clone(),
            chunk_schema_version: config.chunk_schema_version,
            repo_count: 0,
            materialized_snapshot_count: 0,
            ready_build_count: 0,
            stale_build_count: 0,
        });
    }
    if !config.store_dir.exists() {
        return Ok(SemanticRuntimeStatus {
            enabled: true,
            store_dir: config.store_dir.display().to_string(),
            embedding_model_id: config.embedding_provider.model_id.clone(),
            chunk_schema_version: config.chunk_schema_version,
            repo_count: 0,
            materialized_snapshot_count: 0,
            ready_build_count: 0,
            stale_build_count: 0,
        });
    }

    let mut repo_count = 0usize;
    let mut materialized_snapshot_count = 0usize;
    let entries = fs::read_dir(&config.store_dir)
        .map_err(|error| hyperindex_core::HyperindexError::Message(error.to_string()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| hyperindex_core::HyperindexError::Message(error.to_string()))?;
        if !entry
            .file_type()
            .map_err(|error| hyperindex_core::HyperindexError::Message(error.to_string()))?
            .is_dir()
        {
            continue;
        }
        let store_path = entry.path().join("semantic.sqlite3");
        if !store_path.exists() {
            continue;
        }
        let store = SemanticStore::open_at_path(store_path)
            .map_err(|error| hyperindex_core::HyperindexError::Message(error.to_string()))?;
        let status = store
            .status()
            .map_err(|error| hyperindex_core::HyperindexError::Message(error.to_string()))?;
        repo_count += 1;
        materialized_snapshot_count += status.build_count;
    }

    Ok(SemanticRuntimeStatus {
        enabled: true,
        store_dir: config.store_dir.display().to_string(),
        embedding_model_id: config.embedding_provider.model_id.clone(),
        chunk_schema_version: config.chunk_schema_version,
        repo_count,
        materialized_snapshot_count,
        ready_build_count: materialized_snapshot_count,
        stale_build_count: 0,
    })
}

fn build_record(store: &SemanticStore, build: &StoredSemanticBuild) -> SemanticBuildRecord {
    SemanticBuildRecord {
        build_id: build.semantic_build_id.clone(),
        state: SemanticBuildState::Succeeded,
        requested_at: build.created_at.clone(),
        started_at: Some(build.created_at.clone()),
        finished_at: Some(build.created_at.clone()),
        manifest: Some(store.manifest_for(build)),
        refresh_stats: build.refresh_stats.clone(),
        refresh_mode: Some(build.refresh_mode.clone()),
        fallback_reason: build.fallback_reason.clone(),
        diagnostics: build.diagnostics.clone(),
        loaded_from_existing_build: true,
    }
}

fn ensure_semantic_enabled(
    semantic_config: &SemanticConfig,
    repo_id: &str,
    snapshot_id: &str,
) -> Result<(), ProtocolError> {
    if semantic_config.enabled {
        return Ok(());
    }
    Err(ProtocolError::semantic_not_ready(repo_id, snapshot_id))
}

fn load_symbol_state(
    symbol_store_root: &Path,
    repo_id: &str,
    snapshot_id: &str,
) -> hyperindex_symbol_store::SymbolStoreResult<Option<hyperindex_symbol_store::IndexedSnapshotState>>
{
    let store = SymbolStore::open(symbol_store_root, repo_id)?;
    store.load_indexed_snapshot_state(snapshot_id)
}

fn load_symbol_index_materialization(
    symbol_store_root: &Path,
    repo_id: &str,
    snapshot: &ComposedSnapshot,
) -> hyperindex_symbol_store::SymbolStoreResult<
    Option<(
        FactsBatch,
        SymbolGraph,
        hyperindex_protocol::symbols::SymbolIndexBuildId,
    )>,
> {
    let Some(state) = load_symbol_state(symbol_store_root, repo_id, &snapshot.snapshot_id)? else {
        return Ok(None);
    };
    let store = SymbolStore::open(symbol_store_root, repo_id)?;
    let extracted = store.load_snapshot_facts(&snapshot.snapshot_id)?;
    let facts = FactsBatch {
        files: extracted.files,
    };
    let graph = SymbolGraphBuilder.build_with_snapshot(&facts, snapshot);
    Ok(Some((
        facts,
        graph,
        hyperindex_protocol::symbols::SymbolIndexBuildId(format!(
            "symbol-index-scaffold-{}",
            state.snapshot_id
        )),
    )))
}

fn load_previous_semantic_snapshot(
    repo_store: &RepoStore,
    semantic_store: &SemanticStore,
    repo_id: &str,
    current_snapshot_id: &str,
) -> hyperindex_semantic_store::SemanticStoreResult<Option<ComposedSnapshot>> {
    let summaries = repo_store.list_manifests(repo_id, 32).map_err(|error| {
        hyperindex_semantic_store::SemanticStoreError::Compatibility(error.to_string())
    })?;
    for summary in summaries {
        if summary.snapshot_id == current_snapshot_id {
            continue;
        }
        if semantic_store.load_build(&summary.snapshot_id)?.is_none() {
            continue;
        }
        let Some(snapshot) = repo_store
            .load_manifest(&summary.snapshot_id)
            .map_err(|error| {
                hyperindex_semantic_store::SemanticStoreError::Compatibility(error.to_string())
            })?
        else {
            continue;
        };
        return Ok(Some(snapshot));
    }
    Ok(None)
}

fn status_diagnostics(
    symbol_ready: bool,
    build: Option<&StoredSemanticBuild>,
    index_present: bool,
    stale: bool,
) -> Vec<hyperindex_protocol::semantic::SemanticDiagnostic> {
    if build.is_none() {
        return vec![if !symbol_ready {
            warning_diagnostic(
                "semantic_build_missing",
                "no semantic build is present yet; build semantic state for this snapshot before querying",
            )
        } else {
            info_diagnostic(
                "semantic_build_missing",
                "no semantic build is present yet for this snapshot",
            )
        }];
    }
    if !index_present {
        return vec![warning_diagnostic(
            "semantic_vector_index_missing",
            "semantic build metadata exists but the persisted vector index is missing; rebuild is required",
        )];
    }
    if stale {
        return vec![warning_diagnostic(
            "semantic_build_stale",
            "stored semantic build is stale for the current config; rebuild is required",
        )];
    }
    let mut diagnostics = vec![info_diagnostic(
        "semantic_query_ready",
        "semantic chunks and vectors are materialized and query-ready",
    )];
    if !symbol_ready {
        diagnostics.push(info_diagnostic(
            "semantic_symbol_index_optional",
            "semantic queries can warm-load the persisted vector index even when the symbol store is absent",
        ));
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use hyperindex_config::LoadedConfig;
    use hyperindex_protocol::semantic::{
        SemanticBuildParams, SemanticInspectChunkParams, SemanticQueryFilters, SemanticQueryParams,
        SemanticQueryText, SemanticRerankMode, SemanticStatusParams,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, SnapshotFile, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use hyperindex_repo_store::RepoStore;
    use hyperindex_semantic_store::SemanticStore;

    use super::{SemanticService, scan_semantic_runtime_status};

    #[test]
    fn runtime_status_reports_empty_store_cleanly() {
        let tempdir = tempdir().unwrap();
        let mut config = hyperindex_protocol::config::RuntimeConfig::default();
        config.directories.runtime_root = tempdir.path().join(".hyperindex");
        config.semantic.store_dir = config.directories.runtime_root.join("data/semantic");
        let loaded = LoadedConfig {
            config_path: tempdir.path().join("config.toml"),
            config,
        };
        let status = scan_semantic_runtime_status(&loaded).unwrap();
        assert!(status.enabled);
        assert_eq!(status.materialized_snapshot_count, 0);
    }

    #[test]
    fn query_rejects_empty_query() {
        let service = SemanticService;
        let tempdir = tempdir().unwrap();
        let config = hyperindex_protocol::config::SemanticConfig::default();
        let error = service
            .query(
                tempdir.path(),
                tempdir.path(),
                &config,
                &SemanticQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: SemanticQueryText {
                        text: "   ".to_string(),
                    },
                    filters: SemanticQueryFilters::default(),
                    limit: 10,
                    rerank_mode: SemanticRerankMode::Hybrid,
                },
            )
            .unwrap_err();
        assert_eq!(error.message, "request validation failed");
    }

    #[test]
    fn disabled_status_reports_disabled_state() {
        let service = SemanticService;
        let tempdir = tempdir().unwrap();
        let mut config = hyperindex_protocol::config::SemanticConfig::default();
        config.enabled = false;
        let response = service
            .status(
                tempdir.path(),
                tempdir.path(),
                &config,
                &SemanticStatusParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    build_id: None,
                },
            )
            .unwrap();
        assert!(matches!(
            response.state,
            hyperindex_protocol::semantic::SemanticAnalysisState::Disabled
        ));
    }

    #[test]
    fn inspect_chunk_returns_materialized_chunk_record() {
        let service = SemanticService;
        let tempdir = tempdir().unwrap();
        let config = hyperindex_protocol::config::SemanticConfig::default();
        let snapshot = semantic_fixture_snapshot();
        let repo_store = RepoStore::open_in_memory().unwrap();
        repo_store.persist_manifest(&snapshot).unwrap();

        service
            .build(
                &repo_store,
                tempdir.path(),
                tempdir.path(),
                &config,
                &snapshot,
                &SemanticBuildParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    force: true,
                },
            )
            .unwrap();

        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let chunk = store.list_chunks("snap-123").unwrap().pop().unwrap();
        let response = service
            .inspect_chunk(
                tempdir.path(),
                &config,
                &SemanticInspectChunkParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    chunk_id: chunk.metadata.chunk_id.clone(),
                    build_id: None,
                },
            )
            .unwrap();

        assert_eq!(response.chunk, chunk);
        assert!(response.chunk.serialized_text.contains("invalidateSession"));
        assert_eq!(response.diagnostics[0].code, "semantic_chunk_loaded");
    }

    #[test]
    fn query_returns_filtered_hits_with_stable_ordering() {
        let service = SemanticService;
        let tempdir = tempdir().unwrap();
        let config = hyperindex_protocol::config::SemanticConfig::default();
        let snapshot = semantic_fixture_snapshot();
        let repo_store = RepoStore::open_in_memory().unwrap();
        repo_store.persist_manifest(&snapshot).unwrap();

        service
            .build(
                &repo_store,
                tempdir.path(),
                tempdir.path(),
                &config,
                &snapshot,
                &SemanticBuildParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    force: true,
                },
            )
            .unwrap();

        let first = service
            .query(
                tempdir.path(),
                tempdir.path(),
                &config,
                &SemanticQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: SemanticQueryText {
                        text: "invalidate sessions".to_string(),
                    },
                    filters: SemanticQueryFilters {
                        path_globs: vec!["src/**".to_string()],
                        ..SemanticQueryFilters::default()
                    },
                    limit: 10,
                    rerank_mode: SemanticRerankMode::Hybrid,
                },
            )
            .unwrap();
        let second = service
            .query(
                tempdir.path(),
                tempdir.path(),
                &config,
                &SemanticQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: SemanticQueryText {
                        text: "invalidate sessions".to_string(),
                    },
                    filters: SemanticQueryFilters {
                        path_globs: vec!["src/**".to_string()],
                        ..SemanticQueryFilters::default()
                    },
                    limit: 10,
                    rerank_mode: SemanticRerankMode::Hybrid,
                },
            )
            .unwrap();

        assert!(!first.hits.is_empty());
        assert!(
            first
                .hits
                .iter()
                .all(|hit| hit.chunk.path.starts_with("src/"))
        );
        assert_eq!(
            first
                .hits
                .iter()
                .map(|hit| hit.chunk.chunk_id.0.clone())
                .collect::<Vec<_>>(),
            second
                .hits
                .iter()
                .map(|hit| hit.chunk.chunk_id.0.clone())
                .collect::<Vec<_>>()
        );
        assert!(first.stats.rerank_applied);
        assert!(
            first
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "semantic_rerank_applied")
        );
    }

    #[test]
    fn query_fails_clearly_when_vector_index_metadata_is_corrupt() {
        let service = SemanticService;
        let tempdir = tempdir().unwrap();
        let config = hyperindex_protocol::config::SemanticConfig::default();
        let snapshot = semantic_fixture_snapshot();
        let repo_store = RepoStore::open_in_memory().unwrap();
        repo_store.persist_manifest(&snapshot).unwrap();

        service
            .build(
                &repo_store,
                tempdir.path(),
                tempdir.path(),
                &config,
                &snapshot,
                &SemanticBuildParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    force: true,
                },
            )
            .unwrap();

        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let connection = rusqlite::Connection::open(&store.store_path).unwrap();
        connection
            .execute(
                "UPDATE semantic_vector_index_metadata SET vector_dimensions = ?1 WHERE snapshot_id = ?2",
                rusqlite::params![999u32, "snap-123"],
            )
            .unwrap();

        let error = service
            .query(
                tempdir.path(),
                tempdir.path(),
                &config,
                &SemanticQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: SemanticQueryText {
                        text: "invalidate sessions".to_string(),
                    },
                    filters: SemanticQueryFilters::default(),
                    limit: 10,
                    rerank_mode: SemanticRerankMode::Off,
                },
            )
            .unwrap_err();

        assert!(error.message.contains("vector index dimensions mismatch"));
    }

    fn semantic_fixture_snapshot() -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: "snap-123".to_string(),
            repo_id: "repo-123".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "deadbeef".to_string(),
                digest: "base".to_string(),
                file_count: 2,
                files: vec![
                    SnapshotFile {
                        path: "src/session.ts".to_string(),
                        content_sha256: "sha-session".to_string(),
                        content_bytes: 118,
                        contents: r#"// Invalidates the active session.
export function invalidateSession(sessionId: string): string {
  return `session:${sessionId}`;
}
"#
                        .to_string(),
                    },
                    SnapshotFile {
                        path: "tests/session.test.ts".to_string(),
                        content_sha256: "sha-session-test".to_string(),
                        content_bytes: 121,
                        contents: r#"import { invalidateSession } from "../src/session";

describe("session", () => {
  it("invalidates sessions", () => expect(invalidateSession("1")).toContain("session"));
});
"#
                        .to_string(),
                    },
                ],
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }
}
