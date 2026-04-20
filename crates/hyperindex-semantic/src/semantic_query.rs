use std::time::Instant;

use hyperindex_protocol::semantic::{
    SemanticChunkKind, SemanticChunkMetadata, SemanticChunkRecord, SemanticDiagnostic,
    SemanticDiagnosticSeverity, SemanticIndexManifest, SemanticQueryFilters, SemanticQueryParams,
    SemanticQueryResponse, SemanticQueryStats,
};
use hyperindex_semantic_store::{EmbeddingCacheStore, FlatVectorIndex};
use tracing::info;

use crate::{
    EmbeddingPipeline, EmbeddingProvider, SemanticReranker, SemanticResult, common::sha256_hex,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSearchScaffold {
    pub manifest: SemanticIndexManifest,
    pub chunks: Vec<SemanticChunkRecord>,
    pub index: FlatVectorIndex,
    pub diagnostics: Vec<SemanticDiagnostic>,
}

#[derive(Debug, Default, Clone)]
pub struct SemanticSearchEngine;

impl SemanticSearchEngine {
    pub fn search<C: EmbeddingCacheStore>(
        &self,
        scaffold: &SemanticSearchScaffold,
        params: &SemanticQueryParams,
        embedding_cache: &C,
        provider: &dyn EmbeddingProvider,
    ) -> SemanticResult<SemanticQueryResponse> {
        let query_embedding = EmbeddingPipeline::new(provider)
            .embed_queries(embedding_cache, &[params.query.text.clone()])?;
        self.search_loaded(
            scaffold,
            params,
            query_embedding
                .vectors
                .first()
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        )
    }

    pub fn search_loaded(
        &self,
        scaffold: &SemanticSearchScaffold,
        params: &SemanticQueryParams,
        query_embedding: &[f32],
    ) -> SemanticResult<SemanticQueryResponse> {
        let started_at = Instant::now();
        info!(
            repo_id = %params.repo_id,
            snapshot_id = %params.snapshot_id,
            query = %params.query.text,
            candidate_chunk_count = scaffold.chunks.len(),
            "executed phase6 flat semantic search"
        );

        let mut diagnostics = scaffold.diagnostics.clone();
        let reranker = SemanticReranker::default();
        let scorer = scaffold.index.prepare_query(query_embedding)?;

        let mut filtered_chunk_count = 0usize;
        let mut hits = scaffold
            .chunks
            .iter()
            .filter(|chunk| matches_filters(&chunk.metadata, &params.filters))
            .map(|chunk| {
                filtered_chunk_count += 1;
                let semantic_score = scorer.score_chunk_id(&chunk.metadata.chunk_id.0)?;
                let mut hit = reranker.build_hit(
                    &params.query,
                    chunk,
                    semantic_score,
                    params.rerank_mode.clone(),
                );
                hit.snippet = snippet_for(chunk);
                Ok(hit)
            })
            .collect::<SemanticResult<Vec<_>>>()?;

        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.semantic_score.cmp(&left.semantic_score))
                .then_with(|| left.chunk.path.cmp(&right.chunk.path))
                .then_with(|| {
                    chunk_kind_key(&left.chunk.chunk_kind)
                        .cmp(chunk_kind_key(&right.chunk.chunk_kind))
                })
                .then_with(|| left.chunk.chunk_id.0.cmp(&right.chunk.chunk_id.0))
        });
        hits.truncate(params.limit as usize);
        for (index, hit) in hits.iter_mut().enumerate() {
            hit.rank = (index + 1) as u32;
        }

        if filtered_chunk_count == 0 {
            diagnostics.push(SemanticDiagnostic {
                severity: SemanticDiagnosticSeverity::Info,
                code: "semantic_query_no_candidates".to_string(),
                message: "semantic query filters matched no chunk candidates".to_string(),
            });
        } else if hits.is_empty() {
            diagnostics.push(SemanticDiagnostic {
                severity: SemanticDiagnosticSeverity::Info,
                code: "semantic_query_no_hits".to_string(),
                message: "semantic query returned no hits for the filtered candidate set"
                    .to_string(),
            });
        } else if !matches!(
            params.rerank_mode,
            hyperindex_protocol::semantic::SemanticRerankMode::Off
        ) {
            diagnostics.push(SemanticDiagnostic {
                severity: SemanticDiagnosticSeverity::Info,
                code: "semantic_rerank_applied".to_string(),
                message:
                    "semantic hybrid reranking applied lexical, path/package, and symbol-backed priors"
                        .to_string(),
            });
        }

        Ok(SemanticQueryResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            query: params.query.clone(),
            manifest: Some(scaffold.manifest.clone()),
            stats: SemanticQueryStats {
                limit_requested: params.limit,
                candidate_chunk_count: scaffold.chunks.len() as u32,
                filtered_chunk_count: filtered_chunk_count as u32,
                hits_returned: hits.len() as u32,
                rerank_applied: !matches!(
                    params.rerank_mode,
                    hyperindex_protocol::semantic::SemanticRerankMode::Off
                ),
                elapsed_ms: started_at.elapsed().as_millis() as u64,
            },
            hits,
            diagnostics,
        })
    }
}

