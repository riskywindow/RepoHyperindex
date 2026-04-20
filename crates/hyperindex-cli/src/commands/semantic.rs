use std::path::Path;
use std::time::Instant;

use hyperindex_config::load_or_default;
use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_daemon::semantic::SemanticService;
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::repo::RepoRecord;
use hyperindex_protocol::semantic::{
    SemanticBuildParams, SemanticBuildRecord, SemanticBuildResponse, SemanticBuildState,
    SemanticChunkRecord, SemanticInspectChunkParams, SemanticInspectChunkResponse,
    SemanticQueryFilters, SemanticQueryParams, SemanticQueryText, SemanticRerankMode,
    SemanticStatusParams,
};
use hyperindex_protocol::status::DaemonStatusParams;
use hyperindex_repo_store::RepoStore;
use hyperindex_semantic::cli_integration::{render_search_response, render_status_response};
use hyperindex_semantic_store::{SemanticBuildProfile, SemanticStore, StoredVectorIndexMetadata};
use hyperindex_symbol_store::SymbolStore;
use serde::Serialize;

use crate::client::DaemonClient;

pub fn status(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::SemanticStatus(SemanticStatusParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        build_id: None,
    }))? {
        SuccessPayload::SemanticStatus(response) => {
            render_status_response(&response, json_output).map_err(render_error)
        }
        other => Err(unexpected_response("semantic_status", other)),
    }
}

pub fn query(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    query: &str,
    limit: u32,
    path_globs: Vec<String>,
    rerank_mode: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::SemanticQuery(SemanticQueryParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        query: SemanticQueryText {
            text: query.to_string(),
        },
        filters: SemanticQueryFilters {
            path_globs,
            ..SemanticQueryFilters::default()
        },
        limit,
        rerank_mode: parse_rerank_mode(rerank_mode)?,
    }))? {
        SuccessPayload::SemanticQuery(response) => {
            render_search_response(&response, json_output).map_err(render_error)
        }
        other => Err(unexpected_response("semantic_query", other)),
    }
}

pub fn build(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    force: bool,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::SemanticBuild(SemanticBuildParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        force,
    }))? {
        SuccessPayload::SemanticBuild(response) => render_build_response(&response, json_output),
        other => Err(unexpected_response("semantic_build", other)),
    }
}

pub fn rebuild(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    if daemon_status(config_path).is_ok() {
        return build(config_path, repo_id, snapshot_id, true, json_output);
    }

    let context = LocalSemanticContext::load(config_path, repo_id, snapshot_id)?;
    let response = SemanticService
        .build(
            &context.repo_store,
            &context.loaded.config.semantic.store_dir,
            &context.loaded.config.symbol_index.store_dir,
            &context.loaded.config.semantic,
            &context.snapshot,
            &SemanticBuildParams {
                repo_id: repo_id.to_string(),
                snapshot_id: snapshot_id.to_string(),
                force: true,
            },
        )
        .map_err(|error| HyperindexError::Message(error.message))?;
    render_build_response(&response, json_output)
}

pub fn inspect_chunk(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    chunk_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::SemanticInspectChunk(
        SemanticInspectChunkParams {
            repo_id: repo_id.to_string(),
            snapshot_id: snapshot_id.to_string(),
            chunk_id: hyperindex_protocol::semantic::SemanticChunkId(chunk_id.to_string()),
            build_id: None,
        },
    ))? {
        SuccessPayload::SemanticInspectChunk(response) => {
            render_inspect_chunk_response(&response, json_output)
        }
        other => Err(unexpected_response("semantic_inspect_chunk", other)),
    }
}

pub fn inspect_index(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let context = LocalSemanticContext::load(config_path, repo_id, snapshot_id)?;
    let store = context.semantic_store()?;
    let build = store
        .load_build(snapshot_id)
        .map_err(store_error)?
        .ok_or_else(|| {
            HyperindexError::Message(format!("semantic build for {snapshot_id} was not found"))
        })?;
    let index_metadata = store
        .load_vector_index_metadata(snapshot_id)
        .map_err(store_error)?
        .ok_or_else(|| {
            HyperindexError::Message(format!(
                "semantic vector index for {snapshot_id} was not found"
            ))
        })?;
    let report = LocalSemanticInspectIndexReport {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        semantic_build_id: build.semantic_build_id.0.clone(),
        manifest: store.manifest_for(&build),
        index_metadata,
    };
    if json_output {
        return serde_json::to_string_pretty(&report).map_err(render_error);
    }
    Ok(render_local_inspect_index_report(&report))
}

