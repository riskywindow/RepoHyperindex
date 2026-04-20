use std::path::Path;

use hyperindex_config::load_or_default;
use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::semantic::{
    SemanticChunkRecord, SemanticQueryFilters, SemanticQueryParams, SemanticQueryText,
    SemanticRerankMode, SemanticStatusParams,
};
use hyperindex_repo_store::RepoStore;
use hyperindex_semantic::SemanticScaffoldBuilder;
use hyperindex_semantic::cli_integration::{
    render_local_report, render_search_response, render_status_response,
};
use hyperindex_semantic_store::{SemanticStore, StoredSemanticBuild, StoredVectorIndexMetadata};
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

pub fn search(
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

pub fn rebuild(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let context = LocalSemanticContext::load(config_path, repo_id, snapshot_id)?;
    let symbol_build_id = context
        .symbol_store()?
        .load_indexed_snapshot_state(snapshot_id)
        .map_err(store_error)?
        .map(|state| {
            hyperindex_protocol::symbols::SymbolIndexBuildId(format!(
                "symbol-index-scaffold-{}",
                state.snapshot_id
            ))
        });
    let builder = SemanticScaffoldBuilder::from_config(&context.loaded.config.semantic);
    let store = context.semantic_store()?;
    let draft = builder
        .build(&context.snapshot, symbol_build_id, &store)
        .map_err(store_error)?;
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
        diagnostics: draft.diagnostics.clone(),
    };
    store
        .persist_build_with_chunks_and_vectors(&build, &draft.chunks, &draft.chunk_vectors)
        .map_err(store_error)?;
    let report = LocalSemanticRebuildReport {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        store_path: store.store_path.display().to_string(),
        semantic_build_id: build.semantic_build_id.0.clone(),
        chunk_count: build.chunk_count,
        embedding_count: build.embedding_count,
        cache_hits: build.embedding_cache_hits,
        cache_misses: build.embedding_cache_misses,
        cache_writes: build.embedding_cache_writes,
        diagnostics: build
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.clone())
            .collect(),
    };
    render_local_report(
        &report,
        &format!("semantic rebuild {snapshot_id}"),
        &[
            format!("repo_id: {}", report.repo_id),
            format!("store_path: {}", report.store_path),
            format!("semantic_build_id: {}", report.semantic_build_id),
            format!("chunk_count: {}", report.chunk_count),
            format!("embedding_count: {}", report.embedding_count),
            format!("cache_hits: {}", report.cache_hits),
            format!("cache_misses: {}", report.cache_misses),
            format!("cache_writes: {}", report.cache_writes),
            format!(
                "diagnostics: {}",
                if report.diagnostics.is_empty() {
                    "-".to_string()
                } else {
                    report.diagnostics.join(", ")
                }
            ),
        ],
        json_output,
    )
    .map_err(render_error)
}

pub fn inspect_chunk(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    chunk_id: &str,
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
    let chunk = store
        .load_chunk(
            snapshot_id,
            &hyperindex_protocol::semantic::SemanticChunkId(chunk_id.to_string()),
        )
        .map_err(store_error)?
        .ok_or_else(|| {
            HyperindexError::Message(format!("semantic chunk {chunk_id} was not found"))
        })?;
    let report = LocalSemanticInspectReport {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        semantic_build_id: build.semantic_build_id.0.clone(),
        chunk,
    };
    if json_output {
        return serde_json::to_string_pretty(&report).map_err(render_error);
    }
    Ok(render_local_inspect_report(&report))
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
struct LocalSemanticRebuildReport {
    repo_id: String,
    snapshot_id: String,
    store_path: String,
    semantic_build_id: String,
    chunk_count: usize,
    embedding_count: usize,
    cache_hits: usize,
    cache_misses: usize,
    cache_writes: usize,
    diagnostics: Vec<String>,
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

    fn symbol_store(&self) -> HyperindexResult<SymbolStore> {
        SymbolStore::open(
            &self.loaded.config.symbol_index.store_dir,
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

fn unexpected_response(method: &str, payload: SuccessPayload) -> HyperindexError {
    HyperindexError::Message(format!("unexpected {method} response: {payload:?}"))
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
    use super::parse_rerank_mode;

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
}