fn matches_filters(metadata: &SemanticChunkMetadata, filters: &SemanticQueryFilters) -> bool {
    matches_path_globs(&metadata.path, &filters.path_globs)
        && matches_optional_str(metadata.package_name.as_deref(), &filters.package_names)
        && matches_optional_str(metadata.package_root.as_deref(), &filters.package_roots)
        && matches_optional_str(metadata.workspace_root.as_deref(), &filters.workspace_roots)
        && matches_optional_language(metadata.language.as_ref(), &filters.languages)
        && matches_extension(metadata.extension.as_deref(), &filters.extensions)
        && matches_optional_symbol_kind(metadata.symbol_kind.as_ref(), &filters.symbol_kinds)
}

fn matches_path_globs(path: &str, path_globs: &[String]) -> bool {
    path_globs.is_empty()
        || path_globs
            .iter()
            .any(|pattern| wildcard_matches(pattern, path))
}

fn matches_optional_str(value: Option<&str>, filters: &[String]) -> bool {
    filters.is_empty() || value.is_some_and(|value| filters.iter().any(|filter| filter == value))
}

fn matches_optional_language(
    value: Option<&hyperindex_protocol::symbols::LanguageId>,
    filters: &[hyperindex_protocol::symbols::LanguageId],
) -> bool {
    filters.is_empty() || value.is_some_and(|value| filters.iter().any(|filter| filter == value))
}

fn matches_optional_symbol_kind(
    value: Option<&hyperindex_protocol::symbols::SymbolKind>,
    filters: &[hyperindex_protocol::symbols::SymbolKind],
) -> bool {
    filters.is_empty() || value.is_some_and(|value| filters.iter().any(|filter| filter == value))
}

fn matches_extension(value: Option<&str>, filters: &[String]) -> bool {
    filters.is_empty()
        || value.is_some_and(|value| {
            let normalized_value = value.trim_start_matches('.');
            filters
                .iter()
                .any(|filter| filter.trim_start_matches('.') == normalized_value)
        })
}

fn snippet_for(chunk: &SemanticChunkRecord) -> String {
    let mut lines = chunk
        .serialized_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    if lines.len() > 200 {
        lines.truncate(200);
        lines.push_str("...");
    }
    if lines.is_empty() {
        format!("chunk {}", sha256_hex(chunk.serialized_text.as_bytes()))
    } else {
        lines
    }
}

fn chunk_kind_key(kind: &SemanticChunkKind) -> &'static str {
    match kind {
        SemanticChunkKind::SymbolBody => "symbol_body",
        SemanticChunkKind::FileHeader => "file_header",
        SemanticChunkKind::RouteFile => "route_file",
        SemanticChunkKind::ConfigFile => "config_file",
        SemanticChunkKind::TestFile => "test_file",
        SemanticChunkKind::FallbackWindow => "fallback_window",
    }
}

