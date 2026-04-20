use std::fs;
use std::path::{Path, PathBuf};

use hyperindex_symbols::{
    ExportFactRecord, ExtractedFileFacts, FactsBatch, ImportFactRecord, SymbolFactRecord,
};
use rusqlite::{Connection, OptionalExtension, params};
use tracing::{debug, info};

use crate::SymbolStoreResult;
use crate::migrations::{SYMBOL_STORE_MIGRATIONS, SYMBOL_STORE_SCHEMA_VERSION};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedSnapshot {
    pub repo_id: String,
    pub snapshot_id: String,
    pub planned_file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolStoreStatus {
    pub db_path: String,
    pub schema_version: u32,
    pub indexed_snapshots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotStorageStats {
    pub indexed_file_count: usize,
    pub symbol_fact_count: usize,
    pub occurrence_count: usize,
    pub edge_count: usize,
    pub import_fact_count: usize,
    pub export_fact_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSnapshotState {
    pub repo_id: String,
    pub snapshot_id: String,
    pub parser_config_digest: String,
    pub schema_version: u32,
    pub indexed_file_count: usize,
    pub refresh_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSnapshot {
    pub snapshot_id: String,
    pub files: Vec<ExtractedFileFacts>,
}

#[derive(Debug, Clone)]
pub struct SymbolStore {
    db_path: PathBuf,
}

impl SymbolStore {
    pub fn open(root: &Path, repo_id: &str) -> SymbolStoreResult<Self> {
        fs::create_dir_all(root)?;
        let store = Self {
            db_path: root.join(format!("{repo_id}.symbols.sqlite3")),
        };
        store.migrate()?;
        info!(repo_id = repo_id, db_path = %store.db_path.display(), "opened phase4 symbol store scaffold");
        Ok(store)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn record_scaffold_snapshot(
        &self,
        repo_id: &str,
        snapshot_id: &str,
        planned_file_count: usize,
    ) -> SymbolStoreResult<RecordedSnapshot> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            INSERT INTO scaffold_snapshots (repo_id, snapshot_id, planned_file_count, updated_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            ON CONFLICT(snapshot_id) DO UPDATE SET
                repo_id = excluded.repo_id,
                planned_file_count = excluded.planned_file_count,
                updated_at = CURRENT_TIMESTAMP
            "#,
            params![repo_id, snapshot_id, planned_file_count],
        )?;
        debug!(
            repo_id = repo_id,
            snapshot_id = snapshot_id,
            planned_file_count,
            "recorded scaffold snapshot metadata"
        );
        Ok(RecordedSnapshot {
            repo_id: repo_id.to_string(),
            snapshot_id: snapshot_id.to_string(),
            planned_file_count,
        })
    }

    pub fn persist_facts(
        &self,
        repo_id: &str,
        snapshot_id: &str,
        batch: &FactsBatch,
    ) -> SymbolStoreResult<RecordedSnapshot> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;

        transaction.execute(
            r#"
            INSERT INTO scaffold_snapshots (repo_id, snapshot_id, planned_file_count, updated_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            ON CONFLICT(snapshot_id) DO UPDATE SET
                repo_id = excluded.repo_id,
                planned_file_count = excluded.planned_file_count,
                updated_at = CURRENT_TIMESTAMP
            "#,
            params![repo_id, snapshot_id, batch.files.len()],
        )?;
        Self::delete_snapshot_rows(&transaction, snapshot_id)?;
        Self::insert_snapshot_files(&transaction, snapshot_id, &batch.files)?;

        transaction.commit()?;
        debug!(
            repo_id = repo_id,
            snapshot_id = snapshot_id,
            files = batch.files.len(),
            symbols = batch.symbol_count(),
            "persisted extracted symbol facts"
        );
        Ok(RecordedSnapshot {
            repo_id: repo_id.to_string(),
            snapshot_id: snapshot_id.to_string(),
            planned_file_count: batch.files.len(),
        })
    }

    pub fn record_indexed_snapshot_state(
        &self,
        state: &IndexedSnapshotState,
    ) -> SymbolStoreResult<()> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            INSERT INTO indexed_snapshot_state (
                snapshot_id,
                repo_id,
                parser_config_digest,
                schema_version,
                indexed_file_count,
                refresh_mode,
                indexed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)
            ON CONFLICT(snapshot_id) DO UPDATE SET
                repo_id = excluded.repo_id,
                parser_config_digest = excluded.parser_config_digest,
                schema_version = excluded.schema_version,
                indexed_file_count = excluded.indexed_file_count,
                refresh_mode = excluded.refresh_mode,
                indexed_at = CURRENT_TIMESTAMP
            "#,
            params![
                state.snapshot_id,
                state.repo_id,
                state.parser_config_digest,
                state.schema_version,
                state.indexed_file_count,
                state.refresh_mode,
            ],
        )?;
        Ok(())
    }

    pub fn load_indexed_snapshot_state(
        &self,
        snapshot_id: &str,
    ) -> SymbolStoreResult<Option<IndexedSnapshotState>> {
        let connection = self.connect()?;
        connection
            .query_row(
                r#"
                SELECT repo_id, snapshot_id, parser_config_digest, schema_version, indexed_file_count, refresh_mode
                FROM indexed_snapshot_state
                WHERE snapshot_id = ?1
                "#,
                params![snapshot_id],
                |row| {
                    Ok(IndexedSnapshotState {
                        repo_id: row.get(0)?,
                        snapshot_id: row.get(1)?,
                        parser_config_digest: row.get(2)?,
                        schema_version: row.get(3)?,
                        indexed_file_count: row.get(4)?,
                        refresh_mode: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn load_snapshot_facts(&self, snapshot_id: &str) -> SymbolStoreResult<ExtractedSnapshot> {
        let connection = self.connect()?;
        let mut files = connection.prepare(
            r#"
            SELECT path, artifact_json, facts_json
            FROM indexed_files
            WHERE snapshot_id = ?1
            ORDER BY path ASC
            "#,
        )?;
        let rows = files.query_map(params![snapshot_id], |row| {
            let path: String = row.get(0)?;
            let artifact_json: String = row.get(1)?;
            let facts_json: String = row.get(2)?;
            Ok((path, artifact_json, facts_json))
        })?;

        let mut extracted_files = Vec::new();
        for row in rows {
            let (path, artifact_json, facts_json) = row?;
            extracted_files.push(ExtractedFileFacts {
                artifact: serde_json::from_str(&artifact_json)?,
                facts: serde_json::from_str(&facts_json)?,
                symbol_facts: self.load_symbol_facts(&connection, snapshot_id, &path)?,
                import_facts: self.load_import_facts(&connection, snapshot_id, &path)?,
                export_facts: self.load_export_facts(&connection, snapshot_id, &path)?,
            });
        }

        Ok(ExtractedSnapshot {
            snapshot_id: snapshot_id.to_string(),
            files: extracted_files,
        })
    }

    pub fn status(&self) -> SymbolStoreResult<SymbolStoreStatus> {
        let connection = self.connect()?;
        let schema_version =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let indexed_snapshots = connection
            .query_row("SELECT COUNT(*) FROM scaffold_snapshots", [], |row| {
                row.get(0)
            })
            .optional()?
            .unwrap_or(0usize);
        Ok(SymbolStoreStatus {
            db_path: self.db_path.display().to_string(),
            schema_version,
            indexed_snapshots,
        })
    }

    pub fn quick_check(&self) -> SymbolStoreResult<Vec<String>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare("PRAGMA quick_check(1)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn snapshot_storage_stats(
        &self,
        snapshot_id: &str,
    ) -> SymbolStoreResult<SnapshotStorageStats> {
        let connection = self.connect()?;
        Ok(SnapshotStorageStats {
            indexed_file_count: count_snapshot_rows(&connection, "indexed_files", snapshot_id)?,
            symbol_fact_count: count_snapshot_rows(&connection, "symbol_facts", snapshot_id)?,
            occurrence_count: count_snapshot_rows(&connection, "symbol_occurrences", snapshot_id)?,
            edge_count: count_snapshot_rows(&connection, "graph_edges", snapshot_id)?,
            import_fact_count: count_snapshot_rows(&connection, "import_facts", snapshot_id)?,
            export_fact_count: count_snapshot_rows(&connection, "export_facts", snapshot_id)?,
        })
    }

    fn migrate(&self) -> SymbolStoreResult<()> {
        let connection = self.connect()?;
        connection.execute_batch(SYMBOL_STORE_MIGRATIONS)?;
        validate_schema(&connection)?;
        connection.pragma_update(None, "user_version", SYMBOL_STORE_SCHEMA_VERSION)?;
        Ok(())
    }

    fn connect(&self) -> SymbolStoreResult<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }

    fn delete_snapshot_rows(
        transaction: &rusqlite::Transaction<'_>,
        snapshot_id: &str,
    ) -> SymbolStoreResult<()> {
        for table in [
            "indexed_files",
            "symbol_facts",
            "symbol_occurrences",
            "graph_edges",
            "import_facts",
            "export_facts",
        ] {
            transaction.execute(
                &format!("DELETE FROM {table} WHERE snapshot_id = ?1"),
                params![snapshot_id],
            )?;
        }
        Ok(())
    }

    fn insert_snapshot_files(
        transaction: &rusqlite::Transaction<'_>,
        snapshot_id: &str,
        files: &[ExtractedFileFacts],
    ) -> SymbolStoreResult<()> {
        for file in files {
            transaction.execute(
                r#"
                INSERT INTO indexed_files (snapshot_id, path, artifact_json, facts_json)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![
                    snapshot_id,
                    file.facts.path,
                    serde_json::to_string(&file.artifact)?,
                    serde_json::to_string(&file.facts)?,
                ],
            )?;
            for symbol_fact in &file.symbol_facts {
                transaction.execute(
                    r#"
                    INSERT INTO symbol_facts (
                        snapshot_id,
                        path,
                        symbol_id,
                        display_name,
                        kind,
                        container_symbol_id,
                        visibility,
                        signature_digest,
                        file_path,
                        record_json
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                    "#,
                    params![
                        snapshot_id,
                        file.facts.path,
                        symbol_fact.symbol.symbol_id.0,
                        symbol_fact.symbol.display_name,
                        format!("{:?}", symbol_fact.symbol.kind),
                        symbol_fact.container.as_ref().map(|value| value.0.clone()),
                        format!("{:?}", symbol_fact.visibility),
                        symbol_fact.signature_digest,
                        symbol_fact.file_path,
                        serde_json::to_string(symbol_fact)?,
                    ],
                )?;
            }
            for occurrence in &file.facts.occurrences {
                transaction.execute(
                    r#"
                    INSERT INTO symbol_occurrences (
                        snapshot_id,
                        occurrence_id,
                        symbol_id,
                        path,
                        role,
                        record_json
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    "#,
                    params![
                        snapshot_id,
                        occurrence.occurrence_id.0,
                        occurrence.symbol_id.0,
                        occurrence.path,
                        format!("{:?}", occurrence.role),
                        serde_json::to_string(occurrence)?,
                    ],
                )?;
            }
            for edge in &file.facts.edges {
                transaction.execute(
                    r#"
                    INSERT INTO graph_edges (snapshot_id, edge_id, kind, record_json)
                    VALUES (?1, ?2, ?3, ?4)
                    "#,
                    params![
                        snapshot_id,
                        edge.edge_id,
                        format!("{:?}", edge.kind),
                        serde_json::to_string(edge)?,
                    ],
                )?;
            }
            for import_fact in &file.import_facts {
                transaction.execute(
                    r#"
                    INSERT INTO import_facts (
                        snapshot_id,
                        path,
                        symbol_id,
                        local_name,
                        module_specifier,
                        record_json
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    "#,
                    params![
                        snapshot_id,
                        import_fact.path,
                        import_fact.symbol_id.0,
                        import_fact.local_name,
                        import_fact.module_specifier,
                        serde_json::to_string(import_fact)?,
                    ],
                )?;
            }
            for export_fact in &file.export_facts {
                transaction.execute(
                    r#"
                    INSERT INTO export_facts (
                        snapshot_id,
                        path,
                        symbol_id,
                        exported_name,
                        module_specifier,
                        record_json
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    "#,
                    params![
                        snapshot_id,
                        export_fact.path,
                        export_fact.symbol_id.0,
                        export_fact.exported_name,
                        export_fact.module_specifier.clone().unwrap_or_default(),
                        serde_json::to_string(export_fact)?,
                    ],
                )?;
            }
        }
        Ok(())
    }

    fn load_symbol_facts(
        &self,
        connection: &Connection,
        snapshot_id: &str,
        path: &str,
    ) -> SymbolStoreResult<Vec<SymbolFactRecord>> {
        let mut statement = connection.prepare(
            r#"
            SELECT record_json
            FROM symbol_facts
            WHERE snapshot_id = ?1 AND path = ?2
            ORDER BY rowid ASC
            "#,
        )?;
        let rows =
            statement.query_map(params![snapshot_id, path], |row| row.get::<_, String>(0))?;
        let mut values = Vec::new();
        for row in rows {
            values.push(serde_json::from_str(&row?)?);
        }
        Ok(values)
    }

    fn load_import_facts(
        &self,
        connection: &Connection,
        snapshot_id: &str,
        path: &str,
    ) -> SymbolStoreResult<Vec<ImportFactRecord>> {
        let mut statement = connection.prepare(
            r#"
            SELECT record_json
            FROM import_facts
            WHERE snapshot_id = ?1 AND path = ?2
            ORDER BY rowid ASC
            "#,
        )?;
        let rows =
            statement.query_map(params![snapshot_id, path], |row| row.get::<_, String>(0))?;
        let mut values = Vec::new();
        for row in rows {
            values.push(serde_json::from_str(&row?)?);
        }
        Ok(values)
    }

    fn load_export_facts(
        &self,
        connection: &Connection,
        snapshot_id: &str,
        path: &str,
    ) -> SymbolStoreResult<Vec<ExportFactRecord>> {
        let mut statement = connection.prepare(
            r#"
            SELECT record_json
            FROM export_facts
            WHERE snapshot_id = ?1 AND path = ?2
            ORDER BY rowid ASC
            "#,
        )?;
        let rows =
            statement.query_map(params![snapshot_id, path], |row| row.get::<_, String>(0))?;
        let mut values = Vec::new();
        for row in rows {
            values.push(serde_json::from_str(&row?)?);
        }
        Ok(values)
    }
}

fn count_snapshot_rows(
    connection: &Connection,
    table: &str,
    snapshot_id: &str,
) -> SymbolStoreResult<usize> {
    connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE snapshot_id = ?1"),
            params![snapshot_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn validate_schema(connection: &Connection) -> SymbolStoreResult<()> {
    ensure_table_columns(
        connection,
        "indexed_files",
        &["snapshot_id", "path", "artifact_json", "facts_json"],
    )?;
    ensure_table_columns(
        connection,
        "indexed_snapshot_state",
        &[
            "snapshot_id",
            "repo_id",
            "parser_config_digest",
            "schema_version",
            "indexed_file_count",
            "refresh_mode",
            "indexed_at",
        ],
    )?;
    Ok(())
}

fn ensure_table_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> SymbolStoreResult<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = std::collections::BTreeSet::new();
    for row in rows {
        columns.insert(row?);
    }
    for column in required_columns {
        if !columns.contains(*column) {
            return Err(rusqlite::Error::InvalidColumnName(format!("{table}.{column}")).into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hyperindex_parser::ParseCore;
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, SnapshotFile, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use hyperindex_symbols::{FactWorkspace, FactsBatch, SymbolGraphBuilder};
    use tempfile::tempdir;

    use super::{ExtractedSnapshot, SymbolStore};

    fn extract_batch(path: &str, contents: &str) -> FactsBatch {
        let snapshot = ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: "snap-1".to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "abc123".to_string(),
                digest: "base".to_string(),
                file_count: 1,
                files: vec![SnapshotFile {
                    path: path.to_string(),
                    content_sha256: format!("sha-{path}"),
                    content_bytes: contents.len(),
                    contents: contents.to_string(),
                }],
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        };
        let mut parser = ParseCore::default();
        let artifact = parser
            .parse_file_from_snapshot(&snapshot, path)
            .unwrap()
            .unwrap();
        FactWorkspace.extract("repo-1", "snap-1", &[artifact])
    }

    fn snapshot(files: Vec<(&str, &str)>) -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: "snap-1".to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "abc123".to_string(),
                digest: "base".to_string(),
                file_count: files.len(),
                files: files
                    .into_iter()
                    .map(|(path, contents)| SnapshotFile {
                        path: path.to_string(),
                        content_sha256: format!("sha-{path}"),
                        content_bytes: contents.len(),
                        contents: contents.to_string(),
                    })
                    .collect(),
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    #[test]
    fn store_bootstraps_sqlite_and_tracks_scaffold_snapshots() {
        let tempdir = tempdir().unwrap();
        let store = SymbolStore::open(tempdir.path(), "repo-1").unwrap();
        let recorded = store
            .record_scaffold_snapshot("repo-1", "snap-1", 3)
            .unwrap();
        let status = store.status().unwrap();

        assert_eq!(recorded.planned_file_count, 3);
        assert_eq!(status.schema_version, 3);
        assert_eq!(status.indexed_snapshots, 1);
        assert!(store.db_path().exists());
    }

    #[test]
    fn persists_and_reloads_extracted_file_and_symbol_facts() {
        let tempdir = tempdir().unwrap();
        let store = SymbolStore::open(tempdir.path(), "repo-1").unwrap();
        let batch = extract_batch(
            "src/module.ts",
            include_str!("../../hyperindex-parser/tests/fixtures/valid/module.ts"),
        );

        store.persist_facts("repo-1", "snap-1", &batch).unwrap();
        let loaded = store.load_snapshot_facts("snap-1").unwrap();
        let status = store.status().unwrap();

        assert_eq!(
            loaded,
            ExtractedSnapshot {
                snapshot_id: "snap-1".to_string(),
                files: batch.files.clone(),
            }
        );
        assert_eq!(status.schema_version, 3);
        assert_eq!(status.indexed_snapshots, 1);
    }

    #[test]
    fn quick_check_and_snapshot_stats_report_expected_counts() {
        let tempdir = tempdir().unwrap();
        let store = SymbolStore::open(tempdir.path(), "repo-1").unwrap();
        let batch = extract_batch(
            "src/module.ts",
            include_str!("../../hyperindex-parser/tests/fixtures/valid/module.ts"),
        );

        store.persist_facts("repo-1", "snap-1", &batch).unwrap();
        let quick_check = store.quick_check().unwrap();
        let stats = store.snapshot_storage_stats("snap-1").unwrap();

        assert_eq!(quick_check, vec!["ok".to_string()]);
        assert_eq!(stats.indexed_file_count, 1);
        assert_eq!(stats.symbol_fact_count, batch.files[0].symbol_facts.len());
        assert_eq!(
            stats.occurrence_count,
            batch.files[0].facts.occurrences.len()
        );
        assert_eq!(stats.edge_count, batch.files[0].facts.edges.len());
    }

    #[test]
    fn graph_contents_are_deterministic_after_store_reload() {
        let tempdir = tempdir().unwrap();
        let store = SymbolStore::open(tempdir.path(), "repo-1").unwrap();
        let snapshot = snapshot(vec![
            (
                "src/lib.ts",
                r#"
                export function createSession() {
                  return 1;
                }
                "#,
            ),
            (
                "src/main.ts",
                r#"
                import { createSession } from "./lib";
                export function run() {
                  return createSession();
                }
                "#,
            ),
        ]);
        let mut parser = ParseCore::default();
        let plan = parser.parse_snapshot(&snapshot).unwrap();
        let batch = FactWorkspace.extract("repo-1", "snap-1", &plan.artifacts);
        let builder = SymbolGraphBuilder::default();
        let original = builder.build_with_snapshot(&batch, &snapshot);

        store.persist_facts("repo-1", "snap-1", &batch).unwrap();
        let loaded = store.load_snapshot_facts("snap-1").unwrap();
        let reloaded = builder.build_with_snapshot(
            &FactsBatch {
                files: loaded.files.clone(),
            },
            &snapshot,
        );

        assert_eq!(reloaded, original);
    }
}
