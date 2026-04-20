use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::config::RuntimeConfig;
use hyperindex_protocol::repo::RepoRecord;
use hyperindex_protocol::snapshot::SnapshotManifest;
use rusqlite::Connection;

use crate::migrations::apply_migrations;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreSummary {
    pub repo_count: usize,
    pub manifest_count: usize,
    pub event_count: usize,
    pub job_count: usize,
}

pub struct RepoStore {
    connection: Connection,
    pub sqlite_path: PathBuf,
    pub manifest_root: PathBuf,
    _temp_root: Option<tempfile::TempDir>,
}

impl RepoStore {
    pub fn open(root_dir: impl Into<PathBuf>) -> HyperindexResult<Self> {
        let root_dir = root_dir.into();
        Self::open_with_paths(root_dir.join("runtime.sqlite3"), root_dir.join("manifests"))
    }

    pub fn open_from_config(config: &RuntimeConfig) -> HyperindexResult<Self> {
        Self::open_with_paths(
            config.repo_registry.sqlite_path.clone(),
            config.repo_registry.manifests_dir.clone(),
        )
    }

    pub fn open_with_paths(
        sqlite_path: impl Into<PathBuf>,
        manifest_root: impl Into<PathBuf>,
    ) -> HyperindexResult<Self> {
        let sqlite_path = sqlite_path.into();
        let manifest_root = manifest_root.into();
        ensure_store_dirs(&sqlite_path, &manifest_root)?;
        let connection = open_resilient_connection(&sqlite_path)?;
        let store = Self {
            connection,
            sqlite_path,
            manifest_root,
            _temp_root: None,
        };
        store.restore_repo_registry_backup_if_empty()?;
        store.reindex_manifests_from_disk()?;
        Ok(store)
    }

    pub fn open_in_memory() -> HyperindexResult<Self> {
        let temp_root = tempfile::tempdir().map_err(|error| {
            HyperindexError::Message(format!("failed to create temp store root: {error}"))
        })?;
        let connection = Connection::open_in_memory()
            .map_err(|error| HyperindexError::Message(format!("sqlite open failed: {error}")))?;
        apply_migrations(&connection)?;
        let sqlite_path = temp_root.path().join("runtime.sqlite3");
        let manifest_root = temp_root.path().join("manifests");
        fs::create_dir_all(&manifest_root).map_err(|error| {
            HyperindexError::Message(format!(
                "failed to create manifest dir {}: {error}",
                manifest_root.display()
            ))
        })?;
        Ok(Self {
            connection,
            sqlite_path,
            manifest_root,
            _temp_root: Some(temp_root),
        })
    }

    pub fn manifest_dir(&self) -> PathBuf {
        self.manifest_root.clone()
    }

