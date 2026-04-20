use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use hyperindex_protocol::semantic::{
    EmbeddingCacheKey, SemanticEmbeddingInputKind, SemanticEmbeddingProviderKind,
};

use crate::SemanticStoreResult;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredEmbeddingRecord {
    pub cache_key: EmbeddingCacheKey,
    pub input_kind: SemanticEmbeddingInputKind,
    pub provider_kind: SemanticEmbeddingProviderKind,
    pub provider_id: String,
    pub provider_version: String,
    pub model_id: String,
    pub model_digest: String,
    pub provider_config_digest: String,
    pub text_digest: String,
    pub dimensions: usize,
    pub normalized: bool,
    pub vector: Vec<f32>,
    pub stored_at: String,
}

pub trait EmbeddingCacheStore {
    fn load_embedding(
        &self,
        cache_key: &EmbeddingCacheKey,
    ) -> SemanticStoreResult<Option<StoredEmbeddingRecord>>;

    fn load_embeddings(
        &self,
        cache_keys: &[EmbeddingCacheKey],
    ) -> SemanticStoreResult<BTreeMap<EmbeddingCacheKey, StoredEmbeddingRecord>> {
        let mut records = BTreeMap::new();
        for cache_key in cache_keys {
            if records.contains_key(cache_key) {
                continue;
            }
            if let Some(record) = self.load_embedding(cache_key)? {
                records.insert(record.cache_key.clone(), record);
            }
        }
        Ok(records)
    }

    fn persist_embeddings(&self, records: &[StoredEmbeddingRecord]) -> SemanticStoreResult<()>;

    fn embedding_entry_count(&self) -> SemanticStoreResult<usize>;
}

pub fn build_embedding_cache_key(
    input_kind: &SemanticEmbeddingInputKind,
    text_digest: &str,
    provider_kind: &SemanticEmbeddingProviderKind,
    provider_id: &str,
    provider_version: &str,
    model_id: &str,
    model_digest: &str,
    provider_config_digest: &str,
) -> EmbeddingCacheKey {
    let mut hasher = Sha256::new();
    for part in [
        embedding_input_kind_name(input_kind),
        embedding_provider_kind_name(provider_kind),
        provider_id,
        provider_version,
        model_id,
        model_digest,
        provider_config_digest,
        text_digest,
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    EmbeddingCacheKey(hex::encode(hasher.finalize()))
}

fn embedding_input_kind_name(kind: &SemanticEmbeddingInputKind) -> &'static str {
    match kind {
        SemanticEmbeddingInputKind::Document => "document",
        SemanticEmbeddingInputKind::Query => "query",
    }
}

fn embedding_provider_kind_name(kind: &SemanticEmbeddingProviderKind) -> &'static str {
    match kind {
        SemanticEmbeddingProviderKind::DeterministicFixture => "deterministic_fixture",
        SemanticEmbeddingProviderKind::LocalOnnx => "local_onnx",
        SemanticEmbeddingProviderKind::ExternalProcess => "external_process",
        SemanticEmbeddingProviderKind::Placeholder => "placeholder",
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::semantic::{
        SemanticEmbeddingInputKind, SemanticEmbeddingProviderKind,
    };

    use super::build_embedding_cache_key;

    #[test]
    fn cache_key_is_stable_for_same_inputs() {
        let left = build_embedding_cache_key(
            &SemanticEmbeddingInputKind::Document,
            "text-1",
            &SemanticEmbeddingProviderKind::DeterministicFixture,
            "fixture",
            "v1",
            "fixture-model",
            "model-v1",
            "config-v1",
        );
        let right = build_embedding_cache_key(
            &SemanticEmbeddingInputKind::Document,
            "text-1",
            &SemanticEmbeddingProviderKind::DeterministicFixture,
            "fixture",
            "v1",
            "fixture-model",
            "model-v1",
            "config-v1",
        );
        assert_eq!(left, right);
    }

    #[test]
    fn cache_key_changes_when_query_and_document_paths_differ() {
        let document = build_embedding_cache_key(
            &SemanticEmbeddingInputKind::Document,
            "text-1",
            &SemanticEmbeddingProviderKind::DeterministicFixture,
            "fixture",
            "v1",
            "fixture-model",
            "model-v1",
            "config-v1",
        );
        let query = build_embedding_cache_key(
            &SemanticEmbeddingInputKind::Query,
            "text-1",
            &SemanticEmbeddingProviderKind::DeterministicFixture,
            "fixture",
            "v1",
            "fixture-model",
            "model-v1",
            "config-v1",
        );
        assert_ne!(document, query);
    }

    #[test]
    fn cache_key_changes_when_provider_identity_changes() {
        let original = build_embedding_cache_key(
            &SemanticEmbeddingInputKind::Document,
            "text-1",
            &SemanticEmbeddingProviderKind::DeterministicFixture,
            "fixture",
            "v1",
            "fixture-model",
            "model-v1",
            "config-v1",
        );
        let changed = build_embedding_cache_key(
            &SemanticEmbeddingInputKind::Document,
            "text-1",
            &SemanticEmbeddingProviderKind::DeterministicFixture,
            "fixture",
            "v2",
            "fixture-model",
            "model-v1",
            "config-v1",
        );
        assert_ne!(original, changed);
    }
}
