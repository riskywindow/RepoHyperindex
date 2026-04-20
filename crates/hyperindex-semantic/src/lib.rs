pub mod chunker;
pub mod cli_integration;
pub mod common;
pub mod daemon_integration;
pub mod embedding_pipeline;
pub mod embedding_provider;
pub mod semantic_model;
pub mod semantic_query;
pub mod semantic_rerank;

use hyperindex_semantic_store::SemanticStoreError;
use hyperindex_symbols::SymbolError;
use thiserror::Error;

pub use chunker::{ChunkingPlan, ScaffoldChunker};
pub use embedding_pipeline::{
    EmbeddedChunkBatch, EmbeddingPipeline, EmbeddingPipelineStats, QueryEmbeddingBatch,
};
pub use embedding_provider::{
    DeterministicFixtureEmbeddingProvider, EmbeddingProvider, EmbeddingProviderIdentity,
    ExternalProcessEmbeddingProvider, provider_from_config,
};
pub use semantic_model::{SemanticBuildDraft, SemanticScaffoldBuilder};
pub use semantic_query::{SemanticSearchEngine, SemanticSearchScaffold};
pub use semantic_rerank::SemanticReranker;

#[derive(Debug, Error)]
pub enum SemanticError {
    #[error("semantic scaffold for {0} is not implemented yet")]
    NotImplemented(&'static str),
    #[error("semantic query is invalid: {0}")]
    InvalidQuery(String),
    #[error("embedding provider is misconfigured: {0}")]
    EmbeddingProviderMisconfigured(String),
    #[error("embedding provider failed: {0}")]
    EmbeddingProviderFailed(String),
    #[error("embedding provider returned invalid output: {0}")]
    EmbeddingOutputInvalid(String),
    #[error(transparent)]
    Symbol(#[from] SymbolError),
    #[error(transparent)]
    Store(#[from] SemanticStoreError),
}

pub type SemanticResult<T> = Result<T, SemanticError>;

#[cfg(test)]
mod tests {
    use hyperindex_protocol::semantic::{
        SemanticChunkId, SemanticChunkKind, SemanticChunkSourceKind, SemanticChunkTextMetadata,
        SemanticQueryFilters, SemanticQueryParams, SemanticQueryText, SemanticRerankMode,
    };
    use hyperindex_protocol::snapshot::ComposedSnapshot;
    use hyperindex_semantic_store::{
        FlatVectorIndex, StoredChunkVector, StoredVectorIndexMetadata,
    };

    use crate::chunker::ScaffoldChunker;
    use crate::common::stable_digest;
    use crate::embedding_provider::{DeterministicFixtureEmbeddingProvider, EmbeddingProvider};
    use crate::semantic_query::{SemanticSearchEngine, SemanticSearchScaffold};
    #[test]
    fn stable_digest_is_deterministic() {
        let left = stable_digest(&["repo", "snapshot", "phase6"]);
        let right = stable_digest(&["repo", "snapshot", "phase6"]);
        assert_eq!(left, right);
    }

    #[test]
    fn scaffold_chunker_emits_placeholder_diagnostic_without_chunks() {
        let snapshot = ComposedSnapshot {
            version: hyperindex_protocol::STORAGE_VERSION,
            protocol_version: hyperindex_protocol::PROTOCOL_VERSION.to_string(),
            repo_id: "repo-123".to_string(),
            repo_root: "/tmp/repo".to_string(),
            snapshot_id: "snap-123".to_string(),
            base: hyperindex_protocol::snapshot::BaseSnapshot {
                kind: hyperindex_protocol::snapshot::BaseSnapshotKind::GitCommit,
                commit: "deadbeef".to_string(),
                digest: "base".to_string(),
                file_count: 0,
                files: Vec::new(),
            },
            working_tree: hyperindex_protocol::snapshot::WorkingTreeOverlay {
                digest: "working-tree".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        };
        let plan = ScaffoldChunker::new(
            1,
            hyperindex_protocol::semantic::SemanticChunkTextConfig {
                serializer_id: "phase6-structured-text".to_string(),
                format_version: 1,
                includes_path_header: true,
                includes_symbol_context: true,
                normalized_newlines: true,
            },
        )
        .build(&snapshot)
        .unwrap();
        assert_eq!(plan.chunk_schema_version, 1);
        assert!(plan.chunks.is_empty());
        assert_eq!(plan.diagnostics[0].code, "semantic_chunks_empty");
    }

    #[test]
    fn deterministic_embedding_provider_returns_vectors() {
        let provider = DeterministicFixtureEmbeddingProvider::new(
            hyperindex_protocol::config::SemanticConfig::default().embedding_provider,
        );
        let embeddings = provider
            .embed_documents(&["invalidate sessions".to_string()])
            .unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].len(), 384);
    }

    #[test]
    fn search_engine_hybrid_results_include_explanation_fields() {
        let response = SemanticSearchEngine::default()
            .search_loaded(
                &SemanticSearchScaffold {
                    manifest: hyperindex_protocol::semantic::SemanticIndexManifest {
                        build_id: hyperindex_protocol::semantic::SemanticBuildId(
                            "semantic-build-123".to_string(),
                        ),
                        repo_id: "repo-123".to_string(),
                        snapshot_id: "snap-123".to_string(),
                        semantic_config_digest: "config-v1".to_string(),
                        chunk_schema_version: 1,
                        symbol_index_build_id: None,
                        embedding_provider:
                            hyperindex_protocol::semantic::SemanticEmbeddingProviderConfig {
                                provider_kind: hyperindex_protocol::semantic::SemanticEmbeddingProviderKind::DeterministicFixture,
                                model_id: "fixture".to_string(),
                                model_digest: "fixture-v1".to_string(),
                                vector_dimensions: 2,
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
                            schema_version: 4,
                            manifest_sha256: None,
                        },
                        embedding_cache:
                            hyperindex_protocol::semantic::SemanticEmbeddingCacheManifest {
                                key_algorithm: "sha256".to_string(),
                                entry_count: 1,
                                store_path: Some("/tmp/semantic.sqlite3".to_string()),
                            },
                        indexed_chunk_count: 1,
                        indexed_file_count: 1,
                        created_at: "123".to_string(),
                    },
                    chunks: vec![hyperindex_protocol::semantic::SemanticChunkRecord {
                        metadata: hyperindex_protocol::semantic::SemanticChunkMetadata {
                            chunk_id: SemanticChunkId("chunk-123".to_string()),
                            chunk_kind: SemanticChunkKind::SymbolBody,
                            source_kind: SemanticChunkSourceKind::Symbol,
                            path: "src/session.ts".to_string(),
                            language: None,
                            extension: Some("ts".to_string()),
                            package_name: None,
                            package_root: None,
                            workspace_root: Some(".".to_string()),
                            symbol_id: None,
                            symbol_display_name: Some("invalidateSession".to_string()),
                            symbol_kind: None,
                            symbol_is_exported: Some(true),
                            symbol_is_default_export: Some(false),
                            span: None,
                            content_sha256: "sha256:a".to_string(),
                            text: SemanticChunkTextMetadata {
                                serializer_id: "phase6-structured-text".to_string(),
                                format_version: 1,
                                text_digest: "sha256:text-a".to_string(),
                                text_bytes: 1,
                                token_count_estimate: 1,
                            },
                        },
                        serialized_text: "invalidateSession".to_string(),
                        embedding_cache: None,
                    }],
                    index: FlatVectorIndex::from_persisted(
                        StoredVectorIndexMetadata::flat(
                            "snap-123",
                            hyperindex_protocol::semantic::SemanticBuildId(
                                "semantic-build-123".to_string(),
                            ),
                            2,
                            true,
                            1,
                            "123",
                        ),
                        vec![StoredChunkVector {
                            chunk_id: SemanticChunkId("chunk-123".to_string()),
                            cache_key: None,
                            vector: vec![1.0, 0.0],
                        }],
                    )
                    .unwrap(),
                    diagnostics: Vec::new(),
                },
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
                &[1.0, 0.0],
            )
            .unwrap();

        assert_eq!(response.hits[0].chunk.path, "src/session.ts");
        assert!(response.stats.rerank_applied);
        assert!(response.hits[0].explanation.is_some());
    }

    #[test]
    fn search_engine_returns_real_hits() {
        let manifest = hyperindex_protocol::semantic::SemanticIndexManifest {
            build_id: hyperindex_protocol::semantic::SemanticBuildId(
                "semantic-build-123".to_string(),
            ),
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            semantic_config_digest: "config-v1".to_string(),
            chunk_schema_version: 1,
            symbol_index_build_id: None,
            embedding_provider: hyperindex_protocol::semantic::SemanticEmbeddingProviderConfig {
                provider_kind:
                    hyperindex_protocol::semantic::SemanticEmbeddingProviderKind::DeterministicFixture,
                model_id: "fixture".to_string(),
                model_digest: "fixture-v1".to_string(),
                vector_dimensions: 2,
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
                schema_version: 4,
                manifest_sha256: None,
            },
            embedding_cache:
                hyperindex_protocol::semantic::SemanticEmbeddingCacheManifest {
                    key_algorithm: "sha256".to_string(),
                    entry_count: 1,
                    store_path: Some("/tmp/semantic.sqlite3".to_string()),
                },
            indexed_chunk_count: 1,
            indexed_file_count: 1,
            created_at: "123".to_string(),
        };
        let response = SemanticSearchEngine::default().search_loaded(
            &SemanticSearchScaffold {
                manifest,
                chunks: vec![hyperindex_protocol::semantic::SemanticChunkRecord {
                    metadata: hyperindex_protocol::semantic::SemanticChunkMetadata {
                        chunk_id: SemanticChunkId("chunk-123".to_string()),
                        chunk_kind: SemanticChunkKind::SymbolBody,
                        source_kind: SemanticChunkSourceKind::Symbol,
                        path: "src/session.ts".to_string(),
                        language: None,
                        extension: Some("ts".to_string()),
                        package_name: None,
                        package_root: None,
                        workspace_root: Some(".".to_string()),
                        symbol_id: None,
                        symbol_display_name: Some("invalidateSession".to_string()),
                        symbol_kind: None,
                        symbol_is_exported: Some(true),
                        symbol_is_default_export: Some(false),
                        span: None,
                        content_sha256: "sha256:a".to_string(),
                        text: SemanticChunkTextMetadata {
                            serializer_id: "phase6-structured-text".to_string(),
                            format_version: 1,
                            text_digest: "sha256:text-a".to_string(),
                            text_bytes: 1,
                            token_count_estimate: 1,
                        },
                    },
                    serialized_text: "invalidateSession".to_string(),
                    embedding_cache: None,
                }],
                index: FlatVectorIndex::from_persisted(
                    StoredVectorIndexMetadata::flat(
                        "snap-123",
                        hyperindex_protocol::semantic::SemanticBuildId(
                            "semantic-build-123".to_string(),
                        ),
                        2,
                        true,
                        1,
                        "123",
                    ),
                    vec![StoredChunkVector {
                        chunk_id: SemanticChunkId("chunk-123".to_string()),
                        cache_key: None,
                        vector: vec![1.0, 0.0],
                    }],
                )
                .unwrap(),
                diagnostics: Vec::new(),
            },
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
            &[1.0, 0.0],
        );
        let response = response.unwrap();
        assert_eq!(response.hits.len(), 1);
        assert_eq!(response.hits[0].chunk.path, "src/session.ts");
        assert_eq!(response.diagnostics[0].code, "semantic_rerank_applied");
    }
}