    pub fn summary(&self) -> HyperindexResult<StoreSummary> {
        Ok(StoreSummary {
            repo_count: count_rows(&self.connection, "repos")?,
            manifest_count: count_rows(&self.connection, "snapshot_manifests")?,
            event_count: count_rows(&self.connection, "watch_events")?,
            job_count: count_rows(&self.connection, "scheduler_jobs")?,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn repo_registry_backup_path(&self) -> PathBuf {
        self.sqlite_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("repo-registry-backup.json")
    }

    pub fn sync_repo_registry_backup(&self) -> HyperindexResult<()> {
        let repos = self.list_repos()?;
        let backup_path = self.repo_registry_backup_path();
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                HyperindexError::Message(format!(
                    "failed to create backup dir {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let temp_path = backup_path.with_extension("json.tmp");
        let raw = serde_json::to_string_pretty(&repos).map_err(|error| {
            HyperindexError::Message(format!("repo registry backup encode failed: {error}"))
        })?;
        fs::write(&temp_path, raw).map_err(|error| {
            HyperindexError::Message(format!(
                "failed to write repo registry backup {}: {error}",
                temp_path.display()
            ))
        })?;
        fs::rename(&temp_path, &backup_path).map_err(|error| {
            HyperindexError::Message(format!(
                "failed to move repo registry backup {} to {}: {error}",
                temp_path.display(),
                backup_path.display()
            ))
        })?;
        Ok(())
    }

    pub fn restore_repo_registry_backup_if_empty(&self) -> HyperindexResult<()> {
        if count_rows(&self.connection, "repos")? > 0 {
            return Ok(());
        }

        let backup_path = self.repo_registry_backup_path();
        if !backup_path.exists() {
            return Ok(());
        }

        let raw = fs::read_to_string(&backup_path).map_err(|error| {
            HyperindexError::Message(format!(
                "failed to read repo registry backup {}: {error}",
                backup_path.display()
            ))
        })?;
        let repos: Vec<RepoRecord> = serde_json::from_str(&raw).map_err(|error| {
            HyperindexError::Message(format!(
                "failed to decode repo registry backup {}: {error}",
                backup_path.display()
            ))
        })?;
        for repo in repos {
            self.connection()
                .execute(
                    "
                    INSERT OR REPLACE INTO repos (
                      repo_id,
                      repo_root,
                      display_name,
                      created_at,
                      updated_at,
                      branch,
                      head_commit,
                      is_dirty,
                      last_snapshot_id,
                      notes_json,
                      warnings_json,
                      ignore_patterns_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                    ",
                    rusqlite::params![
                        repo.repo_id,
                        repo.repo_root,
                        repo.display_name,
                        repo.created_at,
                        repo.updated_at,
                        repo.branch,
                        repo.head_commit,
                        if repo.is_dirty { 1_i64 } else { 0_i64 },
                        repo.last_snapshot_id,
                        serde_json::to_string(&repo.notes).map_err(|error| {
                            HyperindexError::Message(format!(
                                "repo registry backup notes encode failed: {error}"
                            ))
                        })?,
                        serde_json::to_string(&repo.warnings).map_err(|error| {
                            HyperindexError::Message(format!(
                                "repo registry backup warnings encode failed: {error}"
                            ))
                        })?,
                        serde_json::to_string(&repo.ignore_settings.patterns).map_err(|error| {
                            HyperindexError::Message(format!(
                                "repo registry backup ignore encode failed: {error}"
                            ))
                        })?,
                    ],
                )
                .map_err(|error| {
                    HyperindexError::Message(format!(
                        "failed to restore repo {} from backup: {error}",
                        repo.repo_id
                    ))
                })?;
        }
        Ok(())
    }

    pub fn reindex_manifests_from_disk(&self) -> HyperindexResult<usize> {
        let mut manifests = Vec::new();
        collect_manifest_paths(&self.manifest_root, &mut manifests)?;
        manifests.sort();

        let mut recovered = 0;
        for path in manifests {
            let raw = match fs::read_to_string(&path) {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let manifest: SnapshotManifest = match serde_json::from_str(&raw) {
                Ok(manifest) => manifest,
                Err(_) => continue,
            };

            self.ensure_repo_row_for_manifest(&manifest)?;
            let changed = self
                .connection()
                .execute(
                    "
                    INSERT INTO snapshot_manifests (snapshot_id, repo_id, manifest_path)
                    VALUES (?1, ?2, ?3)
                    ON CONFLICT(snapshot_id) DO UPDATE SET
                      repo_id = excluded.repo_id,
                      manifest_path = excluded.manifest_path
                    ",
                    rusqlite::params![
                        manifest.snapshot_id,
                        manifest.repo_id,
                        path.display().to_string()
                    ],
                )
                .map_err(|error| {
                    HyperindexError::Message(format!(
                        "manifest reindex failed for {}: {error}",
                        path.display()
                    ))
                })?;
            if changed > 0 {
                recovered += 1;
            }
        }

        Ok(recovered)
    }

    fn ensure_repo_row_for_manifest(&self, manifest: &SnapshotManifest) -> HyperindexResult<()> {
        self.connection()
            .execute(
                "
                INSERT INTO repos (
                  repo_id,
                  repo_root,
                  display_name,
                  branch,
                  head_commit,
                  is_dirty,
                  last_snapshot_id,
                  notes_json,
                  warnings_json,
                  ignore_patterns_json
                ) VALUES (?1, ?2, ?3, NULL, ?4, 0, ?5, '[]', '[]', '[]')
                ON CONFLICT(repo_id) DO UPDATE SET
                  repo_root = excluded.repo_root,
                  display_name = CASE
                    WHEN repos.display_name = '' THEN excluded.display_name
                    ELSE repos.display_name
                  END,
                  head_commit = COALESCE(repos.head_commit, excluded.head_commit),
                  last_snapshot_id = COALESCE(repos.last_snapshot_id, excluded.last_snapshot_id)
                ",
                rusqlite::params![
                    manifest.repo_id,
                    manifest.repo_root,
                    default_display_name(&manifest.repo_root),
                    manifest.base.commit,
                    manifest.snapshot_id,
                ],
            )
            .map_err(|error| {
                HyperindexError::Message(format!(
                    "failed to restore repo {} from manifest metadata: {error}",
                    manifest.repo_id
                ))
            })?;
        Ok(())
    }
}

fn count_rows(connection: &Connection, table: &str) -> HyperindexResult<usize> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| HyperindexError::Message(format!("prepare failed: {error}")))?;
    let count: i64 = statement
        .query_row([], |row| row.get(0))
        .map_err(|error| HyperindexError::Message(format!("count query failed: {error}")))?;
    Ok(count as usize)
}

fn ensure_store_dirs(sqlite_path: &Path, manifest_root: &Path) -> HyperindexResult<()> {
    if let Some(parent) = sqlite_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            HyperindexError::Message(format!(
                "failed to create sqlite parent {}: {error}",
                parent.display()
            ))
        })?;
    }
    fs::create_dir_all(manifest_root).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to create manifest dir {}: {error}",
            manifest_root.display()
        ))
    })?;
    Ok(())
}

fn open_resilient_connection(sqlite_path: &Path) -> HyperindexResult<Connection> {
    match open_verified_connection(sqlite_path) {
        Ok(connection) => Ok(connection),
        Err(error) if sqlite_path.exists() && is_recoverable_sqlite_error(&error.to_string()) => {
            quarantine_sqlite_artifacts(sqlite_path)?;
            open_verified_connection(sqlite_path)
        }
        Err(error) => Err(error),
    }
}

