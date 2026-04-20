use hyperindex_core::{HyperindexError, HyperindexResult};
use rusqlite::Connection;

pub fn apply_migrations(connection: &Connection) -> HyperindexResult<()> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS repos (
              repo_id TEXT PRIMARY KEY,
              repo_root TEXT NOT NULL,
              display_name TEXT NOT NULL DEFAULT '',
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              branch TEXT,
              head_commit TEXT,
              is_dirty INTEGER NOT NULL DEFAULT 0,
              last_snapshot_id TEXT,
              notes_json TEXT NOT NULL DEFAULT '[]',
              warnings_json TEXT NOT NULL DEFAULT '[]',
              ignore_patterns_json TEXT NOT NULL DEFAULT '[]'
            );

            CREATE TABLE IF NOT EXISTS snapshot_manifests (
              snapshot_id TEXT PRIMARY KEY,
              repo_id TEXT NOT NULL,
              manifest_path TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS buffers (
              repo_id TEXT NOT NULL,
              buffer_id TEXT NOT NULL,
              path TEXT NOT NULL,
              version INTEGER NOT NULL,
              language TEXT,
              content_sha256 TEXT NOT NULL,
              content_bytes INTEGER NOT NULL,
              contents TEXT NOT NULL,
              updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repo_id, buffer_id)
            );

            CREATE TABLE IF NOT EXISTS watch_events (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              repo_id TEXT NOT NULL,
              sequence_id INTEGER NOT NULL,
              kind TEXT NOT NULL,
              path TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS scheduler_jobs (
              job_id TEXT PRIMARY KEY,
              repo_id TEXT,
              kind TEXT NOT NULL,
              state TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )
        .map_err(|error| HyperindexError::Message(format!("migration failed: {error}")))?;

    ensure_repo_column(connection, "display_name", "TEXT NOT NULL DEFAULT ''")?;
    ensure_repo_column(connection, "created_at", "TEXT NOT NULL DEFAULT ''")?;
    ensure_repo_column(connection, "updated_at", "TEXT NOT NULL DEFAULT ''")?;
    ensure_repo_column(connection, "notes_json", "TEXT NOT NULL DEFAULT '[]'")?;
    ensure_repo_column(connection, "warnings_json", "TEXT NOT NULL DEFAULT '[]'")?;
    ensure_repo_column(
        connection,
        "ignore_patterns_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;

    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS repos_repo_root_unique ON repos(repo_root)",
            [],
        )
        .map_err(|error| HyperindexError::Message(format!("index creation failed: {error}")))?;
    connection
        .execute(
            "UPDATE repos SET created_at = CURRENT_TIMESTAMP WHERE created_at = ''",
            [],
        )
        .map_err(|error| {
            HyperindexError::Message(format!("created_at backfill failed: {error}"))
        })?;
    connection
        .execute(
            "UPDATE repos SET updated_at = CURRENT_TIMESTAMP WHERE updated_at = ''",
            [],
        )
        .map_err(|error| {
            HyperindexError::Message(format!("updated_at backfill failed: {error}"))
        })?;

    Ok(())
}

fn ensure_repo_column(
    connection: &Connection,
    column_name: &str,
    definition: &str,
) -> HyperindexResult<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(repos)")
        .map_err(|error| HyperindexError::Message(format!("pragma table_info failed: {error}")))?;
    let mut rows = statement
        .query([])
        .map_err(|error| HyperindexError::Message(format!("pragma query failed: {error}")))?;
    let mut exists = false;
    while let Some(row) = rows
        .next()
        .map_err(|error| HyperindexError::Message(format!("pragma row read failed: {error}")))?
    {
        let name: String = row
            .get(1)
            .map_err(|error| HyperindexError::Message(format!("pragma decode failed: {error}")))?;
        if name == column_name {
            exists = true;
            break;
        }
    }

    if !exists {
        let sql = format!("ALTER TABLE repos ADD COLUMN {column_name} {definition}");
        connection.execute(&sql, []).map_err(|error| {
            HyperindexError::Message(format!("failed to add repos.{column_name}: {error}"))
        })?;
    }

    Ok(())
}
