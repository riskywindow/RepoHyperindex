use hyperindex_protocol::semantic::{
    EmbeddingCacheKey, SemanticChunkRecord, SemanticEmbeddingCacheMetadata,
    SemanticEmbeddingInputKind,
};
use hyperindex_semantic_store::{
    EmbeddingCacheStore, StoredChunkVector, StoredEmbeddingRecord, build_embedding_cache_key,
};

use crate::common::{sha256_hex, unix_timestamp_string};
use crate::embedding_provider::EmbeddingProvider;
use crate::{SemanticError, SemanticResult};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmbeddingPipelineStats {
    pub requested: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub provider_batches: usize,
    pub cache_writes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedChunkBatch {
    pub chunks: Vec<SemanticChunkRecord>,
    pub chunk_vectors: Vec<StoredChunkVector>,
    pub stats: EmbeddingPipelineStats,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryEmbeddingBatch {
    pub vectors: Vec<Vec<f32>>,
    pub cache_metadata: Vec<SemanticEmbeddingCacheMetadata>,
    pub stats: EmbeddingPipelineStats,
}

pub struct EmbeddingPipeline<'a> {
    provider: &'a dyn EmbeddingProvider,
}

impl<'a> EmbeddingPipeline<'a> {
    pub fn new(provider: &'a dyn EmbeddingProvider) -> Self {
        Self { provider }
    }

    pub fn embed_chunk_documents<C: EmbeddingCacheStore>(
        &self,
        cache: &C,
        chunks: &[SemanticChunkRecord],
    ) -> SemanticResult<EmbeddedChunkBatch> {
        let prepared = chunks
            .iter()
            .map(|chunk| PreparedInput {
                text: chunk.serialized_text.clone(),
                text_digest: chunk.metadata.text.text_digest.clone(),
                cache_key: self.cache_key(
                    SemanticEmbeddingInputKind::Document,
                    &chunk.metadata.text.text_digest,
                ),
            })
            .collect::<Vec<_>>();
        let resolved =
            self.resolve_embeddings(cache, SemanticEmbeddingInputKind::Document, &prepared)?;

        let chunks = chunks
            .iter()
            .zip(resolved.cache_metadata.iter())
            .map(|(chunk, cache_metadata)| {
                let mut updated = chunk.clone();
                updated.embedding_cache = Some(cache_metadata.clone());
                updated
            })
            .collect::<Vec<_>>();
        let chunk_vectors = chunks
            .iter()
            .zip(resolved.cache_metadata.iter())
            .zip(resolved.vectors.iter())
            .map(|((chunk, cache_metadata), vector)| StoredChunkVector {
                chunk_id: chunk.metadata.chunk_id.clone(),
                cache_key: Some(cache_metadata.cache_key.clone()),
                vector: vector.clone(),
            })
            .collect();

        Ok(EmbeddedChunkBatch {
            chunks,
            chunk_vectors,
            stats: resolved.stats,
        })
    }

    pub fn embed_queries<C: EmbeddingCacheStore>(
        &self,
        cache: &C,
        queries: &[String],
    ) -> SemanticResult<QueryEmbeddingBatch> {
        let prepared = queries
            .iter()
            .map(|query| {
                let text_digest = sha256_hex(query.as_bytes());
                PreparedInput {
                    text: query.clone(),
                    cache_key: self.cache_key(SemanticEmbeddingInputKind::Query, &text_digest),
                    text_digest,
                }
            })
            .collect::<Vec<_>>();
        let resolved =
            self.resolve_embeddings(cache, SemanticEmbeddingInputKind::Query, &prepared)?;
        Ok(QueryEmbeddingBatch {
            vectors: resolved.vectors,
            cache_metadata: resolved.cache_metadata,
            stats: resolved.stats,
        })
    }

    fn resolve_embeddings<C: EmbeddingCacheStore>(
        &self,
        cache: &C,
        input_kind: SemanticEmbeddingInputKind,
        inputs: &[PreparedInput],
    ) -> SemanticResult<ResolvedEmbeddingBatch> {
        let mut stats = EmbeddingPipelineStats {
            requested: inputs.len(),
            ..EmbeddingPipelineStats::default()
        };
        let mut vectors = vec![Vec::new(); inputs.len()];
        let mut cache_metadata = vec![None; inputs.len()];
        let mut misses = Vec::new();
        let cached = cache
            .load_embeddings(
                &inputs
                    .iter()
                    .map(|input| input.cache_key.clone())
                    .collect::<Vec<_>>(),
            )
            .map_err(SemanticError::from)?;

        for (index, input) in inputs.iter().enumerate() {
            if input.text.as_bytes().len() > self.provider.config().max_input_bytes as usize {
                return Err(SemanticError::EmbeddingProviderFailed(format!(
                    "{} input exceeded max_input_bytes ({} > {})",
                    match input_kind {
                        SemanticEmbeddingInputKind::Document => "document",
                        SemanticEmbeddingInputKind::Query => "query",
                    },
                    input.text.as_bytes().len(),
                    self.provider.config().max_input_bytes
                )));
            }
            if let Some(record) = cached.get(&input.cache_key) {
                stats.cache_hits += 1;
                vectors[index] = record.vector.clone();
                cache_metadata[index] = Some(cache_metadata_for_record(record));
            } else {
                stats.cache_misses += 1;
                misses.push((index, input.clone()));
            }
        }

        if !misses.is_empty() {
            let batch_size = self.provider.config().max_batch_size.max(1) as usize;
            let mut new_records = Vec::new();
            for batch in misses.chunks(batch_size) {
                let texts = batch
                    .iter()
                    .map(|(_, input)| input.text.clone())
                    .collect::<Vec<_>>();
                let produced = match input_kind {
                    SemanticEmbeddingInputKind::Document => {
                        self.provider.embed_documents(&texts)?
                    }
                    SemanticEmbeddingInputKind::Query => self.provider.embed_queries(&texts)?,
                };
                stats.provider_batches += 1;
                for ((index, input), vector) in batch.iter().zip(produced.into_iter()) {
                    let record = StoredEmbeddingRecord {
                        cache_key: input.cache_key.clone(),
                        input_kind: input_kind.clone(),
                        provider_kind: self.provider.identity().provider_kind.clone(),
                        provider_id: self.provider.identity().provider_id.clone(),
                        provider_version: self.provider.identity().provider_version.clone(),
                        model_id: self.provider.identity().model_id.clone(),
                        model_digest: self.provider.identity().model_digest.clone(),
                        provider_config_digest: self.provider.config_digest().to_string(),
                        text_digest: input.text_digest.clone(),
                        dimensions: vector.len(),
                        normalized: self.provider.config().normalized,
                        vector: vector.clone(),
                        stored_at: unix_timestamp_string(),
                    };
                    vectors[*index] = vector;
                    cache_metadata[*index] = Some(cache_metadata_for_record(&record));
                    new_records.push(record);
                }
            }
            cache
                .persist_embeddings(&new_records)
                .map_err(SemanticError::from)?;
            stats.cache_writes = new_records.len();
        }

        Ok(ResolvedEmbeddingBatch {
            vectors,
            cache_metadata: cache_metadata
                .into_iter()
                .map(|entry| {
                    entry.ok_or_else(|| {
                        SemanticError::EmbeddingOutputInvalid(
                            "embedding pipeline did not resolve all cache metadata".to_string(),
                        )
                    })
                })
                .collect::<SemanticResult<Vec<_>>>()?,
            stats,
        })
    }

    fn cache_key(
        &self,
        input_kind: SemanticEmbeddingInputKind,
        text_digest: &str,
    ) -> EmbeddingCacheKey {
        build_embedding_cache_key(
            &input_kind,
            text_digest,
            &self.provider.identity().provider_kind,
            &self.provider.identity().provider_id,
            &self.provider.identity().provider_version,
            &self.provider.identity().model_id,
            &self.provider.identity().model_digest,
            self.provider.config_digest(),
        )
    }
}

#[derive(Debug, Clone)]
struct PreparedInput {
    text: String,
    text_digest: String,
    cache_key: EmbeddingCacheKey,
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedEmbeddingBatch {
    vectors: Vec<Vec<f32>>,
    cache_metadata: Vec<SemanticEmbeddingCacheMetadata>,
    stats: EmbeddingPipelineStats,
}

fn cache_metadata_for_record(record: &StoredEmbeddingRecord) -> SemanticEmbeddingCacheMetadata {
    SemanticEmbeddingCacheMetadata {
        cache_key: record.cache_key.clone(),
        input_kind: record.input_kind.clone(),
        model_digest: record.model_digest.clone(),
        text_digest: record.text_digest.clone(),
        provider_config_digest: record.provider_config_digest.clone(),
        vector_dimensions: record.dimensions as u32,
        normalized: record.normalized,
        stored_at: Some(record.stored_at.clone()),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use hyperindex_protocol::config::SemanticConfig;
    use hyperindex_protocol::semantic::{
        SemanticChunkId, SemanticChunkKind, SemanticChunkMetadata, SemanticChunkRecord,
        SemanticChunkSourceKind, SemanticChunkTextMetadata,
    };
    use hyperindex_semantic_store::SemanticStore;

    use super::EmbeddingPipeline;
    use crate::embedding_provider::DeterministicFixtureEmbeddingProvider;

    #[test]
    fn document_pipeline_reports_cache_hits_and_misses() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let provider = DeterministicFixtureEmbeddingProvider::new(
            SemanticConfig::default().embedding_provider,
        );
        let pipeline = EmbeddingPipeline::new(&provider);
        let chunk = sample_chunk("chunk-1", "text-digest-1", "invalidate sessions");

        let first = pipeline
            .embed_chunk_documents(&store, std::slice::from_ref(&chunk))
            .unwrap();
        let second = pipeline
            .embed_chunk_documents(&store, std::slice::from_ref(&chunk))
            .unwrap();

        assert_eq!(first.stats.cache_hits, 0);
        assert_eq!(first.stats.cache_misses, 1);
        assert_eq!(first.stats.cache_writes, 1);
        assert_eq!(second.stats.cache_hits, 1);
        assert_eq!(second.stats.cache_misses, 0);
        assert_eq!(second.stats.cache_writes, 0);
        assert_eq!(
            first.chunks[0].embedding_cache.as_ref().unwrap().cache_key,
            second.chunks[0].embedding_cache.as_ref().unwrap().cache_key
        );
    }

    #[test]
    fn cache_invalidates_when_chunk_content_changes() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let provider = DeterministicFixtureEmbeddingProvider::new(
            SemanticConfig::default().embedding_provider,
        );
        let pipeline = EmbeddingPipeline::new(&provider);
        let original = sample_chunk("chunk-1", "text-digest-1", "invalidate sessions");
        let changed = sample_chunk("chunk-1", "text-digest-2", "invalidate session now");

        let first = pipeline
            .embed_chunk_documents(&store, std::slice::from_ref(&original))
            .unwrap();
        let second = pipeline
            .embed_chunk_documents(&store, std::slice::from_ref(&changed))
            .unwrap();

        assert_eq!(first.stats.cache_misses, 1);
        assert_eq!(second.stats.cache_misses, 1);
        assert_ne!(
            first.chunks[0].embedding_cache.as_ref().unwrap().cache_key,
            second.chunks[0].embedding_cache.as_ref().unwrap().cache_key
        );
    }

    #[test]
    fn cache_invalidates_when_provider_changes() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let first_provider = DeterministicFixtureEmbeddingProvider::new(
            SemanticConfig::default().embedding_provider,
        );
        let mut second_config = SemanticConfig::default().embedding_provider;
        second_config.model_digest = "phase6-deterministic-fixture-v2".to_string();
        let second_provider = DeterministicFixtureEmbeddingProvider::new(second_config);
        let chunk = sample_chunk("chunk-1", "text-digest-1", "invalidate sessions");

        let first = EmbeddingPipeline::new(&first_provider)
            .embed_chunk_documents(&store, std::slice::from_ref(&chunk))
            .unwrap();
        let second = EmbeddingPipeline::new(&second_provider)
            .embed_chunk_documents(&store, std::slice::from_ref(&chunk))
            .unwrap();

        assert_eq!(first.stats.cache_misses, 1);
        assert_eq!(second.stats.cache_misses, 1);
        assert_ne!(
            first.chunks[0].embedding_cache.as_ref().unwrap().cache_key,
            second.chunks[0].embedding_cache.as_ref().unwrap().cache_key
        );
    }

    #[test]
    fn query_and_document_paths_use_distinct_cache_entries() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let provider = DeterministicFixtureEmbeddingProvider::new(
            SemanticConfig::default().embedding_provider,
        );
        let pipeline = EmbeddingPipeline::new(&provider);
        let chunk = sample_chunk("chunk-1", "text-digest-1", "invalidate sessions");

        let document = pipeline
            .embed_chunk_documents(&store, std::slice::from_ref(&chunk))
            .unwrap();
        let query = pipeline
            .embed_queries(&store, &["invalidate sessions".to_string()])
            .unwrap();
        let query_again = pipeline
            .embed_queries(&store, &["invalidate sessions".to_string()])
            .unwrap();

        assert_eq!(query.stats.cache_misses, 1);
        assert_eq!(query_again.stats.cache_hits, 1);
        assert_ne!(
            document.chunks[0]
                .embedding_cache
                .as_ref()
                .unwrap()
                .cache_key,
            query.cache_metadata[0].cache_key
        );
    }

    fn sample_chunk(chunk_id: &str, text_digest: &str, text: &str) -> SemanticChunkRecord {
        SemanticChunkRecord {
            metadata: SemanticChunkMetadata {
                chunk_id: SemanticChunkId(chunk_id.to_string()),
                chunk_kind: SemanticChunkKind::SymbolBody,
                source_kind: SemanticChunkSourceKind::Symbol,
                path: "src/session.ts".to_string(),
                language: None,
                extension: Some("ts".to_string()),
                package_name: None,
                package_root: None,
                workspace_root: None,
                symbol_id: None,
                symbol_display_name: None,
                symbol_kind: None,
                symbol_is_exported: None,
                symbol_is_default_export: None,
                span: None,
                content_sha256: "sha-content".to_string(),
                text: SemanticChunkTextMetadata {
                    serializer_id: "phase6-structured-text".to_string(),
                    format_version: 1,
                    text_digest: text_digest.to_string(),
                    text_bytes: text.len() as u32,
                    token_count_estimate: 3,
                },
            },
            serialized_text: text.to_string(),
            embedding_cache: None,
        }
    }
}