fn open_verified_connection(sqlite_path: &Path) -> HyperindexResult<Connection> {
    let connection = Connection::open(sqlite_path).map_err(|error| {
        HyperindexError::Message(format!("failed to open {}: {error}", sqlite_path.display()))
    })?;
    apply_migrations(&connection)?;
    verify_sqlite_health(&connection, sqlite_path)?;
    Ok(connection)
}

fn verify_sqlite_health(connection: &Connection, sqlite_path: &Path) -> HyperindexResult<()> {
    let health: String = connection
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(|error| {
            HyperindexError::Message(format!(
                "sqlite health check failed for {}: {error}",
                sqlite_path.display()
            ))
        })?;
    if health != "ok" {
        return Err(HyperindexError::Message(format!(
            "sqlite health check failed for {}: {health}",
            sqlite_path.display()
        )));
    }
    Ok(())
}

fn quarantine_sqlite_artifacts(sqlite_path: &Path) -> HyperindexResult<()> {
    for suffix in ["", "-wal", "-shm"] {
        let path = if suffix.is_empty() {
            sqlite_path.to_path_buf()
        } else {
            PathBuf::from(format!("{}{}", sqlite_path.display(), suffix))
        };
        if !path.exists() {
            continue;
        }
        let quarantine_path = next_quarantine_path(&path)?;
        fs::rename(&path, &quarantine_path).map_err(|error| {
            HyperindexError::Message(format!(
                "failed to quarantine {} to {}: {error}",
                path.display(),
                quarantine_path.display()
            ))
        })?;
    }
    Ok(())
}

fn next_quarantine_path(path: &Path) -> HyperindexResult<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| {
            HyperindexError::Message(format!("invalid sqlite artifact path: {}", path.display()))
        })?
        .to_os_string();
    for ordinal in 1..=1024 {
        let mut candidate_name = OsString::from(&file_name);
        candidate_name.push(format!(".corrupt-{ordinal}"));
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(HyperindexError::Message(format!(
        "failed to allocate quarantine path for {}",
        path.display()
    )))
}

fn is_recoverable_sqlite_error(message: &str) -> bool {
    let lowered = message.to_lowercase();
    lowered.contains("malformed")
        || lowered.contains("not a database")
        || lowered.contains("sqlite health check failed")
        || lowered.contains("database disk image is malformed")
}

fn collect_manifest_paths(root: &Path, collected: &mut Vec<PathBuf>) -> HyperindexResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to read manifest dir {}: {error}",
            root.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            HyperindexError::Message(format!("failed to read manifest dir entry: {error}"))
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_manifest_paths(&path, collected)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            collected.push(path);
        }
    }
    Ok(())
}

fn default_display_name(repo_root: &str) -> String {
    Path::new(repo_root)
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| repo_root.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, WorkingTreeOverlay,
    };

    use super::RepoStore;

    #[test]
    fn in_memory_store_initializes_empty_schema() {
        let store = RepoStore::open_in_memory().unwrap();
        let summary = store.summary().unwrap();
        assert_eq!(summary.repo_count, 0);
        assert_eq!(summary.manifest_count, 0);
        assert_eq!(summary.event_count, 0);
        assert_eq!(summary.job_count, 0);
    }

    #[test]
    fn corrupt_sqlite_is_quarantined_and_manifests_are_reindexed() {
        let tempdir = tempfile::tempdir().unwrap();
        let sqlite_path = tempdir.path().join("state/runtime.sqlite3");
        let manifest_root = tempdir.path().join("data/manifests");
        fs::create_dir_all(sqlite_path.parent().unwrap()).unwrap();
        fs::create_dir_all(manifest_root.join("repo-1")).unwrap();
        fs::write(&sqlite_path, "definitely not sqlite").unwrap();
        fs::write(
            manifest_root.join("repo-1/snap-1.json"),
            serde_json::to_string_pretty(&ComposedSnapshot {
                version: 1,
                protocol_version: "repo-hyperindex.local/v1".to_string(),
                snapshot_id: "snap-1".to_string(),
                repo_id: "repo-1".to_string(),
                repo_root: tempdir.path().join("repo").display().to_string(),
                base: BaseSnapshot {
                    kind: BaseSnapshotKind::GitCommit,
                    commit: "abc123".to_string(),
                    digest: "base-1".to_string(),
                    file_count: 0,
                    files: Vec::new(),
                },
                working_tree: WorkingTreeOverlay {
                    digest: "work-1".to_string(),
                    entries: Vec::new(),
                },
                buffers: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();

        let store = RepoStore::open_with_paths(&sqlite_path, &manifest_root).unwrap();
        let summary = store.summary().unwrap();
        assert_eq!(summary.repo_count, 1);
        assert_eq!(summary.manifest_count, 1);

        let parent = sqlite_path.parent().unwrap();
        let quarantined = fs::read_dir(parent)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .any(|name| name.starts_with("runtime.sqlite3.corrupt-"));
        assert!(quarantined);
    }
}
