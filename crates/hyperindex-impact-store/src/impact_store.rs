use std::fs;
use std::path::{Path, PathBuf};

use hyperindex_impact::ImpactMaterializedState;
use hyperindex_protocol::impact::ImpactRefreshStats;
use rusqlite::{Connection, OptionalExtension, params};
use tracing::info;

use crate::{
    IMPACT_STORE_SCHEMA_VERSION, ImpactStoreResult, default_store_path, planned_migrations,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactBuildManifest {
    pub repo_id: String,
    pub snapshot_id: String,
    pub symbol_build_id: Option<String>,
    pub store_path: PathBuf,
    pub schema_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredImpactBuild {
    pub repo_id: String,
    pub snapshot_id: String,
    pub impact_config_digest: String,
    pub schema_version: u32,
    pub symbol_build_id: Option<String>,
    pub created_at: String,
    pub refresh_stats: ImpactRefreshStats,
    pub refresh_mode: String,
    pub fallback_reason: Option<String>,
    pub state: ImpactMaterializedState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactStoreStatus {
    pub db_path: String,
    pub schema_version: u32,
    pub build_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactStore {
    pub store_path: PathBuf,
    pub schema_version: u32,
}

impl ImpactStore {
    pub fn scaffold(runtime_root: &Path, repo_id: &str) -> ImpactStoreResult<Self> {
        let store_path = default_store_path(runtime_root, repo_id);
        Self::open_at_path(store_path)
    }

    pub fn open_in_store_dir(store_dir: &Path, repo_id: &str) -> ImpactStoreResult<Self> {
        Self::open_at_path(store_dir.join(repo_id).join("impact.sqlite3"))
    }

    pub fn open_at_path(store_path: PathBuf) -> ImpactStoreResult<Self> {
        if let Some(parent) = store_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let store = Self {
            store_path,
            schema_version: IMPACT_STORE_SCHEMA_VERSION,
        };
        store.migrate()?;
        info!(path = %store.store_path.display(), "opened phase5 impact store");
        Ok(store)
    }

    pub fn manifest_for(
        &self,
        repo_id: &str,
        snapshot_id: &str,
        symbol_build_id: Option<&str>,
    ) -> ImpactBuildManifest {
        ImpactBuildManifest {
            repo_id: repo_id.to_string(),
            snapshot_id: snapshot_id.to_string(),
            symbol_build_id: symbol_build_id.map(str::to_string),
            store_path: self.store_path.clone(),
            schema_version: self.schema_version,
        }
    }

    pub fn persist_build(&self, build: &StoredImpactBuild) -> ImpactStoreResult<()> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            INSERT INTO impact_builds (
                snapshot_id,
                repo_id,
                impact_config_digest,
                schema_version,
                symbol_build_id,
                created_at,
                refresh_mode,
                fallback_reason,
                build_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(snapshot_id) DO UPDATE SET
                repo_id = excluded.repo_id,
                impact_config_digest = excluded.impact_config_digest,
                schema_version = excluded.schema_version,
                symbol_build_id = excluded.symbol_build_id,
                created_at = excluded.created_at,
                refresh_mode = excluded.refresh_mode,
                fallback_reason = excluded.fallback_reason,
                build_json = excluded.build_json
            "#,
            params![
                build.snapshot_id,
                build.repo_id,
                build.impact_config_digest,
                build.schema_version,
                build.symbol_build_id,
                build.created_at,
                build.refresh_mode,
                build.fallback_reason,
                serde_json::to_string(build)?,
            ],
        )?;
        Ok(())
    }

    pub fn load_build(&self, snapshot_id: &str) -> ImpactStoreResult<Option<StoredImpactBuild>> {
        let connection = self.connect()?;
        connection
            .query_row(
                "SELECT build_json FROM impact_builds WHERE snapshot_id = ?1",
                [snapshot_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(Into::into)
    }

    pub fn list_builds(&self) -> ImpactStoreResult<Vec<StoredImpactBuild>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            r#"
            SELECT build_json
            FROM impact_builds
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

    pub fn status(&self) -> ImpactStoreResult<ImpactStoreStatus> {
        let connection = self.connect()?;
        let schema_version =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let build_count = connection
            .query_row("SELECT COUNT(*) FROM impact_builds", [], |row| row.get(0))
            .optional()?
            .unwrap_or(0usize);
        Ok(ImpactStoreStatus {
            db_path: self.store_path.display().to_string(),
            schema_version,
            build_count,
        })
    }

    pub fn quick_check(&self) -> ImpactStoreResult<Vec<String>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare("PRAGMA quick_check(1)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    fn migrate(&self) -> ImpactStoreResult<()> {
        let connection = self.connect()?;
        let current_version =
            connection.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?;
        for migration in planned_migrations() {
            if migration.schema_version <= current_version {
                continue;
            }
            if migration.schema_version == 2 {
                connection.execute_batch(
                    r#"
                    CREATE TABLE IF NOT EXISTS impact_builds (
                        snapshot_id TEXT PRIMARY KEY,
                        repo_id TEXT NOT NULL,
                        impact_config_digest TEXT NOT NULL,
                        schema_version INTEGER NOT NULL,
                        symbol_build_id TEXT,
                        created_at TEXT NOT NULL,
                        refresh_mode TEXT NOT NULL,
                        fallback_reason TEXT,
                        build_json TEXT NOT NULL
                    );
                    "#,
                )?;
            }
            connection.pragma_update(None, "user_version", migration.schema_version)?;
        }
        validate_schema(&connection)?;
        Ok(())
    }

    fn connect(&self) -> ImpactStoreResult<Connection> {
        Ok(Connection::open(&self.store_path)?)
    }
}

fn validate_schema(connection: &Connection) -> ImpactStoreResult<()> {
    let mut statement = connection.prepare("PRAGMA table_info(impact_builds)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = std::collections::BTreeSet::new();
    for row in rows {
        columns.insert(row?);
    }
    for column in [
        "snapshot_id",
        "repo_id",
        "impact_config_digest",
        "schema_version",
        "symbol_build_id",
        "created_at",
        "refresh_mode",
        "fallback_reason",
        "build_json",
    ] {
        if !columns.contains(column) {
            return Err(
                rusqlite::Error::InvalidColumnName(format!("impact_builds.{column}")).into(),
            );
        }
    }
    Ok(())
}
