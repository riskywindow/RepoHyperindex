use std::collections::BTreeMap;

use hyperindex_protocol::semantic::{EmbeddingCacheKey, SemanticBuildId, SemanticChunkId};
use serde::{Deserialize, Serialize};

use crate::SemanticStoreError;

pub const FLAT_VECTOR_INDEX_KIND: &str = "flat_cosine_scan";
pub const FLAT_VECTOR_INDEX_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredVectorIndexMetadata {
    pub snapshot_id: String,
    pub semantic_build_id: SemanticBuildId,
    pub index_kind: String,
    pub index_schema_version: u32,
    pub vector_dimensions: u32,
    pub normalized: bool,
    pub indexed_vector_count: u64,
    pub created_at: String,
}

impl StoredVectorIndexMetadata {
    pub fn flat(
        snapshot_id: impl Into<String>,
        semantic_build_id: SemanticBuildId,
        vector_dimensions: u32,
        normalized: bool,
        indexed_vector_count: usize,
        created_at: impl Into<String>,
    ) -> Self {
        Self {
            snapshot_id: snapshot_id.into(),
            semantic_build_id,
            index_kind: FLAT_VECTOR_INDEX_KIND.to_string(),
            index_schema_version: FLAT_VECTOR_INDEX_SCHEMA_VERSION,
            vector_dimensions,
            normalized,
            indexed_vector_count: indexed_vector_count as u64,
            created_at: created_at.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredChunkVector {
    pub chunk_id: SemanticChunkId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<EmbeddingCacheKey>,
    pub vector: Vec<f32>,
}

impl StoredChunkVector {
    pub fn dimensions(&self) -> u32 {
        self.vector.len() as u32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorSearchResult {
    pub chunk_id: String,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlatVectorIndex {
    metadata: StoredVectorIndexMetadata,
    vectors_by_chunk_id: BTreeMap<String, Vec<f32>>,
    vector_norms_by_chunk_id: BTreeMap<String, f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparedQueryScorer<'a> {
    index: &'a FlatVectorIndex,
    query_embedding: &'a [f32],
    query_norm: f32,
}

impl FlatVectorIndex {
    pub fn from_persisted(
        metadata: StoredVectorIndexMetadata,
        chunk_vectors: Vec<StoredChunkVector>,
    ) -> Result<Self, SemanticStoreError> {
        if metadata.index_kind != FLAT_VECTOR_INDEX_KIND {
            return Err(SemanticStoreError::Compatibility(format!(
                "vector index kind mismatch for snapshot {}: expected {}, found {}",
                metadata.snapshot_id, FLAT_VECTOR_INDEX_KIND, metadata.index_kind
            )));
        }
        if metadata.index_schema_version != FLAT_VECTOR_INDEX_SCHEMA_VERSION {
            return Err(SemanticStoreError::Compatibility(format!(
                "vector index schema version mismatch for snapshot {}: expected {}, found {}",
                metadata.snapshot_id,
                FLAT_VECTOR_INDEX_SCHEMA_VERSION,
                metadata.index_schema_version
            )));
        }

        let mut vectors_by_chunk_id = BTreeMap::new();
        for chunk_vector in chunk_vectors {
            if chunk_vector.dimensions() != metadata.vector_dimensions {
                return Err(SemanticStoreError::Compatibility(format!(
                    "vector dimensions mismatch for chunk {}: expected {}, found {}",
                    chunk_vector.chunk_id.0,
                    metadata.vector_dimensions,
                    chunk_vector.dimensions()
                )));
            }
            if vectors_by_chunk_id
                .insert(chunk_vector.chunk_id.0.clone(), chunk_vector.vector)
                .is_some()
            {
                return Err(SemanticStoreError::Compatibility(format!(
                    "duplicate vector row found for chunk {}",
                    chunk_vector.chunk_id.0
                )));
            }
        }

        if vectors_by_chunk_id.len() as u64 != metadata.indexed_vector_count {
            return Err(SemanticStoreError::Compatibility(format!(
                "vector index row count mismatch for snapshot {}: expected {}, found {}",
                metadata.snapshot_id,
                metadata.indexed_vector_count,
                vectors_by_chunk_id.len()
            )));
        }

        let vector_norms_by_chunk_id = if metadata.normalized {
            BTreeMap::new()
        } else {
            vectors_by_chunk_id
                .iter()
                .map(|(chunk_id, vector)| (chunk_id.clone(), vector_norm(vector)))
                .collect()
        };

        Ok(Self {
            metadata,
            vectors_by_chunk_id,
            vector_norms_by_chunk_id,
        })
    }

    pub fn metadata(&self) -> &StoredVectorIndexMetadata {
        &self.metadata
    }

    pub fn chunk_count(&self) -> usize {
        self.vectors_by_chunk_id.len()
    }

    pub fn prepare_query<'a>(
        &'a self,
        query_embedding: &'a [f32],
    ) -> Result<PreparedQueryScorer<'a>, SemanticStoreError> {
        if query_embedding.len() as u32 != self.metadata.vector_dimensions {
            return Err(SemanticStoreError::Compatibility(format!(
                "query vector dimensions mismatch: expected {}, found {}",
                self.metadata.vector_dimensions,
                query_embedding.len()
            )));
        }
        Ok(PreparedQueryScorer {
            index: self,
            query_embedding,
            query_norm: if self.metadata.normalized {
                1.0
            } else {
                vector_norm(query_embedding)
            },
        })
    }

    pub fn score_chunk_id(
        &self,
        query_embedding: &[f32],
        chunk_id: &str,
    ) -> Result<u32, SemanticStoreError> {
        self.prepare_query(query_embedding)?
            .score_chunk_id(chunk_id)
    }
}

impl<'a> PreparedQueryScorer<'a> {
    pub fn score_chunk_id(&self, chunk_id: &str) -> Result<u32, SemanticStoreError> {
        let candidate = self
            .index
            .vectors_by_chunk_id
            .get(chunk_id)
            .ok_or_else(|| {
                SemanticStoreError::Compatibility(format!(
                    "vector index did not contain chunk {chunk_id}"
                ))
            })?;
        if candidate.len() as u32 != self.index.metadata.vector_dimensions {
            return Err(SemanticStoreError::Compatibility(format!(
                "candidate vector dimensions mismatch: expected {}, found {}",
                self.index.metadata.vector_dimensions,
                candidate.len()
            )));
        }
        let dot = self
            .query_embedding
            .iter()
            .zip(candidate.iter())
            .map(|(left, right)| left * right)
            .sum::<f32>();
        let denominator = if self.index.metadata.normalized {
            1.0
        } else {
            self.query_norm
                * self
                    .index
                    .vector_norms_by_chunk_id
                    .get(chunk_id)
                    .copied()
                    .unwrap_or_else(|| vector_norm(candidate))
        };
        if denominator <= f32::EPSILON {
            return Ok(0);
        }
        let cosine = (dot / denominator).clamp(-1.0, 1.0);
        Ok(((cosine + 1.0) * 500_000.0).round() as u32)
    }
}

fn vector_norm(values: &[f32]) -> f32 {
    values.iter().map(|value| value * value).sum::<f32>().sqrt()
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::semantic::SemanticChunkId;

    use super::{
        FLAT_VECTOR_INDEX_SCHEMA_VERSION, FlatVectorIndex, StoredChunkVector,
        StoredVectorIndexMetadata,
    };

    #[test]
    fn flat_index_scores_vectors() {
        let index = FlatVectorIndex::from_persisted(
            StoredVectorIndexMetadata {
                snapshot_id: "snap-123".to_string(),
                semantic_build_id: hyperindex_protocol::semantic::SemanticBuildId(
                    "semantic-build-123".to_string(),
                ),
                index_kind: "flat_cosine_scan".to_string(),
                index_schema_version: FLAT_VECTOR_INDEX_SCHEMA_VERSION,
                vector_dimensions: 3,
                normalized: true,
                indexed_vector_count: 2,
                created_at: "123".to_string(),
            },
            vec![
                StoredChunkVector {
                    chunk_id: SemanticChunkId("chunk-a".to_string()),
                    cache_key: None,
                    vector: vec![1.0, 0.0, 0.0],
                },
                StoredChunkVector {
                    chunk_id: SemanticChunkId("chunk-b".to_string()),
                    cache_key: None,
                    vector: vec![0.0, 1.0, 0.0],
                },
            ],
        )
        .unwrap();

        assert!(
            index.score_chunk_id(&[1.0, 0.0, 0.0], "chunk-a").unwrap()
                > index.score_chunk_id(&[1.0, 0.0, 0.0], "chunk-b").unwrap()
        );
    }

    #[test]
    fn flat_index_rejects_schema_mismatches() {
        let error = FlatVectorIndex::from_persisted(
            StoredVectorIndexMetadata {
                snapshot_id: "snap-123".to_string(),
                semantic_build_id: hyperindex_protocol::semantic::SemanticBuildId(
                    "semantic-build-123".to_string(),
                ),
                index_kind: "flat_cosine_scan".to_string(),
                index_schema_version: 99,
                vector_dimensions: 2,
                normalized: true,
                indexed_vector_count: 1,
                created_at: "123".to_string(),
            },
            vec![StoredChunkVector {
                chunk_id: SemanticChunkId("chunk-a".to_string()),
                cache_key: None,
                vector: vec![1.0, 0.0],
            }],
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("vector index schema version mismatch")
        );
    }
}
