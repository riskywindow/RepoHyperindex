use std::collections::BTreeSet;
use std::path::Path;

use hyperindex_protocol::config::SemanticConfig;
use hyperindex_protocol::semantic::{
    SemanticBuildId, SemanticChunkRecord, SemanticDiagnostic, SemanticDiagnosticSeverity,
    SemanticEmbeddingCacheManifest, SemanticEmbeddingProviderConfig, SemanticIndexManifest,
    SemanticIndexStorage, SemanticStorageFormat,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::symbols::SymbolIndexBuildId;
use hyperindex_semantic_store::{EmbeddingCacheStore, StoredChunkVector};
use tracing::info;

use crate::EmbeddingPipeline;
use crate::SemanticResult;
use crate::chunker::ScaffoldChunker;
use crate::common::{stable_digest, unix_timestamp_string};
use crate::embedding_pipeline::EmbeddingPipelineStats;
use crate::embedding_provider::{
    provider_config_digest, provider_from_config, provider_identity_from_config,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticBuildDraft {
    pub repo_id: String,
    pub snapshot_id: String,
    pub semantic_build_id: SemanticBuildId,
    pub semantic_config_digest: String,
    pub embedding_provider: SemanticEmbeddingProviderConfig,
    pub chunk_text: hyperindex_protocol::semantic::SemanticChunkTextConfig,
    pub chunk_schema_version: u32,
    pub symbol_index_build_id: Option<SymbolIndexBuildId>,
    pub created_at: String,
    pub refresh_mode: String,
    pub chunk_count: usize,
    pub indexed_file_count: usize,
    pub embedding_count: usize,
    pub embedding_stats: EmbeddingPipelineStats,
    pub diagnostics: Vec<SemanticDiagnostic>,
    pub chunks: Vec<SemanticChunkRecord>,
    pub chunk_vectors: Vec<StoredChunkVector>,
}

impl SemanticBuildDraft {
    pub fn manifest(&self, store_path: &Path) -> SemanticIndexManifest {
        SemanticIndexManifest {
            build_id: self.semantic_build_id.clone(),
            repo_id: self.repo_id.clone(),
            snapshot_id: self.snapshot_id.clone(),
            semantic_config_digest: self.semantic_config_digest.clone(),
            chunk_schema_version: self.chunk_schema_version,
            symbol_index_build_id: self.symbol_index_build_id.clone(),
            embedding_provider: self.embedding_provider.clone(),
            chunk_text: self.chunk_text.clone(),
            storage: SemanticIndexStorage {
                format: SemanticStorageFormat::Sqlite,
                path: store_path.display().to_string(),
                schema_version: 1,
                manifest_sha256: None,
            },
            embedding_cache: SemanticEmbeddingCacheManifest {
                key_algorithm:
                    "sha256(input_kind + text_digest + provider_identity + provider_config_digest)"
                        .to_string(),
                entry_count: self.embedding_count as u64,
                store_path: Some(store_path.display().to_string()),
            },
            indexed_chunk_count: self.chunk_count as u64,
            indexed_file_count: self.indexed_file_count as u64,
            created_at: self.created_at.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SemanticScaffoldBuilder {
    config: SemanticConfig,
    chunker: ScaffoldChunker,
}

impl SemanticScaffoldBuilder {
    pub fn from_config(config: &SemanticConfig) -> Self {
        Self {
            config: config.clone(),
            chunker: ScaffoldChunker::new(config.chunk_schema_version, config.chunk_text.clone()),
        }
    }

    pub fn config_digest(&self) -> String {
        let provider_identity = provider_identity_from_config(&self.config);
        stable_digest(&[
            &self.config.chunk_schema_version.to_string(),
            &provider_config_digest(&provider_identity, &self.config.embedding_provider),
            &self.config.chunk_text.serializer_id,
            &self.config.chunk_text.format_version.to_string(),
            &self.config.chunk_text.includes_path_header.to_string(),
            &self.config.chunk_text.includes_symbol_context.to_string(),
            &self.config.chunk_text.normalized_newlines.to_string(),
        ])
    }

    pub fn build<C: EmbeddingCacheStore>(
        &self,
        snapshot: &ComposedSnapshot,
        symbol_index_build_id: Option<SymbolIndexBuildId>,
        embedding_cache: &C,
    ) -> SemanticResult<SemanticBuildDraft> {
        let chunking = self.chunker.build(snapshot)?;
        let provider = provider_from_config(&self.config)?;
        let embedded = EmbeddingPipeline::new(provider.as_ref())
            .embed_chunk_documents(embedding_cache, &chunking.chunks)?;
        let semantic_config_digest = self.config_digest();
        let symbol_build = symbol_index_build_id
            .as_ref()
            .map(|build_id| build_id.0.as_str())
            .unwrap_or("no-symbol-build");
        let semantic_build_id = SemanticBuildId(format!(
            "semantic-build-{}",
            &stable_digest(&[
                &snapshot.repo_id,
                &snapshot.snapshot_id,
                &semantic_config_digest,
                symbol_build,
            ])[..16]
        ));
        let mut diagnostics = chunking.diagnostics;
        diagnostics.push(SemanticDiagnostic {
            severity: SemanticDiagnosticSeverity::Info,
            code: "semantic_embedding_provider_ready".to_string(),
            message: format!(
                "embedding provider {} materialized {} document embeddings with {} cache hits and {} misses",
                provider.identity().model_id,
                embedded.chunks.len(),
                embedded.stats.cache_hits,
                embedded.stats.cache_misses,
            ),
        });
        let indexed_file_count = embedded
            .chunks
            .iter()
            .map(|chunk| chunk.metadata.path.clone())
            .collect::<BTreeSet<_>>()
            .len();
        info!(
            repo_id = %snapshot.repo_id,
            snapshot_id = %snapshot.snapshot_id,
            semantic_build_id = %semantic_build_id.0,
            chunk_count = chunking.chunks.len(),
            "prepared phase6 semantic build draft"
        );
        Ok(SemanticBuildDraft {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            semantic_build_id,
            semantic_config_digest,
            embedding_provider: self.config.embedding_provider.clone(),
            chunk_text: self.config.chunk_text.clone(),
            chunk_schema_version: self.config.chunk_schema_version,
            symbol_index_build_id,
            created_at: unix_timestamp_string(),
            refresh_mode: "full_rebuild".to_string(),
            chunk_count: embedded.chunks.len(),
            indexed_file_count,
            embedding_count: embedded.chunks.len(),
            embedding_stats: embedded.stats,
            diagnostics,
            chunks: embedded.chunks,
            chunk_vectors: embedded.chunk_vectors,
        })
    }
}
