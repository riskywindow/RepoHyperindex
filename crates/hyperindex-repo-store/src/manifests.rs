use std::fs;
use std::path::PathBuf;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::snapshot::{SnapshotManifest, SnapshotSummary};
use rusqlite::{OptionalExtension, params};

use crate::db::RepoStore;

impl RepoStore {
    pub fn persist_manifest(&self, manifest: &SnapshotManifest) -> HyperindexResult<()> {
        let repo_dir = self.manifest_dir().join(&manifest.repo_id);
        fs::create_dir_all(&repo_dir).map_err(|error| {
            HyperindexError::Message(format!("failed to create {}: {error}", repo_dir.display()))
        })?;
        let manifest_path = repo_dir.join(format!("{}.json", manifest.snapshot_id));
        let temp_path = repo_dir.join(format!("{}.json.tmp", manifest.snapshot_id));
        let raw = serde_json::to_string_pretty(manifest).map_err(|error| {
            HyperindexError::Message(format!("manifest serialization failed: {error}"))
        })?;
        fs::write(&temp_path, raw).map_err(|error| {
            HyperindexError::Message(format!("failed to write {}: {error}", temp_path.display()))
        })?;
        fs::rename(&temp_path, &manifest_path).map_err(|error| {
            HyperindexError::Message(format!(
                "failed to move {} to {}: {error}",
                temp_path.display(),
                manifest_path.display()
            ))
        })?;

        self.connection()
            .execute(
                "
                INSERT INTO snapshot_manifests (snapshot_id, repo_id, manifest_path)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(snapshot_id) DO UPDATE SET
                  repo_id = excluded.repo_id,
                  manifest_path = excluded.manifest_path
                ",
                params![
                    manifest.snapshot_id,
                    manifest.repo_id,
                    manifest_path.display().to_string()
                ],
            )
            .map_err(|error| {
                HyperindexError::Message(format!("manifest upsert failed: {error}"))
            })?;
        self.connection()
            .execute(
                "
                UPDATE repos
                SET last_snapshot_id = ?1, updated_at = CURRENT_TIMESTAMP
                WHERE repo_id = ?2
                ",
                params![manifest.snapshot_id, manifest.repo_id],
            )
            .map_err(|error| {
                HyperindexError::Message(format!("repo snapshot update failed: {error}"))
            })?;
        self.sync_repo_registry_backup()?;
        Ok(())
    }

    pub fn load_manifest(&self, snapshot_id: &str) -> HyperindexResult<Option<SnapshotManifest>> {
        let path = self
            .manifest_path(snapshot_id)?
            .map(PathBuf::from)
            .or_else(|| self.find_manifest_path_on_disk(snapshot_id).ok().flatten());
        match path {
            Some(path) => {
                let raw = fs::read_to_string(&path).map_err(|error| {
                    HyperindexError::Message(format!("failed to read {}: {error}", path.display()))
                })?;
                serde_json::from_str(&raw).map(Some).map_err(|error| {
                    HyperindexError::Message(format!(
                        "failed to decode manifest {}: {error}",
                        path.display()
                    ))
                })
            }
            None => Ok(None),
        }
    }

    pub fn list_manifests(
        &self,
        repo_id: &str,
        limit: usize,
    ) -> HyperindexResult<Vec<SnapshotSummary>> {
        let mut statement = self
            .connection()
            .prepare(
                "
                SELECT manifest_path
                FROM snapshot_manifests
                WHERE repo_id = ?1
                ORDER BY created_at DESC, snapshot_id DESC
                LIMIT ?2
                ",
            )
            .map_err(|error| HyperindexError::Message(format!("prepare failed: {error}")))?;
        let paths = statement
            .query_map(params![repo_id, limit as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| HyperindexError::Message(format!("query failed: {error}")))?;

        let mut summaries = Vec::new();
        for path in paths {
            let path = PathBuf::from(path.map_err(|error| {
                HyperindexError::Message(format!("row decode failed: {error}"))
            })?);
            let raw = fs::read_to_string(&path).map_err(|error| {
                HyperindexError::Message(format!("failed to read {}: {error}", path.display()))
            })?;
            let manifest: SnapshotManifest = serde_json::from_str(&raw).map_err(|error| {
                HyperindexError::Message(format!(
                    "failed to decode manifest {}: {error}",
                    path.display()
                ))
            })?;
            summaries.push(SnapshotSummary {
                snapshot_id: manifest.snapshot_id,
                repo_id: manifest.repo_id,
                base_commit: manifest.base.commit,
                working_tree_digest: manifest.working_tree.digest,
                has_working_tree: !manifest.working_tree.entries.is_empty(),
                buffer_count: manifest.buffers.len(),
            });
        }
        Ok(summaries)
    }

    fn manifest_path(&self, snapshot_id: &str) -> HyperindexResult<Option<String>> {
        self.connection()
            .prepare("SELECT manifest_path FROM snapshot_manifests WHERE snapshot_id = ?1")
            .map_err(|error| HyperindexError::Message(format!("prepare failed: {error}")))?
            .query_row([snapshot_id], |row| row.get(0))
            .optional()
            .map_err(|error| HyperindexError::Message(format!("query failed: {error}")))
    }

    fn find_manifest_path_on_disk(&self, snapshot_id: &str) -> HyperindexResult<Option<PathBuf>> {
        let mut manifests = Vec::new();
        self.collect_manifest_paths(&self.manifest_root, &mut manifests)?;
        let expected = format!("{snapshot_id}.json");
        Ok(manifests
            .into_iter()
            .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(&expected)))
    }

    fn collect_manifest_paths(
        &self,
        root: &std::path::Path,
        collected: &mut Vec<PathBuf>,
    ) -> HyperindexResult<()> {
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
                self.collect_manifest_paths(&path, collected)?;
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                collected.push(path);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, WorkingTreeOverlay,
    };

    use crate::RepoStore;

    #[test]
    fn manifest_store_roundtrips_json_manifest() {
        let store = RepoStore::open_in_memory().unwrap();
        assert_ne!(store.manifest_dir(), PathBuf::from(":memory:/manifests"));
        let manifest = ComposedSnapshot {
            version: 1,
            protocol_version: "repo-hyperindex.local/v1".to_string(),
            snapshot_id: "snap-1".to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
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
        };

        store.persist_manifest(&manifest).unwrap();
        let loaded = store.load_manifest("snap-1").unwrap().unwrap();
        assert_eq!(loaded, manifest);
        let listed = store.list_manifests("repo-1", 10).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].snapshot_id, "snap-1");
    }
}
