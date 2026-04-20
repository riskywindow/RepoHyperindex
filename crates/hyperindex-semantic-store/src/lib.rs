pub mod embedding_cache;
pub mod migrations;
pub mod semantic_store;
pub mod vector_index;

use std::path::{Path, PathBuf};

use thiserror::Error;

pub use embedding_cache::{EmbeddingCacheStore, StoredEmbeddingRecord, build_embedding_cache_key};
pub use hyperindex_protocol::semantic::EmbeddingCacheKey;
pub use migrations::{SEMANTIC_STORE_SCHEMA_VERSION, SemanticStoreMigration, planned_migrations};
pub use semantic_store::{SemanticStore, SemanticStoreStatus, StoredSemanticBuild};
pub use vector_index::{
    FLAT_VECTOR_INDEX_KIND, FLAT_VECTOR_INDEX_SCHEMA_VERSION, FlatVectorIndex, StoredChunkVector,
    StoredVectorIndexMetadata, VectorSearchResult,
};

#[derive(Debug, Error)]
pub enum SemanticStoreError {
    #[error("semantic store io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("semantic store sqlite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("semantic store json failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("semantic store compatibility failed: {0}")]
    Compatibility(String),
}

pub type SemanticStoreResult<T> = Result<T, SemanticStoreError>;

pub fn default_store_path(runtime_root: &Path, repo_id: &str) -> PathBuf {
    runtime_root
        .join("data")
        .join("semantic")
        .join(repo_id)
        .join("semantic.sqlite3")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{SEMANTIC_STORE_SCHEMA_VERSION, default_store_path, planned_migrations};

    #[test]
    fn default_store_path_matches_phase6_layout() {
        let path = default_store_path(Path::new(".hyperindex"), "repo-123");
        assert_eq!(
            path,
            Path::new(".hyperindex/data/semantic/repo-123/semantic.sqlite3")
        );
    }

    #[test]
    fn planned_migrations_include_current_schema_version() {
        let migrations = planned_migrations();
        assert_eq!(
            migrations.last().unwrap().schema_version,
            SEMANTIC_STORE_SCHEMA_VERSION
        );
    }
}
