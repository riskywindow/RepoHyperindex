use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use hyperindex_protocol::semantic::{
    EmbeddingCacheKey, SemanticBuildId, SemanticChunkId, SemanticChunkRecord,
    SemanticChunkTextConfig, SemanticDiagnostic, SemanticEmbeddingCacheManifest,
    SemanticEmbeddingInputKind, SemanticEmbeddingProviderConfig, SemanticEmbeddingProviderKind,
    SemanticIndexManifest, SemanticIndexStorage, SemanticRefreshStats, SemanticStorageFormat,
};
use rusqlite::{Connection, OptionalExtension, params};
use tracing::info;

use crate::{
    EmbeddingCacheStore, FlatVectorIndex, SEMANTIC_STORE_SCHEMA_VERSION, SemanticStoreError,
    SemanticStoreResult, StoredChunkVector, StoredEmbeddingRecord, StoredVectorIndexMetadata,
    default_store_path, planned_migrations,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SemanticBuildProfile {
    pub chunk_materialization_ms: u64,
    pub embedding_resolution_ms: u64,
    pub vector_persist_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredSemanticBuild {
    pub repo_id: String,
    pub snapshot_id: String,
    pub semantic_build_id: SemanticBuildId,
    pub semantic_config_digest: String,
    pub schema_version: u32,
    pub chunk_schema_version: u32,
    pub embedding_provider: SemanticEmbeddingProviderConfig,
    pub chunk_text: SemanticChunkTextConfig,
    pub symbol_index_build_id: Option<hyperindex_protocol::symbols::SymbolIndexBuildId>,
    pub created_at: String,
    pub refresh_mode: String,
    pub chunk_count: usize,
    pub indexed_file_count: usize,
    pub embedding_count: usize,
    #[serde(default)]
    pub embedding_cache_hits: usize,
    #[serde(default)]
    pub embedding_cache_misses: usize,
    #[serde(default)]
    pub embedding_cache_writes: usize,
    #[serde(default)]
    pub embedding_provider_batches: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<SemanticBuildProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_stats: Option<SemanticRefreshStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub diagnostics: Vec<SemanticDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticStoreStatus {
    pub db_path: String,
    pub schema_version: u32,
    pub build_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticStore {
    pub store_path: PathBuf,
    pub schema_version: u32,
}

impl SemanticStore {
    pub fn scaffold(runtime_root: &Path, repo_id: &str) -> SemanticStoreResult<Self> {
        let store_path = default_store_path(runtime_root, repo_id);
        Self::open_at_path(store_path)
    }

    pub fn open_in_store_dir(store_dir: &Path, repo_id: &str) -> SemanticStoreResult<Self> {
        Self::open_at_path(store_dir.join(repo_id).join("semantic.sqlite3"))
    }

    pub fn open_at_path(store_path: PathBuf) -> SemanticStoreResult<Self> {
        if let Some(parent) = store_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let store = Self {
            store_path,
            schema_version: SEMANTIC_STORE_SCHEMA_VERSION,
        };
        store.migrate()?;
        info!(path = %store.store_path.display(), "opened phase6 semantic store scaffold");
        Ok(store)
    }

    pub fn manifest_for(&self, build: &StoredSemanticBuild) -> SemanticIndexManifest {
        SemanticIndexManifest {
            build_id: build.semantic_build_id.clone(),
            repo_id: build.repo_id.clone(),
            snapshot_id: build.snapshot_id.clone(),
            semantic_config_digest: build.semantic_config_digest.clone(),
            chunk_schema_version: build.chunk_schema_version,
            symbol_index_build_id: build.symbol_index_build_id.clone(),
            embedding_provider: build.embedding_provider.clone(),
            chunk_text: build.chunk_text.clone(),
            storage: SemanticIndexStorage {
                format: SemanticStorageFormat::Sqlite,
                path: self.store_path.display().to_string(),
                schema_version: self.schema_version,
                manifest_sha256: None,
            },
            embedding_cache: SemanticEmbeddingCacheManifest {
                key_algorithm:
                    "sha256(input_kind + text_digest + provider_identity + provider_config_digest)"
                        .to_string(),
                entry_count: build.embedding_count as u64,
                store_path: Some(self.store_path.display().to_string()),
            },
            indexed_chunk_count: build.chunk_count as u64,
            indexed_file_count: build.indexed_file_count as u64,
            created_at: build.created_at.clone(),
        }
    }

    pub fn persist_build(&self, build: &StoredSemanticBuild) -> SemanticStoreResult<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        persist_build_row(&transaction, build)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn persist_build_with_chunks(
        &self,
        build: &StoredSemanticBuild,
        chunks: &[SemanticChunkRecord],
    ) -> SemanticStoreResult<()> {
        self.persist_build_with_chunks_and_vectors(build, chunks, &[])
    }

    pub fn persist_build_with_chunks_and_vectors(
        &self,
        build: &StoredSemanticBuild,
        chunks: &[SemanticChunkRecord],
        chunk_vectors: &[StoredChunkVector],
    ) -> SemanticStoreResult<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        persist_build_row(&transaction, build)?;
        transaction.execute(
            "DELETE FROM semantic_chunks WHERE snapshot_id = ?1",
            [build.snapshot_id.as_str()],
        )?;
        transaction.execute(
            "DELETE FROM semantic_chunk_vectors WHERE snapshot_id = ?1",
            [build.snapshot_id.as_str()],
        )?;
        transaction.execute(
            "DELETE FROM semantic_vector_index_metadata WHERE snapshot_id = ?1",
            [build.snapshot_id.as_str()],
        )?;
        for chunk in chunks {
            transaction.execute(
                r#"
                INSERT INTO semantic_chunks (
                    snapshot_id,
                    semantic_build_id,
                    chunk_id,
                    path,
                    chunk_kind,
                    source_kind,
                    symbol_id,
                    text_digest,
                    content_sha256,
                    record_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    build.snapshot_id,
                    build.semantic_build_id.0,
                    chunk.metadata.chunk_id.0,
                    chunk.metadata.path,
                    serde_json::to_string(&chunk.metadata.chunk_kind)?,
                    serde_json::to_string(&chunk.metadata.source_kind)?,
                    chunk
                        .metadata
                        .symbol_id
                        .as_ref()
                        .map(|symbol_id| symbol_id.0.clone()),
                    chunk.metadata.text.text_digest,
                    chunk.metadata.content_sha256,
                    serde_json::to_string(chunk)?,
                ],
            )?;
        }
        persist_vector_index_rows(&transaction, build, chunk_vectors)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn load_build(
        &self,
        snapshot_id: &str,
    ) -> SemanticStoreResult<Option<StoredSemanticBuild>> {
        let connection = self.connect()?;
        connection
            .query_row(
                "SELECT build_json FROM semantic_builds WHERE snapshot_id = ?1",
                [snapshot_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(Into::into)
    }

    pub fn list_builds(&self) -> SemanticStoreResult<Vec<StoredSemanticBuild>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            r#"
            SELECT build_json
            FROM semantic_builds
            ORDER BY snapshot_id ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut builds = Vec::new();
        for row in rows {
            let raw = row?;
            builds.push(serde_json::from_str(&raw)?);
        }
        Ok(builds)
    }

    pub fn load_chunk(
        &self,
        snapshot_id: &str,
        chunk_id: &SemanticChunkId,
    ) -> SemanticStoreResult<Option<SemanticChunkRecord>> {
        let connection = self.connect()?;
        connection
            .query_row(
                r#"
                SELECT record_json
                FROM semantic_chunks
                WHERE snapshot_id = ?1 AND chunk_id = ?2
                "#,
                params![snapshot_id, chunk_id.0.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(Into::into)
    }

    pub fn list_chunks(&self, snapshot_id: &str) -> SemanticStoreResult<Vec<SemanticChunkRecord>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            r#"
            SELECT record_json
            FROM semantic_chunks
            WHERE snapshot_id = ?1
            ORDER BY path ASC, chunk_id ASC
            "#,
        )?;
        let rows = statement.query_map([snapshot_id], |row| row.get::<_, String>(0))?;
        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(serde_json::from_str(&row?)?);
        }
        Ok(chunks)
    }

    pub fn load_vector_index_metadata(
        &self,
        snapshot_id: &str,
    ) -> SemanticStoreResult<Option<StoredVectorIndexMetadata>> {
        let connection = self.connect()?;
        connection
            .query_row(
                r#"
                SELECT
                    snapshot_id,
                    semantic_build_id,
                    index_kind,
                    index_schema_version,
                    vector_dimensions,
                    normalized,
                    indexed_vector_count,
                    created_at
                FROM semantic_vector_index_metadata
                WHERE snapshot_id = ?1
                "#,
                [snapshot_id],
                |row| {
                    Ok(StoredVectorIndexMetadata {
                        snapshot_id: row.get(0)?,
                        semantic_build_id: SemanticBuildId(row.get(1)?),
                        index_kind: row.get(2)?,
                        index_schema_version: row.get(3)?,
                        vector_dimensions: row.get(4)?,
                        normalized: row.get(5)?,
                        indexed_vector_count: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn load_chunk_vectors(
        &self,
        snapshot_id: &str,
    ) -> SemanticStoreResult<Vec<StoredChunkVector>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            r#"
            SELECT chunk_id, cache_key, vector_json
            FROM semantic_chunk_vectors
            WHERE snapshot_id = ?1
            ORDER BY chunk_id ASC
            "#,
        )?;
        let rows = statement.query_map([snapshot_id], |row| {
            let vector_json = row.get::<_, String>(2)?;
            let vector = serde_json::from_str(&vector_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(StoredChunkVector {
                chunk_id: SemanticChunkId(row.get(0)?),
                cache_key: row.get::<_, Option<String>>(1)?.map(EmbeddingCacheKey),
                vector,
            })
        })?;
        let mut chunk_vectors = Vec::new();
        for row in rows {
            chunk_vectors.push(row?);
        }
        Ok(chunk_vectors)
    }

    pub fn load_vector_index(
        &self,
        snapshot_id: &str,
        build: &StoredSemanticBuild,
    ) -> SemanticStoreResult<FlatVectorIndex> {
        let metadata = self
            .load_vector_index_metadata(snapshot_id)?
            .ok_or_else(|| {
                SemanticStoreError::Compatibility(format!(
                    "vector index metadata was not found for snapshot {snapshot_id}"
                ))
            })?;
        if metadata.semantic_build_id != build.semantic_build_id {
            return Err(SemanticStoreError::Compatibility(format!(
                "vector index build mismatch for snapshot {snapshot_id}: expected {}, found {}",
                build.semantic_build_id.0, metadata.semantic_build_id.0
            )));
        }
        if metadata.vector_dimensions != build.embedding_provider.vector_dimensions {
            return Err(SemanticStoreError::Compatibility(format!(
                "vector index dimensions mismatch for snapshot {snapshot_id}: expected {}, found {}",
                build.embedding_provider.vector_dimensions, metadata.vector_dimensions
            )));
        }
        if metadata.normalized != build.embedding_provider.normalized {
            return Err(SemanticStoreError::Compatibility(format!(
                "vector index normalization mismatch for snapshot {snapshot_id}: expected {}, found {}",
                build.embedding_provider.normalized, metadata.normalized
            )));
        }
        FlatVectorIndex::from_persisted(metadata, self.load_chunk_vectors(snapshot_id)?)
    }

    pub fn load_embedding(
        &self,
        cache_key: &EmbeddingCacheKey,
    ) -> SemanticStoreResult<Option<StoredEmbeddingRecord>> {
        let mut loaded = self.load_embeddings(std::slice::from_ref(cache_key))?;
        Ok(loaded.remove(cache_key))
    }

    pub fn persist_embeddings(&self, records: &[StoredEmbeddingRecord]) -> SemanticStoreResult<()> {
        if records.is_empty() {
            return Ok(());
        }
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        for record in records {
            transaction.execute(
                r#"
                INSERT INTO embedding_cache (
                    cache_key,
                    input_kind,
                    provider_kind,
                    provider_id,
                    provider_version,
                    model_id,
                    model_digest,
                    provider_config_digest,
                    text_digest,
                    dimensions,
                    normalized,
                    vector_json,
                    stored_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ON CONFLICT(cache_key) DO UPDATE SET
                    input_kind = excluded.input_kind,
                    provider_kind = excluded.provider_kind,
                    provider_id = excluded.provider_id,
                    provider_version = excluded.provider_version,
                    model_id = excluded.model_id,
                    model_digest = excluded.model_digest,
                    provider_config_digest = excluded.provider_config_digest,
                    text_digest = excluded.text_digest,
                    dimensions = excluded.dimensions,
                    normalized = excluded.normalized,
                    vector_json = excluded.vector_json,
                    stored_at = excluded.stored_at
                "#,
                params![
                    record.cache_key.0,
                    input_kind_name(&record.input_kind),
                    provider_kind_name(&record.provider_kind),
                    record.provider_id,
                    record.provider_version,
                    record.model_id,
                    record.model_digest,
                    record.provider_config_digest,
                    record.text_digest,
                    record.dimensions,
                    record.normalized,
                    serde_json::to_string(&record.vector)?,
                    record.stored_at,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn embedding_entry_count(&self) -> SemanticStoreResult<usize> {
        let connection = self.connect()?;
        connection
            .query_row("SELECT COUNT(*) FROM embedding_cache", [], |row| row.get(0))
            .map_err(Into::into)
    }

    pub fn status(&self) -> SemanticStoreResult<SemanticStoreStatus> {
        let connection = self.connect()?;
        let schema_version =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let build_count = connection
            .query_row("SELECT COUNT(*) FROM semantic_builds", [], |row| row.get(0))
            .optional()?
            .unwrap_or(0usize);
        Ok(SemanticStoreStatus {
            db_path: self.store_path.display().to_string(),
            schema_version,
            build_count,
        })
    }

    pub fn quick_check(&self) -> SemanticStoreResult<Vec<String>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare("PRAGMA quick_check(1)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    fn migrate(&self) -> SemanticStoreResult<()> {
        let connection = self.connect()?;
        let current_version =
            connection.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?;
        for migration in planned_migrations() {
            if migration.schema_version <= current_version {
                continue;
            }
            if migration.schema_version == 1 {
                connection.execute_batch(
                    r#"
                    CREATE TABLE IF NOT EXISTS semantic_builds (
                        snapshot_id TEXT PRIMARY KEY,
                        repo_id TEXT NOT NULL,
                        semantic_build_id TEXT NOT NULL,
                        semantic_config_digest TEXT NOT NULL,
                        schema_version INTEGER NOT NULL,
                        chunk_schema_version INTEGER NOT NULL,
                        embedding_model_id TEXT NOT NULL,
                        embedding_model_digest TEXT NOT NULL,
                        symbol_index_build_id TEXT,
                        created_at TEXT NOT NULL,
                        refresh_mode TEXT NOT NULL,
                        chunk_count INTEGER NOT NULL,
                        indexed_file_count INTEGER NOT NULL,
                        embedding_count INTEGER NOT NULL,
                        build_json TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS embedding_cache (
                        cache_key TEXT PRIMARY KEY,
                        model_digest TEXT NOT NULL,
                        dimensions INTEGER NOT NULL,
                        vector_json TEXT NOT NULL
                    );
                    "#,
                )?;
            }
            if migration.schema_version == 2 {
                connection.execute_batch(
                    r#"
                    CREATE TABLE IF NOT EXISTS semantic_chunks (
                        snapshot_id TEXT NOT NULL,
                        semantic_build_id TEXT NOT NULL,
                        chunk_id TEXT NOT NULL,
                        path TEXT NOT NULL,
                        chunk_kind TEXT NOT NULL,
                        source_kind TEXT NOT NULL,
                        symbol_id TEXT,
                        text_digest TEXT NOT NULL,
                        content_sha256 TEXT NOT NULL,
                        record_json TEXT NOT NULL,
                        PRIMARY KEY (snapshot_id, chunk_id)
                    );
                    CREATE INDEX IF NOT EXISTS idx_semantic_chunks_snapshot_path
                        ON semantic_chunks (snapshot_id, path);
                    CREATE INDEX IF NOT EXISTS idx_semantic_chunks_snapshot_symbol
                        ON semantic_chunks (snapshot_id, symbol_id);
                    "#,
                )?;
            }
            if migration.schema_version == 3 {
                connection.execute_batch(
                    r#"
                    ALTER TABLE embedding_cache ADD COLUMN input_kind TEXT NOT NULL DEFAULT 'document';
                    ALTER TABLE embedding_cache ADD COLUMN provider_kind TEXT NOT NULL DEFAULT 'placeholder';
                    ALTER TABLE embedding_cache ADD COLUMN provider_id TEXT NOT NULL DEFAULT 'placeholder';
                    ALTER TABLE embedding_cache ADD COLUMN provider_version TEXT NOT NULL DEFAULT 'v1';
                    ALTER TABLE embedding_cache ADD COLUMN model_id TEXT NOT NULL DEFAULT '';
                    ALTER TABLE embedding_cache ADD COLUMN provider_config_digest TEXT NOT NULL DEFAULT '';
                    ALTER TABLE embedding_cache ADD COLUMN text_digest TEXT NOT NULL DEFAULT '';
                    ALTER TABLE embedding_cache ADD COLUMN normalized INTEGER NOT NULL DEFAULT 1;
                    ALTER TABLE embedding_cache ADD COLUMN stored_at TEXT NOT NULL DEFAULT '';
                    "#,
                )?;
            }
            if migration.schema_version == 4 {
                connection.execute_batch(
                    r#"
                    CREATE TABLE IF NOT EXISTS semantic_vector_index_metadata (
                        snapshot_id TEXT PRIMARY KEY,
                        semantic_build_id TEXT NOT NULL,
                        index_kind TEXT NOT NULL,
                        index_schema_version INTEGER NOT NULL,
                        vector_dimensions INTEGER NOT NULL,
                        normalized INTEGER NOT NULL,
                        indexed_vector_count INTEGER NOT NULL,
                        created_at TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS semantic_chunk_vectors (
                        snapshot_id TEXT NOT NULL,
                        semantic_build_id TEXT NOT NULL,
                        chunk_id TEXT NOT NULL,
                        cache_key TEXT,
                        vector_json TEXT NOT NULL,
                        PRIMARY KEY (snapshot_id, chunk_id)
                    );
                    CREATE INDEX IF NOT EXISTS idx_semantic_chunk_vectors_snapshot_build
                        ON semantic_chunk_vectors (snapshot_id, semantic_build_id);
                    "#,
                )?;
            }
            connection.pragma_update(None, "user_version", migration.schema_version)?;
        }
        validate_schema(&connection)?;
        Ok(())
    }

    fn connect(&self) -> SemanticStoreResult<Connection> {
        Ok(Connection::open(&self.store_path)?)
    }
}

impl EmbeddingCacheStore for SemanticStore {
    fn load_embedding(
        &self,
        cache_key: &EmbeddingCacheKey,
    ) -> SemanticStoreResult<Option<StoredEmbeddingRecord>> {
        self.load_embedding(cache_key)
    }

    fn load_embeddings(
        &self,
        cache_keys: &[EmbeddingCacheKey],
    ) -> SemanticStoreResult<BTreeMap<EmbeddingCacheKey, StoredEmbeddingRecord>> {
        let connection = self.connect()?;
        load_embeddings_with_connection(&connection, cache_keys)
    }

    fn persist_embeddings(&self, records: &[StoredEmbeddingRecord]) -> SemanticStoreResult<()> {
        self.persist_embeddings(records)
    }

    fn embedding_entry_count(&self) -> SemanticStoreResult<usize> {
        self.embedding_entry_count()
    }
}

fn load_embeddings_with_connection(
    connection: &Connection,
    cache_keys: &[EmbeddingCacheKey],
) -> SemanticStoreResult<BTreeMap<EmbeddingCacheKey, StoredEmbeddingRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT
            cache_key,
            input_kind,
            provider_kind,
            provider_id,
            provider_version,
            model_id,
            model_digest,
            provider_config_digest,
            text_digest,
            dimensions,
            normalized,
            vector_json,
            stored_at
        FROM embedding_cache
        WHERE cache_key = ?1
        "#,
    )?;
    let mut loaded = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut corrupted = Vec::new();

    for cache_key in cache_keys {
        if !seen.insert(cache_key.0.clone()) {
            continue;
        }
        match statement.query_row([cache_key.0.as_str()], stored_embedding_record_from_row) {
            Ok(record) => {
                loaded.insert(record.cache_key.clone(), record);
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(error) if is_corrupt_embedding_row_error(&error) => {
                corrupted.push(cache_key.clone());
            }
            Err(error) => return Err(error.into()),
        }
    }
    drop(statement);

    if !corrupted.is_empty() {
        delete_embedding_keys(connection, &corrupted)?;
    }

    Ok(loaded)
}

fn stored_embedding_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredEmbeddingRecord> {
    let input_kind = parse_input_kind(row.get::<_, String>(1)?.as_str())?;
    let provider_kind = parse_provider_kind(row.get::<_, String>(2)?.as_str())?;
    let vector_json = row.get::<_, String>(11)?;
    Ok(StoredEmbeddingRecord {
        cache_key: EmbeddingCacheKey(row.get::<_, String>(0)?),
        input_kind,
        provider_kind,
        provider_id: row.get(3)?,
        provider_version: row.get(4)?,
        model_id: row.get(5)?,
        model_digest: row.get(6)?,
        provider_config_digest: row.get(7)?,
        text_digest: row.get(8)?,
        dimensions: row.get(9)?,
        normalized: row.get(10)?,
        vector: serde_json::from_str(&vector_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                11,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        stored_at: row.get(12)?,
    })
}

fn is_corrupt_embedding_row_error(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::FromSqlConversionFailure(..)
            | rusqlite::Error::InvalidColumnType(..)
            | rusqlite::Error::IntegralValueOutOfRange(..)
    )
}

fn delete_embedding_keys(
    connection: &Connection,
    cache_keys: &[EmbeddingCacheKey],
) -> SemanticStoreResult<()> {
    let mut statement = connection.prepare("DELETE FROM embedding_cache WHERE cache_key = ?1")?;
    for cache_key in cache_keys {
        statement.execute([cache_key.0.as_str()])?;
    }
    Ok(())
}

fn persist_build_row(
    connection: &Connection,
    build: &StoredSemanticBuild,
) -> SemanticStoreResult<()> {
    connection.execute(
        r#"
        INSERT INTO semantic_builds (
            snapshot_id,
            repo_id,
            semantic_build_id,
            semantic_config_digest,
            schema_version,
            chunk_schema_version,
            embedding_model_id,
            embedding_model_digest,
            symbol_index_build_id,
            created_at,
            refresh_mode,
            chunk_count,
            indexed_file_count,
            embedding_count,
            build_json
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT(snapshot_id) DO UPDATE SET
            repo_id = excluded.repo_id,
            semantic_build_id = excluded.semantic_build_id,
            semantic_config_digest = excluded.semantic_config_digest,
            schema_version = excluded.schema_version,
            chunk_schema_version = excluded.chunk_schema_version,
            embedding_model_id = excluded.embedding_model_id,
            embedding_model_digest = excluded.embedding_model_digest,
            symbol_index_build_id = excluded.symbol_index_build_id,
            created_at = excluded.created_at,
            refresh_mode = excluded.refresh_mode,
            chunk_count = excluded.chunk_count,
            indexed_file_count = excluded.indexed_file_count,
            embedding_count = excluded.embedding_count,
            build_json = excluded.build_json
        "#,
        params![
            build.snapshot_id,
            build.repo_id,
            build.semantic_build_id.0,
            build.semantic_config_digest,
            build.schema_version,
            build.chunk_schema_version,
            build.embedding_provider.model_id,
            build.embedding_provider.model_digest,
            build
                .symbol_index_build_id
                .as_ref()
                .map(|build_id| build_id.0.clone()),
            build.created_at,
            build.refresh_mode,
            build.chunk_count,
            build.indexed_file_count,
            build.embedding_count,
            serde_json::to_string(build)?,
        ],
    )?;
    Ok(())
}

fn persist_vector_index_rows(
    connection: &Connection,
    build: &StoredSemanticBuild,
    chunk_vectors: &[StoredChunkVector],
) -> SemanticStoreResult<()> {
    let metadata = StoredVectorIndexMetadata::flat(
        &build.snapshot_id,
        build.semantic_build_id.clone(),
        build.embedding_provider.vector_dimensions,
        build.embedding_provider.normalized,
        chunk_vectors.len(),
        &build.created_at,
    );
    connection.execute(
        r#"
        INSERT INTO semantic_vector_index_metadata (
            snapshot_id,
            semantic_build_id,
            index_kind,
            index_schema_version,
            vector_dimensions,
            normalized,
            indexed_vector_count,
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            metadata.snapshot_id,
            metadata.semantic_build_id.0,
            metadata.index_kind,
            metadata.index_schema_version,
            metadata.vector_dimensions,
            metadata.normalized,
            metadata.indexed_vector_count,
            metadata.created_at,
        ],
    )?;
    for chunk_vector in chunk_vectors {
        connection.execute(
            r#"
            INSERT INTO semantic_chunk_vectors (
                snapshot_id,
                semantic_build_id,
                chunk_id,
                cache_key,
                vector_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                build.snapshot_id,
                build.semantic_build_id.0,
                chunk_vector.chunk_id.0,
                chunk_vector
                    .cache_key
                    .as_ref()
                    .map(|cache_key| cache_key.0.clone()),
                serde_json::to_string(&chunk_vector.vector)?,
            ],
        )?;
    }
    Ok(())
}

fn validate_schema(connection: &Connection) -> SemanticStoreResult<()> {
    let mut statement = connection.prepare("PRAGMA table_info(semantic_builds)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = std::collections::BTreeSet::new();
    for row in rows {
        columns.insert(row?);
    }
    for column in [
        "snapshot_id",
        "repo_id",
        "semantic_build_id",
        "semantic_config_digest",
        "schema_version",
        "chunk_schema_version",
        "embedding_model_id",
        "embedding_model_digest",
        "symbol_index_build_id",
        "created_at",
        "refresh_mode",
        "chunk_count",
        "indexed_file_count",
        "embedding_count",
        "build_json",
    ] {
        if !columns.contains(column) {
            return Err(
                rusqlite::Error::InvalidColumnName(format!("semantic_builds.{column}")).into(),
            );
        }
    }
    let mut statement = connection.prepare("PRAGMA table_info(semantic_chunks)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = std::collections::BTreeSet::new();
    for row in rows {
        columns.insert(row?);
    }
    for column in [
        "snapshot_id",
        "semantic_build_id",
        "chunk_id",
        "path",
        "chunk_kind",
        "source_kind",
        "symbol_id",
        "text_digest",
        "content_sha256",
        "record_json",
    ] {
        if !columns.contains(column) {
            return Err(
                rusqlite::Error::InvalidColumnName(format!("semantic_chunks.{column}")).into(),
            );
        }
    }
    let mut statement = connection.prepare("PRAGMA table_info(semantic_vector_index_metadata)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = std::collections::BTreeSet::new();
    for row in rows {
        columns.insert(row?);
    }
    for column in [
        "snapshot_id",
        "semantic_build_id",
        "index_kind",
        "index_schema_version",
        "vector_dimensions",
        "normalized",
        "indexed_vector_count",
        "created_at",
    ] {
        if !columns.contains(column) {
            return Err(rusqlite::Error::InvalidColumnName(format!(
                "semantic_vector_index_metadata.{column}"
            ))
            .into());
        }
    }
    let mut statement = connection.prepare("PRAGMA table_info(semantic_chunk_vectors)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = std::collections::BTreeSet::new();
    for row in rows {
        columns.insert(row?);
    }
    for column in [
        "snapshot_id",
        "semantic_build_id",
        "chunk_id",
        "cache_key",
        "vector_json",
    ] {
        if !columns.contains(column) {
            return Err(rusqlite::Error::InvalidColumnName(format!(
                "semantic_chunk_vectors.{column}"
            ))
            .into());
        }
    }
    let mut statement = connection.prepare("PRAGMA table_info(embedding_cache)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = std::collections::BTreeSet::new();
    for row in rows {
        columns.insert(row?);
    }
    for column in [
        "cache_key",
        "input_kind",
        "provider_kind",
        "provider_id",
        "provider_version",
        "model_id",
        "model_digest",
        "provider_config_digest",
        "text_digest",
        "dimensions",
        "normalized",
        "vector_json",
        "stored_at",
    ] {
        if !columns.contains(column) {
            return Err(
                rusqlite::Error::InvalidColumnName(format!("embedding_cache.{column}")).into(),
            );
        }
    }
    Ok(())
}

fn input_kind_name(kind: &SemanticEmbeddingInputKind) -> &'static str {
    match kind {
        SemanticEmbeddingInputKind::Document => "document",
        SemanticEmbeddingInputKind::Query => "query",
    }
}

fn provider_kind_name(kind: &SemanticEmbeddingProviderKind) -> &'static str {
    match kind {
        SemanticEmbeddingProviderKind::DeterministicFixture => "deterministic_fixture",
        SemanticEmbeddingProviderKind::LocalOnnx => "local_onnx",
        SemanticEmbeddingProviderKind::ExternalProcess => "external_process",
        SemanticEmbeddingProviderKind::Placeholder => "placeholder",
    }
}

fn parse_input_kind(raw: &str) -> rusqlite::Result<SemanticEmbeddingInputKind> {
    match raw {
        "document" => Ok(SemanticEmbeddingInputKind::Document),
        "query" => Ok(SemanticEmbeddingInputKind::Query),
        other => Err(rusqlite::Error::InvalidColumnType(
            1,
            format!("embedding_cache.input_kind={other}"),
            rusqlite::types::Type::Text,
        )),
    }
}

fn parse_provider_kind(raw: &str) -> rusqlite::Result<SemanticEmbeddingProviderKind> {
    match raw {
        "deterministic_fixture" => Ok(SemanticEmbeddingProviderKind::DeterministicFixture),
        "local_onnx" => Ok(SemanticEmbeddingProviderKind::LocalOnnx),
        "external_process" => Ok(SemanticEmbeddingProviderKind::ExternalProcess),
        "placeholder" => Ok(SemanticEmbeddingProviderKind::Placeholder),
        other => Err(rusqlite::Error::InvalidColumnType(
            2,
            format!("embedding_cache.provider_kind={other}"),
            rusqlite::types::Type::Text,
        )),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{SemanticStore, StoredSemanticBuild};
    use crate::{StoredChunkVector, StoredEmbeddingRecord};
    use hyperindex_protocol::semantic::{
        EmbeddingCacheKey, SemanticBuildId, SemanticChunkId, SemanticChunkKind,
        SemanticChunkMetadata, SemanticChunkRecord, SemanticChunkSourceKind,
        SemanticChunkTextConfig, SemanticChunkTextMetadata, SemanticDiagnostic,
        SemanticEmbeddingInputKind, SemanticEmbeddingProviderConfig, SemanticEmbeddingProviderKind,
    };

    #[test]
    fn persisted_build_roundtrips_cleanly() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let build = StoredSemanticBuild {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            semantic_build_id: SemanticBuildId("semantic-build-123".to_string()),
            semantic_config_digest: "config-digest".to_string(),
            schema_version: store.schema_version,
            chunk_schema_version: 1,
            embedding_provider: SemanticEmbeddingProviderConfig {
                provider_kind: SemanticEmbeddingProviderKind::Placeholder,
                model_id: "model".to_string(),
                model_digest: "model-v1".to_string(),
                vector_dimensions: 384,
                normalized: true,
                max_input_bytes: 8192,
                max_batch_size: 32,
            },
            chunk_text: SemanticChunkTextConfig {
                serializer_id: "phase6-structured-text".to_string(),
                format_version: 1,
                includes_path_header: true,
                includes_symbol_context: true,
                normalized_newlines: true,
            },
            symbol_index_build_id: None,
            created_at: "123".to_string(),
            refresh_mode: "scaffold_bootstrap".to_string(),
            chunk_count: 0,
            indexed_file_count: 0,
            embedding_count: 0,
            embedding_cache_hits: 0,
            embedding_cache_misses: 0,
            embedding_cache_writes: 0,
            embedding_provider_batches: 0,
            profile: None,
            refresh_stats: None,
            fallback_reason: None,
            diagnostics: Vec::<SemanticDiagnostic>::new(),
        };
        store.persist_build(&build).unwrap();
        let loaded = store.load_build("snap-123").unwrap().unwrap();
        assert_eq!(loaded, build);
    }

    #[test]
    fn persisted_chunks_roundtrip_cleanly() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let build = StoredSemanticBuild {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            semantic_build_id: SemanticBuildId("semantic-build-123".to_string()),
            semantic_config_digest: "config-digest".to_string(),
            schema_version: store.schema_version,
            chunk_schema_version: 1,
            embedding_provider: SemanticEmbeddingProviderConfig {
                provider_kind: SemanticEmbeddingProviderKind::Placeholder,
                model_id: "model".to_string(),
                model_digest: "model-v1".to_string(),
                vector_dimensions: 384,
                normalized: true,
                max_input_bytes: 8192,
                max_batch_size: 32,
            },
            chunk_text: SemanticChunkTextConfig {
                serializer_id: "phase6-structured-text".to_string(),
                format_version: 1,
                includes_path_header: true,
                includes_symbol_context: true,
                normalized_newlines: true,
            },
            symbol_index_build_id: None,
            created_at: "123".to_string(),
            refresh_mode: "full_rebuild".to_string(),
            chunk_count: 1,
            indexed_file_count: 1,
            embedding_count: 0,
            embedding_cache_hits: 0,
            embedding_cache_misses: 0,
            embedding_cache_writes: 0,
            embedding_provider_batches: 0,
            profile: None,
            refresh_stats: None,
            fallback_reason: None,
            diagnostics: Vec::<SemanticDiagnostic>::new(),
        };
        let chunk = SemanticChunkRecord {
            metadata: SemanticChunkMetadata {
                chunk_id: SemanticChunkId("chunk-123".to_string()),
                chunk_kind: SemanticChunkKind::SymbolBody,
                source_kind: SemanticChunkSourceKind::Symbol,
                path: "src/session.ts".to_string(),
                language: Some(hyperindex_protocol::symbols::LanguageId::Typescript),
                extension: Some("ts".to_string()),
                package_name: Some("@hyperindex/auth".to_string()),
                package_root: Some("packages/auth".to_string()),
                workspace_root: Some(".".to_string()),
                symbol_id: Some(hyperindex_protocol::symbols::SymbolId(
                    "sym.invalidate_session".to_string(),
                )),
                symbol_display_name: Some("invalidateSession".to_string()),
                symbol_kind: Some(hyperindex_protocol::symbols::SymbolKind::Function),
                symbol_is_exported: Some(true),
                symbol_is_default_export: Some(false),
                span: Some(hyperindex_protocol::symbols::SourceSpan {
                    start: hyperindex_protocol::symbols::LinePosition { line: 1, column: 1 },
                    end: hyperindex_protocol::symbols::LinePosition { line: 3, column: 2 },
                    bytes: hyperindex_protocol::symbols::ByteRange { start: 0, end: 64 },
                }),
                content_sha256: "sha-content".to_string(),
                text: SemanticChunkTextMetadata {
                    serializer_id: "phase6-structured-text".to_string(),
                    format_version: 1,
                    text_digest: "sha-text".to_string(),
                    text_bytes: 64,
                    token_count_estimate: 8,
                },
            },
            serialized_text:
                "path: src/session.ts\n\nsource:\nexport function invalidateSession() {}"
                    .to_string(),
            embedding_cache: None,
        };

        store
            .persist_build_with_chunks(&build, std::slice::from_ref(&chunk))
            .unwrap();
        let loaded = store
            .load_chunk("snap-123", &SemanticChunkId("chunk-123".to_string()))
            .unwrap()
            .unwrap();
        let listed = store.list_chunks("snap-123").unwrap();

        assert_eq!(loaded, chunk);
        assert_eq!(listed, vec![chunk]);
    }

    #[test]
    fn persisted_vector_index_roundtrips_and_warm_loads_cleanly() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let build = StoredSemanticBuild {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            semantic_build_id: SemanticBuildId("semantic-build-123".to_string()),
            semantic_config_digest: "config-digest".to_string(),
            schema_version: store.schema_version,
            chunk_schema_version: 1,
            embedding_provider: SemanticEmbeddingProviderConfig {
                provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
                model_id: "fixture-model".to_string(),
                model_digest: "model-v1".to_string(),
                vector_dimensions: 3,
                normalized: true,
                max_input_bytes: 8192,
                max_batch_size: 32,
            },
            chunk_text: SemanticChunkTextConfig {
                serializer_id: "phase6-structured-text".to_string(),
                format_version: 1,
                includes_path_header: true,
                includes_symbol_context: true,
                normalized_newlines: true,
            },
            symbol_index_build_id: None,
            created_at: "123".to_string(),
            refresh_mode: "full_rebuild".to_string(),
            chunk_count: 1,
            indexed_file_count: 1,
            embedding_count: 1,
            embedding_cache_hits: 0,
            embedding_cache_misses: 0,
            embedding_cache_writes: 1,
            embedding_provider_batches: 1,
            profile: None,
            refresh_stats: None,
            fallback_reason: None,
            diagnostics: Vec::<SemanticDiagnostic>::new(),
        };
        let chunk = SemanticChunkRecord {
            metadata: SemanticChunkMetadata {
                chunk_id: SemanticChunkId("chunk-123".to_string()),
                chunk_kind: SemanticChunkKind::SymbolBody,
                source_kind: SemanticChunkSourceKind::Symbol,
                path: "src/session.ts".to_string(),
                language: Some(hyperindex_protocol::symbols::LanguageId::Typescript),
                extension: Some("ts".to_string()),
                package_name: None,
                package_root: None,
                workspace_root: Some(".".to_string()),
                symbol_id: None,
                symbol_display_name: Some("invalidateSession".to_string()),
                symbol_kind: Some(hyperindex_protocol::symbols::SymbolKind::Function),
                symbol_is_exported: Some(true),
                symbol_is_default_export: Some(false),
                span: None,
                content_sha256: "sha-content".to_string(),
                text: SemanticChunkTextMetadata {
                    serializer_id: "phase6-structured-text".to_string(),
                    format_version: 1,
                    text_digest: "sha-text".to_string(),
                    text_bytes: 64,
                    token_count_estimate: 8,
                },
            },
            serialized_text: "invalidateSession".to_string(),
            embedding_cache: Some(
                hyperindex_protocol::semantic::SemanticEmbeddingCacheMetadata {
                    cache_key: EmbeddingCacheKey("cache-123".to_string()),
                    input_kind: SemanticEmbeddingInputKind::Document,
                    model_digest: "model-v1".to_string(),
                    text_digest: "sha-text".to_string(),
                    provider_config_digest: "config-v1".to_string(),
                    vector_dimensions: 3,
                    normalized: true,
                    stored_at: Some("123".to_string()),
                },
            ),
        };
        let chunk_vector = StoredChunkVector {
            chunk_id: chunk.metadata.chunk_id.clone(),
            cache_key: Some(EmbeddingCacheKey("cache-123".to_string())),
            vector: vec![1.0, 0.0, 0.0],
        };

        store
            .persist_build_with_chunks_and_vectors(
                &build,
                std::slice::from_ref(&chunk),
                std::slice::from_ref(&chunk_vector),
            )
            .unwrap();

        let loaded = store.load_vector_index("snap-123", &build).unwrap();
        assert_eq!(loaded.metadata().indexed_vector_count, 1);
        assert_eq!(loaded.chunk_count(), 1);
        assert_eq!(
            loaded
                .score_chunk_id(&[1.0, 0.0, 0.0], &chunk.metadata.chunk_id.0)
                .unwrap(),
            1_000_000
        );

        let reopened = SemanticStore::open_at_path(store.store_path.clone()).unwrap();
        let warm_loaded = reopened.load_vector_index("snap-123", &build).unwrap();
        assert_eq!(warm_loaded.metadata(), loaded.metadata());
        assert_eq!(warm_loaded.chunk_count(), loaded.chunk_count());
    }

    #[test]
    fn vector_index_version_mismatches_fail_clearly() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let build = StoredSemanticBuild {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            semantic_build_id: SemanticBuildId("semantic-build-123".to_string()),
            semantic_config_digest: "config-digest".to_string(),
            schema_version: store.schema_version,
            chunk_schema_version: 1,
            embedding_provider: SemanticEmbeddingProviderConfig {
                provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
                model_id: "fixture-model".to_string(),
                model_digest: "model-v1".to_string(),
                vector_dimensions: 2,
                normalized: true,
                max_input_bytes: 8192,
                max_batch_size: 32,
            },
            chunk_text: SemanticChunkTextConfig {
                serializer_id: "phase6-structured-text".to_string(),
                format_version: 1,
                includes_path_header: true,
                includes_symbol_context: true,
                normalized_newlines: true,
            },
            symbol_index_build_id: None,
            created_at: "123".to_string(),
            refresh_mode: "full_rebuild".to_string(),
            chunk_count: 0,
            indexed_file_count: 0,
            embedding_count: 1,
            embedding_cache_hits: 0,
            embedding_cache_misses: 0,
            embedding_cache_writes: 1,
            embedding_provider_batches: 1,
            profile: None,
            refresh_stats: None,
            fallback_reason: None,
            diagnostics: Vec::<SemanticDiagnostic>::new(),
        };
        store
            .persist_build_with_chunks_and_vectors(
                &build,
                &[],
                &[StoredChunkVector {
                    chunk_id: SemanticChunkId("chunk-123".to_string()),
                    cache_key: None,
                    vector: vec![1.0, 0.0],
                }],
            )
            .unwrap();

        let connection = rusqlite::Connection::open(&store.store_path).unwrap();
        connection
            .execute(
                "UPDATE semantic_vector_index_metadata SET index_schema_version = ?1 WHERE snapshot_id = ?2",
                rusqlite::params![99u32, "snap-123"],
            )
            .unwrap();

        let error = store.load_vector_index("snap-123", &build).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("vector index schema version mismatch")
        );
    }

    #[test]
    fn persisted_embeddings_roundtrip_cleanly() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let record = StoredEmbeddingRecord {
            cache_key: EmbeddingCacheKey("cache-123".to_string()),
            input_kind: SemanticEmbeddingInputKind::Document,
            provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
            provider_id: "deterministic-fixture".to_string(),
            provider_version: "v1".to_string(),
            model_id: "fixture-model".to_string(),
            model_digest: "model-v1".to_string(),
            provider_config_digest: "config-v1".to_string(),
            text_digest: "text-v1".to_string(),
            dimensions: 3,
            normalized: true,
            vector: vec![0.1, 0.2, 0.3],
            stored_at: "123".to_string(),
        };

        store
            .persist_embeddings(std::slice::from_ref(&record))
            .unwrap();
        let loaded = store.load_embedding(&record.cache_key).unwrap().unwrap();

        assert_eq!(loaded, record);
        assert_eq!(store.embedding_entry_count().unwrap(), 1);
    }

    #[test]
    fn manifest_uses_build_scoped_embedding_count() {
        let tempdir = tempdir().unwrap();
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-123").unwrap();
        let build = StoredSemanticBuild {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            semantic_build_id: SemanticBuildId("semantic-build-123".to_string()),
            semantic_config_digest: "config-digest".to_string(),
            schema_version: store.schema_version,
            chunk_schema_version: 1,
            embedding_provider: SemanticEmbeddingProviderConfig {
                provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
                model_id: "fixture-model".to_string(),
                model_digest: "model-v1".to_string(),
                vector_dimensions: 3,
                normalized: true,
                max_input_bytes: 8192,
                max_batch_size: 32,
            },
            chunk_text: SemanticChunkTextConfig {
                serializer_id: "phase6-structured-text".to_string(),
                format_version: 1,
                includes_path_header: true,
                includes_symbol_context: true,
                normalized_newlines: true,
            },
            symbol_index_build_id: None,
            created_at: "123".to_string(),
            refresh_mode: "full_rebuild".to_string(),
            chunk_count: 1,
            indexed_file_count: 1,
            embedding_count: 1,
            embedding_cache_hits: 0,
            embedding_cache_misses: 0,
            embedding_cache_writes: 1,
            embedding_provider_batches: 1,
            profile: None,
            refresh_stats: None,
            fallback_reason: None,
            diagnostics: Vec::<SemanticDiagnostic>::new(),
        };
        store.persist_build(&build).unwrap();
        store
            .persist_embeddings(&[
                StoredEmbeddingRecord {
                    cache_key: EmbeddingCacheKey("cache-doc-123".to_string()),
                    input_kind: SemanticEmbeddingInputKind::Document,
                    provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
                    provider_id: "deterministic-fixture".to_string(),
                    provider_version: "v1".to_string(),
                    model_id: "fixture-model".to_string(),
                    model_digest: "model-v1".to_string(),
                    provider_config_digest: "config-v1".to_string(),
                    text_digest: "text-doc-v1".to_string(),
                    dimensions: 3,
                    normalized: true,
                    vector: vec![0.1, 0.2, 0.3],
                    stored_at: "123".to_string(),
                },
                StoredEmbeddingRecord {
                    cache_key: EmbeddingCacheKey("cache-query-123".to_string()),
                    input_kind: SemanticEmbeddingInputKind::Query,
                    provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
                    provider_id: "deterministic-fixture".to_string(),
                    provider_version: "v1".to_string(),
                    model_id: "fixture-model".to_string(),
                    model_digest: "model-v1".to_string(),
                    provider_config_digest: "config-v1".to_string(),
                    text_digest: "text-query-v1".to_string(),
                    dimensions: 3,
                    normalized: true,
                    vector: vec![0.4, 0.5, 0.6],
                    stored_at: "124".to_string(),
                },
            ])
            .unwrap();

        let manifest = store.manifest_for(&build);

        assert_eq!(store.embedding_entry_count().unwrap(), 2);
        assert_eq!(manifest.embedding_cache.entry_count, 1);
    }
}
