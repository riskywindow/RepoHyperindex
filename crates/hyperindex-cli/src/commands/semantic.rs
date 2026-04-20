use std::path::Path;

use hyperindex_config::load_or_default;
use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::semantic::{
    SemanticBuildParams, SemanticBuildRecord, SemanticBuildResponse, SemanticBuildState,
    SemanticChunkRecord, SemanticInspectChunkParams, SemanticInspectChunkResponse,
    SemanticQueryFilters, SemanticQueryParams, SemanticQueryText, SemanticRerankMode,
    SemanticStatusParams,
};
use hyperindex_repo_store::RepoStore;
use hyperindex_semantic::cli_integration::{
    render_local_report, render_search_response, render_status_response,
};
use hyperindex_semantic_store::{SemanticStore, StoredVectorIndexMetadata};
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
    let context = LocalSemanticContext::load(config_path, repo_id, snapshot_id)?;
    let store = context.semantic_store()?;
    let status = store.status().map_err(store_error)?;
    let quick_check = store.quick_check().map_err(store_error)?;
    let build = store.load_build(snapshot_id).map_err(store_error)?;
    let index_metadata = store
        .load_vector_index_metadata(snapshot_id)
        .map_err(store_error)?;
    let report = LocalSemanticStatsReport {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        store_path: status.db_path.clone(),
        schema_version: status.schema_version,
        build_count: status.build_count,
        quick_check,
        build_present: build.is_some(),
        semantic_build_id: build
            .as_ref()
            .map(|build| build.semantic_build_id.0.clone()),
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
        refresh_mode: build.as_ref().map(|build| build.refresh_mode.clone()),
        fallback_reason: build
            .as_ref()
            .and_then(|build| build.fallback_reason.clone()),
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
    };
    render_local_report(
        &report,
        &format!("semantic stats {snapshot_id}"),
        &[
            format!("repo_id: {}", report.repo_id),
            format!("store_path: {}", report.store_path),
            format!("schema_version: {}", report.schema_version),
            format!("build_count: {}", report.build_count),
            format!("build_present: {}", report.build_present),
            format!(
                "semantic_build_id: {}",
                report
                    .semantic_build_id
                    .clone()
                    .unwrap_or_else(|| "-".to_string())
            ),
            format!("chunk_count: {}", report.chunk_count),
            format!("embedding_count: {}", report.embedding_count),
            format!("cache_hits: {}", report.cache_hits),
            format!("cache_misses: {}", report.cache_misses),
            format!("cache_writes: {}", report.cache_writes),
            format!(
                "refresh_mode: {}",
                report
                    .refresh_mode
                    .clone()
                    .unwrap_or_else(|| "-".to_string())
            ),
            format!(
                "fallback_reason: {}",
                report
                    .fallback_reason
                    .clone()
                    .unwrap_or_else(|| "-".to_string())
            ),
            format!("files_touched: {}", report.files_touched),
            format!("chunks_rebuilt: {}", report.chunks_rebuilt),
            format!("embeddings_regenerated: {}", report.embeddings_regenerated),
            format!("vector_entries_added: {}", report.vector_entries_added),
            format!("vector_entries_updated: {}", report.vector_entries_updated),
            format!("vector_entries_removed: {}", report.vector_entries_removed),
            format!("elapsed_ms: {}", report.elapsed_ms),
            format!("vector_index_present: {}", report.vector_index_present),
            format!(
                "vector_index_schema_version: {}",
                report
                    .vector_index_schema_version
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string())
            ),
            format!(
                "vector_dimensions: {}",
                report
                    .vector_dimensions
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string())
            ),
            format!("indexed_vector_count: {}", report.indexed_vector_count),
            format!("quick_check: {}", report.quick_check.join(", ")),
        ],
        json_output,
    )
    .map_err(render_error)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSemanticStatsReport {
    repo_id: String,
    snapshot_id: String,
    store_path: String,
    schema_version: u32,
    build_count: usize,
    quick_check: Vec<String>,
    build_present: bool,
    semantic_build_id: Option<String>,
    chunk_count: usize,
    embedding_count: usize,
    cache_hits: usize,
    cache_misses: usize,
    cache_writes: usize,
    refresh_mode: Option<String>,
    fallback_reason: Option<String>,
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
        repo_store.show_repo(repo_id)?;
        let snapshot = repo_store.load_manifest(snapshot_id)?.ok_or_else(|| {
            HyperindexError::Message(format!("snapshot {snapshot_id} was not found"))
        })?;
        if snapshot.repo_id != repo_id {
            return Err(HyperindexError::Message(format!(
                "snapshot {snapshot_id} does not belong to repo {repo_id}"
            )));
        }
        Ok(Self { loaded, snapshot })
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

#[cfg(test)]
mod tests {
    use hyperindex_protocol::semantic::{
        SemanticBuildId, SemanticBuildRecord, SemanticBuildState, SemanticIndexManifest,
    };

    use super::{parse_rerank_mode, render_build_summary};

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
}