fn wildcard_matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut pattern_index, mut text_index) = (0usize, 0usize);
    let mut star_index = None;
    let mut match_index = 0usize;

    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            match_index = text_index;
        } else if let Some(index) = star_index {
            pattern_index = index + 1;
            match_index += 1;
            text_index = match_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use hyperindex_protocol::semantic::{
        SemanticBuildId, SemanticChunkId, SemanticChunkKind, SemanticChunkMetadata,
        SemanticChunkRecord, SemanticChunkSourceKind, SemanticChunkTextMetadata,
        SemanticEmbeddingProviderConfig, SemanticEmbeddingProviderKind, SemanticQueryText,
    };
    use hyperindex_protocol::symbols::{LanguageId, SymbolKind};
    use hyperindex_semantic_store::{
        FlatVectorIndex, StoredChunkVector, StoredVectorIndexMetadata,
    };
    use serde::Deserialize;

    use super::{SemanticSearchEngine, SemanticSearchScaffold};

    #[test]
    fn filtered_search_respects_metadata_constraints() {
        let hits = SemanticSearchEngine::default()
            .search_loaded(
                &semantic_fixture_scaffold(),
                &hyperindex_protocol::semantic::SemanticQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: SemanticQueryText {
                        text: "invalidate sessions".to_string(),
                    },
                    filters: hyperindex_protocol::semantic::SemanticQueryFilters {
                        path_globs: vec!["src/**".to_string()],
                        package_names: vec!["@repo/auth".to_string()],
                        languages: vec![LanguageId::Typescript],
                        symbol_kinds: vec![SymbolKind::Function],
                        ..hyperindex_protocol::semantic::SemanticQueryFilters::default()
                    },
                    limit: 10,
                    rerank_mode: hyperindex_protocol::semantic::SemanticRerankMode::Off,
                },
                &[1.0, 0.0, 0.0],
            )
            .unwrap()
            .hits;

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk.path, "src/session.ts");
    }

    #[test]
    fn ties_break_by_path_kind_and_chunk_id() {
        let response = SemanticSearchEngine::default()
            .search_loaded(
                &semantic_fixture_scaffold(),
                &hyperindex_protocol::semantic::SemanticQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: SemanticQueryText {
                        text: "invalidate sessions".to_string(),
                    },
                    filters: hyperindex_protocol::semantic::SemanticQueryFilters::default(),
                    limit: 10,
                    rerank_mode: hyperindex_protocol::semantic::SemanticRerankMode::Hybrid,
                },
                &[0.0, 1.0, 0.0],
            )
            .unwrap();

        assert_eq!(response.hits[0].chunk.path, "src/session.ts");
        assert_eq!(response.hits[1].chunk.path, "tests/session.test.ts");
        assert!(response.stats.rerank_applied);
        assert_eq!(response.diagnostics[0].code, "semantic_rerank_applied");
    }

    fn semantic_fixture_scaffold() -> SemanticSearchScaffold {
        let chunks = vec![
            SemanticChunkRecord {
                metadata: SemanticChunkMetadata {
                    chunk_id: SemanticChunkId("chunk-a".to_string()),
                    chunk_kind: SemanticChunkKind::SymbolBody,
                    source_kind: SemanticChunkSourceKind::Symbol,
                    path: "src/session.ts".to_string(),
                    language: Some(LanguageId::Typescript),
                    extension: Some("ts".to_string()),
                    package_name: Some("@repo/auth".to_string()),
                    package_root: Some("packages/auth".to_string()),
                    workspace_root: Some(".".to_string()),
                    symbol_id: None,
                    symbol_display_name: Some("invalidateSession".to_string()),
                    symbol_kind: Some(SymbolKind::Function),
                    symbol_is_exported: Some(true),
                    symbol_is_default_export: Some(false),
                    span: None,
                    content_sha256: "sha-a".to_string(),
                    text: SemanticChunkTextMetadata {
                        serializer_id: "phase6-structured-text".to_string(),
                        format_version: 1,
                        text_digest: "text-a".to_string(),
                        text_bytes: 32,
                        token_count_estimate: 4,
                    },
                },
                serialized_text: "export function invalidateSession() {}".to_string(),
                embedding_cache: None,
            },
            SemanticChunkRecord {
                metadata: SemanticChunkMetadata {
                    chunk_id: SemanticChunkId("chunk-b".to_string()),
                    chunk_kind: SemanticChunkKind::TestFile,
                    source_kind: SemanticChunkSourceKind::File,
                    path: "tests/session.test.ts".to_string(),
                    language: Some(LanguageId::Typescript),
                    extension: Some("ts".to_string()),
                    package_name: Some("@repo/auth".to_string()),
                    package_root: Some("packages/auth".to_string()),
                    workspace_root: Some(".".to_string()),
                    symbol_id: None,
                    symbol_display_name: None,
                    symbol_kind: None,
                    symbol_is_exported: None,
                    symbol_is_default_export: None,
                    span: None,
                    content_sha256: "sha-b".to_string(),
                    text: SemanticChunkTextMetadata {
                        serializer_id: "phase6-structured-text".to_string(),
                        format_version: 1,
                        text_digest: "text-b".to_string(),
                        text_bytes: 32,
                        token_count_estimate: 4,
                    },
                },
                serialized_text: "describe('session', () => {})".to_string(),
                embedding_cache: None,
            },
        ];

        SemanticSearchScaffold {
            manifest: hyperindex_protocol::semantic::SemanticIndexManifest {
                build_id: SemanticBuildId("semantic-build-123".to_string()),
                repo_id: "repo-123".to_string(),
                snapshot_id: "snap-123".to_string(),
                semantic_config_digest: "config-v1".to_string(),
                chunk_schema_version: 1,
                symbol_index_build_id: None,
                embedding_provider: SemanticEmbeddingProviderConfig {
                    provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
                    model_id: "fixture".to_string(),
                    model_digest: "fixture-v1".to_string(),
                    vector_dimensions: 3,
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
                embedding_cache: hyperindex_protocol::semantic::SemanticEmbeddingCacheManifest {
                    key_algorithm: "sha256".to_string(),
                    entry_count: 2,
                    store_path: Some("/tmp/semantic.sqlite3".to_string()),
                },
                indexed_chunk_count: 2,
                indexed_file_count: 2,
                created_at: "123".to_string(),
            },
            index: FlatVectorIndex::from_persisted(
                StoredVectorIndexMetadata::flat(
                    "snap-123",
                    SemanticBuildId("semantic-build-123".to_string()),
                    3,
                    true,
                    2,
                    "123",
                ),
                vec![
                    StoredChunkVector {
                        chunk_id: SemanticChunkId("chunk-a".to_string()),
                        cache_key: None,
                        vector: vec![0.0, 1.0, 0.0],
                    },
                    StoredChunkVector {
                        chunk_id: SemanticChunkId("chunk-b".to_string()),
                        cache_key: None,
                        vector: vec![0.0, 1.0, 0.0],
                    },
                ],
            )
            .unwrap(),
            chunks,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn hybrid_reranking_changes_expected_ordering() {
        let response = SemanticSearchEngine::default()
            .search_loaded(
                &semantic_fixture_scaffold(),
                &hyperindex_protocol::semantic::SemanticQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: SemanticQueryText {
                        text: "Where is the session invalidation test?".to_string(),
                    },
                    filters: hyperindex_protocol::semantic::SemanticQueryFilters::default(),
                    limit: 10,
                    rerank_mode: hyperindex_protocol::semantic::SemanticRerankMode::Hybrid,
                },
                &[0.0, 1.0, 0.0],
            )
            .unwrap();

        assert_eq!(response.hits[0].chunk.path, "tests/session.test.ts");
        assert!(response.hits[0].score > response.hits[1].score);
        assert!(response.stats.rerank_applied);
        assert!(
            response.hits[0]
                .explanation
                .as_ref()
                .unwrap()
                .signals
                .iter()
                .any(|signal| signal.label == "test_prior")
        );
    }

    #[test]
    fn no_answer_behavior_is_explicit_and_stable() {
        let response = SemanticSearchEngine::default()
            .search_loaded(
                &semantic_fixture_scaffold(),
                &hyperindex_protocol::semantic::SemanticQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: SemanticQueryText {
                        text: "Where is the impossible admin dashboard?".to_string(),
                    },
                    filters: hyperindex_protocol::semantic::SemanticQueryFilters {
                        path_globs: vec!["config/**".to_string()],
                        ..hyperindex_protocol::semantic::SemanticQueryFilters::default()
                    },
                    limit: 10,
                    rerank_mode: hyperindex_protocol::semantic::SemanticRerankMode::Hybrid,
                },
                &[0.0, 0.0, 1.0],
            )
            .unwrap();

        assert!(response.hits.is_empty());
        assert_eq!(response.stats.filtered_chunk_count, 0);
        assert!(
            response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "semantic_query_no_candidates")
        );
    }

    #[test]
    fn checked_in_query_pack_paths_parse_and_align_with_fixture_filters() {
        #[derive(Debug, Deserialize)]
        struct QueryPack {
            queries: Vec<SemanticPackQuery>,
        }

        #[derive(Debug, Deserialize)]
        struct SemanticPackQuery {
            query_id: String,
            text: String,
            path_globs: Vec<String>,
        }

        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let pack_path = manifest_dir
            .join("../../bench/configs/query-packs/synthetic-saas-medium-semantic-pack.json");
        let raw = fs::read_to_string(pack_path).unwrap();
        let pack: QueryPack = serde_json::from_str(&raw).unwrap();

        let fixture_queries = pack
            .queries
            .into_iter()
            .filter(|query| {
                matches!(
                    query.query_id.as_str(),
                    "semantic-hero-session-invalidation"
                        | "semantic-route-logout"
                        | "semantic-session-invalidation-test"
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(fixture_queries.len(), 3);
        assert!(
            fixture_queries
                .iter()
                .any(|query| query.text == "where do we invalidate sessions?")
        );
        assert!(
            fixture_queries
                .iter()
                .all(|query| !query.path_globs.is_empty())
        );
    }
}