pub fn stats(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let report = inspect_local_semantic_state(config_path, repo_id, snapshot_id)?;
    if json_output {
        return render_json(&report);
    }
    Ok(render_local_semantic_report("semantic stats", &report))
}

pub fn doctor(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let report = inspect_local_semantic_state(config_path, repo_id, snapshot_id)?;
    if json_output {
        return render_json(&report);
    }
    Ok(render_local_semantic_report("semantic doctor", &report))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSemanticIssue {
    code: &'static str,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSemanticStoreReport {
    db_path: String,
    schema_version: u32,
    build_count: usize,
    quick_check: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSemanticBuildReport {
    materialized: bool,
    query_ready: bool,
    stale: bool,
    semantic_build_id: Option<String>,
    refresh_mode: Option<String>,
    fallback_reason: Option<String>,
    chunk_count: usize,
    embedding_count: usize,
    cache_hits: usize,
    cache_misses: usize,
    cache_writes: usize,
    cache_hit_rate_percent: Option<u32>,
    files_touched: u64,
    chunks_rebuilt: u64,
    embeddings_regenerated: u64,
    vector_entries_added: u64,
    vector_entries_updated: u64,
    vector_entries_removed: u64,
    elapsed_ms: u64,
    vector_index_present: bool,
    vector_index_schema_version: Option<u32>,
    vector_dimensions: Option<u32>,
    indexed_vector_count: u64,
    provider_available: bool,
    provider_summary: String,
    build_profile: Option<SemanticBuildProfile>,
    vector_warm_load_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSemanticReport {
    daemon_reachable: bool,
    repo_id: String,
    snapshot_id: String,
    repo_last_snapshot_id: Option<String>,
    symbol_index_ready: bool,
    store: Option<LocalSemanticStoreReport>,
    build: LocalSemanticBuildReport,
    actions: Vec<String>,
    issues: Vec<LocalSemanticIssue>,
}

fn inspect_local_semantic_state(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
) -> HyperindexResult<LocalSemanticReport> {
    let context = LocalSemanticContext::load(config_path, repo_id, snapshot_id)?;
    let daemon_reachable = daemon_status(config_path).is_ok();
    let store = context.semantic_store()?;
    let status = store.status().map_err(store_error)?;
    let quick_check = store.quick_check().map_err(store_error)?;
    let build = store.load_build(snapshot_id).map_err(store_error)?;
    let index_metadata = store
        .load_vector_index_metadata(snapshot_id)
        .map_err(store_error)?;
    let symbol_index_ready = SymbolStore::open(
        &context.loaded.config.symbol_index.store_dir,
        &context.repo.repo_id,
    )
    .map_err(store_error)?
    .load_indexed_snapshot_state(snapshot_id)
    .map_err(store_error)?
    .is_some();
    let provider_summary = format!(
        "{:?}:{}",
        context
            .loaded
            .config
            .semantic
            .embedding_provider
            .provider_kind,
        context.loaded.config.semantic.embedding_provider.model_id
    )
    .to_lowercase();
    let provider_available =
        hyperindex_semantic::provider_from_config(&context.loaded.config.semantic).is_ok();
    let stale = build.as_ref().is_some_and(|build| {
        build.schema_version != status.schema_version
            || build.chunk_schema_version != context.loaded.config.semantic.chunk_schema_version
            || build.semantic_config_digest
                != hyperindex_semantic::SemanticScaffoldBuilder::from_config(
                    &context.loaded.config.semantic,
                )
                .config_digest()
    });
    let vector_warm_load_ms = if let Some(build) = build.as_ref() {
        let started = Instant::now();
        match store.load_vector_index(snapshot_id, build) {
            Ok(_) => Some(started.elapsed().as_millis() as u64),
            Err(_) => None,
        }
    } else {
        None
    };
    let cache_hit_rate_percent = build.as_ref().and_then(|build| {
        let total = build.embedding_cache_hits + build.embedding_cache_misses;
        (total > 0).then_some(((build.embedding_cache_hits * 100) / total) as u32)
    });

    let mut actions = Vec::new();
    let mut issues = Vec::new();
    if !quick_check.iter().all(|result| result == "ok") {
        issues.push(LocalSemanticIssue {
            code: "store_quick_check_failed",
            message: format!(
                "semantic store quick_check reported {}; clean stale runtime state and rebuild",
                quick_check.join(", ")
            ),
        });
        actions.push(format!(
            "hyperctl semantic rebuild --repo_id {} --snapshot_id {}",
            repo_id, snapshot_id
        ));
        actions.push("hyperctl cleanup".to_string());
    }
    if build.is_none() {
        issues.push(LocalSemanticIssue {
            code: "semantic_build_missing",
            message: "no semantic build is materialized for this snapshot".to_string(),
        });
        actions.push(format!(
            "hyperctl semantic rebuild --repo_id {} --snapshot_id {}",
            repo_id, snapshot_id
        ));
    }
    if stale {
        issues.push(LocalSemanticIssue {
            code: "semantic_build_stale",
            message: "stored semantic build is stale for the current schema/config".to_string(),
        });
        actions.push(format!(
            "hyperctl semantic rebuild --repo_id {} --snapshot_id {}",
            repo_id, snapshot_id
        ));
    }
    if build.is_some() && index_metadata.is_none() {
        issues.push(LocalSemanticIssue {
            code: "vector_index_missing",
            message: "semantic build metadata exists but vector index metadata is missing"
                .to_string(),
        });
    }
    if build.is_some() && vector_warm_load_ms.is_none() {
        issues.push(LocalSemanticIssue {
            code: "vector_index_unloadable",
            message: "persisted semantic vector index could not be warm-loaded cleanly".to_string(),
        });
    }
    if !provider_available {
        issues.push(LocalSemanticIssue {
            code: "embedding_provider_unavailable",
            message: format!(
                "embedding provider {} is unavailable; query execution will fail until the provider is fixed",
                provider_summary
            ),
        });
    }
    if !symbol_index_ready {
        issues.push(LocalSemanticIssue {
            code: "symbol_index_missing",
            message:
                "symbol index is not materialized for this snapshot; rebuild quality may degrade"
                    .to_string(),
        });
    }
    if context.repo.last_snapshot_id.as_deref() != Some(snapshot_id) {
        actions.push(format!(
            "consider rebuilding semantic state for the repo head snapshot {}",
            context.repo.last_snapshot_id.as_deref().unwrap_or("-")
        ));
    }

    Ok(LocalSemanticReport {
        daemon_reachable,
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        repo_last_snapshot_id: context.repo.last_snapshot_id.clone(),
        symbol_index_ready,
        store: Some(LocalSemanticStoreReport {
            db_path: status.db_path,
            schema_version: status.schema_version,
            build_count: status.build_count,
            quick_check,
        }),
        build: LocalSemanticBuildReport {
            materialized: build.is_some(),
            query_ready: build.is_some()
                && !stale
                && index_metadata.is_some()
                && vector_warm_load_ms.is_some()
                && provider_available,
            stale,
            semantic_build_id: build
                .as_ref()
                .map(|build| build.semantic_build_id.0.clone()),
            refresh_mode: build.as_ref().map(|build| build.refresh_mode.clone()),
            fallback_reason: build
                .as_ref()
                .and_then(|build| build.fallback_reason.clone()),
            chunk_count: build.as_ref().map(|build| build.chunk_count).unwrap_or(0),
            embedding_count: build
                .as_ref()
                .map(|build| build.embedding_count)
                .unwrap_or(0),
            cache_hits: build
                .as_ref()
                .map(|build| build.embedding_cache_hits)
                .unwrap_or(0),
            cache_misses: build
                .as_ref()
                .map(|build| build.embedding_cache_misses)
                .unwrap_or(0),
            cache_writes: build
                .as_ref()
                .map(|build| build.embedding_cache_writes)
                .unwrap_or(0),
            cache_hit_rate_percent,
            files_touched: build
                .as_ref()
                .and_then(|build| {
                    build
                        .refresh_stats
                        .as_ref()
                        .map(|stats| stats.files_touched)
                })
                .unwrap_or(0),
            chunks_rebuilt: build
                .as_ref()
                .and_then(|build| {
                    build
                        .refresh_stats
                        .as_ref()
                        .map(|stats| stats.chunks_rebuilt)
                })
                .unwrap_or(0),
            embeddings_regenerated: build
                .as_ref()
                .and_then(|build| {
                    build
                        .refresh_stats
                        .as_ref()
                        .map(|stats| stats.embeddings_regenerated)
                })
                .unwrap_or(0),
            vector_entries_added: build
                .as_ref()
                .and_then(|build| {
                    build
                        .refresh_stats
                        .as_ref()
                        .map(|stats| stats.vector_entries_added)
                })
                .unwrap_or(0),
            vector_entries_updated: build
                .as_ref()
                .and_then(|build| {
                    build
                        .refresh_stats
                        .as_ref()
                        .map(|stats| stats.vector_entries_updated)
                })
                .unwrap_or(0),
            vector_entries_removed: build
                .as_ref()
                .and_then(|build| {
                    build
                        .refresh_stats
                        .as_ref()
                        .map(|stats| stats.vector_entries_removed)
                })
                .unwrap_or(0),
            elapsed_ms: build
                .as_ref()
                .and_then(|build| build.refresh_stats.as_ref().map(|stats| stats.elapsed_ms))
                .unwrap_or(0),
            vector_index_present: index_metadata.is_some(),
            vector_index_schema_version: index_metadata
                .as_ref()
                .map(|metadata| metadata.index_schema_version),
            vector_dimensions: index_metadata
                .as_ref()
                .map(|metadata| metadata.vector_dimensions),
            indexed_vector_count: index_metadata
                .as_ref()
                .map(|metadata| metadata.indexed_vector_count)
                .unwrap_or(0),
            provider_available,
            provider_summary,
            build_profile: build.as_ref().and_then(|build| build.profile.clone()),
            vector_warm_load_ms,
        },
        actions,
        issues,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSemanticInspectReport {
    repo_id: String,
    snapshot_id: String,
    semantic_build_id: String,
    chunk: SemanticChunkRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSemanticInspectIndexReport {
    repo_id: String,
    snapshot_id: String,
    semantic_build_id: String,
    manifest: hyperindex_protocol::semantic::SemanticIndexManifest,
    index_metadata: StoredVectorIndexMetadata,
}

struct LocalSemanticContext {
    loaded: hyperindex_config::LoadedConfig,
    repo_store: RepoStore,
    repo: RepoRecord,
    snapshot: hyperindex_protocol::snapshot::ComposedSnapshot,
}

impl LocalSemanticContext {
    fn load(
        config_path: Option<&Path>,
        repo_id: &str,
        snapshot_id: &str,
    ) -> HyperindexResult<Self> {
        let loaded = load_or_default(config_path)?;
        let repo_store = RepoStore::open_from_config(&loaded.config)?;
        let repo = repo_store.show_repo(repo_id)?;
        let snapshot = repo_store.load_manifest(snapshot_id)?.ok_or_else(|| {
            HyperindexError::Message(format!("snapshot {snapshot_id} was not found"))
        })?;
        if snapshot.repo_id != repo_id {
            return Err(HyperindexError::Message(format!(
                "snapshot {snapshot_id} does not belong to repo {repo_id}"
            )));
        }
        Ok(Self {
            loaded,
            repo_store,
            repo,
            snapshot,
        })
    }

    fn semantic_store(&self) -> HyperindexResult<SemanticStore> {
        SemanticStore::open_in_store_dir(
            &self.loaded.config.semantic.store_dir,
            &self.snapshot.repo_id,
        )
        .map_err(store_error)
    }
}

fn parse_rerank_mode(raw: &str) -> HyperindexResult<SemanticRerankMode> {
    match raw {
        "off" => Ok(SemanticRerankMode::Off),
        "hybrid" => Ok(SemanticRerankMode::Hybrid),
        other => Err(HyperindexError::Message(format!(
            "unsupported rerank mode {other}; expected off or hybrid"
        ))),
    }
}

fn render_error(error: serde_json::Error) -> HyperindexError {
    HyperindexError::Message(format!("failed to render semantic output: {error}"))
}

fn store_error(error: impl std::fmt::Display) -> HyperindexError {
    HyperindexError::Message(error.to_string())
}

fn render_json<T: Serialize>(response: &T) -> HyperindexResult<String> {
    serde_json::to_string_pretty(response)
        .map_err(|error| HyperindexError::Message(format!("failed to render json: {error}")))
}

fn unexpected_response(method: &str, payload: SuccessPayload) -> HyperindexError {
    HyperindexError::Message(format!("unexpected {method} response: {payload:?}"))
}

fn render_build_response(
    response: &SemanticBuildResponse,
    json_output: bool,
) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    Ok(render_build_summary("semantic build", &response.build))
}

fn render_inspect_chunk_response(
    response: &SemanticInspectChunkResponse,
    json_output: bool,
) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    Ok(render_local_inspect_report(&LocalSemanticInspectReport {
        repo_id: response.repo_id.clone(),
        snapshot_id: response.snapshot_id.clone(),
        semantic_build_id: response
            .manifest
            .as_ref()
            .map(|manifest| manifest.build_id.0.clone())
            .unwrap_or_else(|| "-".to_string()),
        chunk: response.chunk.clone(),
    }))
}

fn render_build_summary(title: &str, build: &SemanticBuildRecord) -> String {
    format!(
        "{title} {}\nstate: {}\nchunk_count: {}\nindexed_file_count: {}\nembedding_count: {}\nrefresh_mode: {}\nfallback_reason: {}\nloaded_from_existing_build: {}",
        build.build_id.0,
        match build.state {
            SemanticBuildState::Queued => "queued",
            SemanticBuildState::Running => "running",
            SemanticBuildState::Succeeded => "succeeded",
            SemanticBuildState::Failed => "failed",
        },
        build
            .manifest
            .as_ref()
            .map(|manifest| manifest.indexed_chunk_count.to_string())
            .unwrap_or_else(|| "-".to_string()),
        build
            .manifest
            .as_ref()
            .map(|manifest| manifest.indexed_file_count.to_string())
            .unwrap_or_else(|| "-".to_string()),
        build
            .manifest
            .as_ref()
            .map(|manifest| manifest.embedding_cache.entry_count.to_string())
            .unwrap_or_else(|| "-".to_string()),
        build.refresh_mode.as_deref().unwrap_or("-"),
        build.fallback_reason.as_deref().unwrap_or("-"),
        build.loaded_from_existing_build,
    )
}

fn render_local_inspect_report(report: &LocalSemanticInspectReport) -> String {
    [
        format!("semantic inspect-chunk {}", report.snapshot_id),
        format!("repo_id: {}", report.repo_id),
        format!("semantic_build_id: {}", report.semantic_build_id),
        format!("chunk_id: {}", report.chunk.metadata.chunk_id.0),
        format!("path: {}", report.chunk.metadata.path),
        format!(
            "symbol: {}",
            report
                .chunk
                .metadata
                .symbol_display_name
                .clone()
                .unwrap_or_else(|| "-".to_string())
        ),
        format!("chunk_kind: {:?}", report.chunk.metadata.chunk_kind).to_lowercase(),
        format!("source_kind: {:?}", report.chunk.metadata.source_kind).to_lowercase(),
        String::new(),
        report.chunk.serialized_text.clone(),
    ]
    .join("\n")
}

fn render_local_inspect_index_report(report: &LocalSemanticInspectIndexReport) -> String {
    [
        format!("semantic inspect-index {}", report.snapshot_id),
        format!("repo_id: {}", report.repo_id),
        format!("semantic_build_id: {}", report.semantic_build_id),
        format!("store_path: {}", report.manifest.storage.path),
        format!("index_kind: {}", report.index_metadata.index_kind),
        format!(
            "index_schema_version: {}",
            report.index_metadata.index_schema_version
        ),
        format!(
            "vector_dimensions: {}",
            report.index_metadata.vector_dimensions
        ),
        format!("normalized: {}", report.index_metadata.normalized),
        format!(
            "indexed_vector_count: {}",
            report.index_metadata.indexed_vector_count
        ),
        format!("created_at: {}", report.index_metadata.created_at),
    ]
    .join("\n")
}

fn render_local_semantic_report(title: &str, report: &LocalSemanticReport) -> String {
    let mut lines = vec![
        format!("{title} {}", report.snapshot_id),
        format!("daemon_reachable: {}", report.daemon_reachable),
        format!(
            "repo_last_snapshot_id: {}",
            report.repo_last_snapshot_id.as_deref().unwrap_or("-")
        ),
        format!("symbol_index_ready: {}", report.symbol_index_ready),
        format!("build_materialized: {}", report.build.materialized),
        format!("query_ready: {}", report.build.query_ready),
        format!("stale: {}", report.build.stale),
        format!(
            "semantic_build_id: {}",
            report.build.semantic_build_id.as_deref().unwrap_or("-")
        ),
        format!("provider: {}", report.build.provider_summary),
        format!("provider_available: {}", report.build.provider_available),
        format!("chunk_count: {}", report.build.chunk_count),
        format!("embedding_count: {}", report.build.embedding_count),
        format!("cache_hits: {}", report.build.cache_hits),
        format!("cache_misses: {}", report.build.cache_misses),
        format!("cache_writes: {}", report.build.cache_writes),
        format!(
            "cache_hit_rate_percent: {}",
            report
                .build
                .cache_hit_rate_percent
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "refresh_mode: {}",
            report.build.refresh_mode.as_deref().unwrap_or("-")
        ),
        format!(
            "fallback_reason: {}",
            report.build.fallback_reason.as_deref().unwrap_or("-")
        ),
        format!("files_touched: {}", report.build.files_touched),
        format!("chunks_rebuilt: {}", report.build.chunks_rebuilt),
        format!(
            "embeddings_regenerated: {}",
            report.build.embeddings_regenerated
        ),
        format!("elapsed_ms: {}", report.build.elapsed_ms),
        format!(
            "vector_index_present: {}",
            report.build.vector_index_present
        ),
        format!(
            "vector_warm_load_ms: {}",
            report
                .build
                .vector_warm_load_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
    ];
    if let Some(store) = &report.store {
        lines.push(format!("store_path: {}", store.db_path));
        lines.push(format!("schema_version: {}", store.schema_version));
        lines.push(format!("build_count: {}", store.build_count));
        lines.push(format!("quick_check: {}", store.quick_check.join(", ")));
    }
    if let Some(profile) = &report.build.build_profile {
        lines.push(format!(
            "build_profile: chunk_materialization_ms={} embedding_resolution_ms={} vector_persist_ms={}",
            profile.chunk_materialization_ms,
            profile.embedding_resolution_ms,
            profile.vector_persist_ms
        ));
    }
    if report.actions.is_empty() {
        lines.push("actions: -".to_string());
    } else {
        lines.push(format!("actions: {}", report.actions.join("; ")));
    }
    if report.issues.is_empty() {
        lines.push("issues: none".to_string());
    } else {
        lines.push(format!("issues: {}", report.issues.len()));
        lines.extend(
            report
                .issues
                .iter()
                .map(|issue| format!("- [{}] {}", issue.code, issue.message)),
        );
    }
    lines.join("\n")
}

fn daemon_status(config_path: Option<&Path>) -> HyperindexResult<()> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::DaemonStatus(DaemonStatusParams::default()))? {
        SuccessPayload::DaemonStatus(_) => Ok(()),
        other => Err(HyperindexError::Message(format!(
            "unexpected daemon status response: {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::semantic::{
        SemanticBuildId, SemanticBuildRecord, SemanticBuildState, SemanticIndexManifest,
    };

    use super::{
        LocalSemanticBuildReport, LocalSemanticIssue, LocalSemanticReport,
        LocalSemanticStoreReport, parse_rerank_mode, render_build_summary,
        render_local_semantic_report,
    };

    #[test]
    fn rerank_mode_parser_accepts_phase6_values() {
        assert!(matches!(
            parse_rerank_mode("off").unwrap(),
            hyperindex_protocol::semantic::SemanticRerankMode::Off
        ));
        assert!(matches!(
            parse_rerank_mode("hybrid").unwrap(),
            hyperindex_protocol::semantic::SemanticRerankMode::Hybrid
        ));
    }

    #[test]
    fn build_summary_reports_reused_builds() {
        let rendered = render_build_summary(
            "semantic build",
            &SemanticBuildRecord {
                build_id: SemanticBuildId("semantic-build-1".to_string()),
                state: SemanticBuildState::Succeeded,
                requested_at: "epoch-ms:1".to_string(),
                started_at: Some("epoch-ms:1".to_string()),
                finished_at: Some("epoch-ms:2".to_string()),
                manifest: Some(SemanticIndexManifest {
                    build_id: SemanticBuildId("semantic-build-1".to_string()),
                    repo_id: "repo-1".to_string(),
                    snapshot_id: "snap-1".to_string(),
                    semantic_config_digest: "cfg".to_string(),
                    chunk_schema_version: 1,
                    symbol_index_build_id: None,
                    embedding_provider:
                        hyperindex_protocol::semantic::SemanticEmbeddingProviderConfig {
                            provider_kind:
                                hyperindex_protocol::semantic::SemanticEmbeddingProviderKind::DeterministicFixture,
                            model_id: "fixture".to_string(),
                            model_digest: "fixture".to_string(),
                            vector_dimensions: 384,
                            normalized: true,
                            max_input_bytes: 8192,
                            max_batch_size: 32,
                        },
                    chunk_text: hyperindex_protocol::semantic::SemanticChunkTextConfig {
                        serializer_id: "phase6-structured-text".to_string(),
                        format_version: 1,
                        includes_path_header: true,
                        includes_symbol_context: true,
                        normalized_newlines: true,
                    },
                    storage: hyperindex_protocol::semantic::SemanticIndexStorage {
                        format: hyperindex_protocol::semantic::SemanticStorageFormat::Sqlite,
                        path: "/tmp/semantic.sqlite3".to_string(),
                        schema_version: 1,
                        manifest_sha256: None,
                    },
                    embedding_cache:
                        hyperindex_protocol::semantic::SemanticEmbeddingCacheManifest {
                            key_algorithm: "sha256".to_string(),
                            entry_count: 7,
                            store_path: Some("/tmp/semantic.sqlite3".to_string()),
                        },
                    indexed_chunk_count: 7,
                    indexed_file_count: 3,
                    created_at: "epoch-ms:2".to_string(),
                }),
                refresh_stats: None,
                refresh_mode: Some("warm_load".to_string()),
                fallback_reason: None,
                diagnostics: Vec::new(),
                loaded_from_existing_build: true,
            },
        );

        assert!(rendered.contains("loaded_from_existing_build: true"));
        assert!(rendered.contains("chunk_count: 7"));
    }

    #[test]
    fn local_semantic_report_renders_actions_issues_and_profiles() {
        let rendered = render_local_semantic_report(
            "semantic doctor",
            &LocalSemanticReport {
                daemon_reachable: false,
                repo_id: "repo-1".to_string(),
                snapshot_id: "snap-1".to_string(),
                repo_last_snapshot_id: Some("snap-2".to_string()),
                symbol_index_ready: true,
                store: Some(LocalSemanticStoreReport {
                    db_path: "/tmp/semantic.sqlite3".to_string(),
                    schema_version: 4,
                    build_count: 1,
                    quick_check: vec!["ok".to_string()],
                }),
                build: LocalSemanticBuildReport {
                    materialized: true,
                    query_ready: false,
                    stale: true,
                    semantic_build_id: Some("semantic-build-1".to_string()),
                    refresh_mode: Some("incremental".to_string()),
                    fallback_reason: Some("cache_or_index_corruption".to_string()),
                    chunk_count: 7,
                    embedding_count: 7,
                    cache_hits: 5,
                    cache_misses: 2,
                    cache_writes: 1,
                    cache_hit_rate_percent: Some(71),
                    files_touched: 1,
                    chunks_rebuilt: 2,
                    embeddings_regenerated: 1,
                    vector_entries_added: 1,
                    vector_entries_updated: 1,
                    vector_entries_removed: 0,
                    elapsed_ms: 44,
                    vector_index_present: true,
                    vector_index_schema_version: Some(1),
                    vector_dimensions: Some(384),
                    indexed_vector_count: 7,
                    provider_available: false,
                    provider_summary: "external_process:missing".to_string(),
                    build_profile: Some(hyperindex_semantic_store::SemanticBuildProfile {
                        chunk_materialization_ms: 11,
                        embedding_resolution_ms: 12,
                        vector_persist_ms: 13,
                    }),
                    vector_warm_load_ms: Some(9),
                },
                actions: vec![
                    "hyperctl semantic rebuild --repo_id repo-1 --snapshot_id snap-1".to_string(),
                ],
                issues: vec![LocalSemanticIssue {
                    code: "embedding_provider_unavailable",
                    message: "provider is unavailable".to_string(),
                }],
            },
        );

        assert!(rendered.contains("semantic doctor snap-1"));
        assert!(rendered.contains("build_profile: chunk_materialization_ms=11"));
        assert!(rendered.contains("actions: hyperctl semantic rebuild"));
        assert!(rendered.contains("[embedding_provider_unavailable] provider is unavailable"));
    }
}
