use std::fs;
use std::path::Path;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_git_state::GitInspector;
use hyperindex_protocol::repo::{
    RepoIgnoreSettings, RepoRecord, ReposAddParams, ReposRemoveParams, ReposRemoveResponse,
};
use rusqlite::{OptionalExtension, params};

use crate::db::RepoStore;

impl RepoStore {
    pub fn add_repo(&self, params: &ReposAddParams) -> HyperindexResult<RepoRecord> {
        let git_state = GitInspector.inspect_repo(&params.repo_root)?;
        let repo_root = canonical_string(&git_state.repo_root)?;
        let repo_id = repo_id_for_root(&repo_root);
        let display_name = params
            .display_name
            .clone()
            .unwrap_or_else(|| default_display_name(&git_state.repo_root));
        let notes_json = encode_json(&params.notes)?;
        let warnings_json = encode_json(&git_state.warnings)?;
        let ignore_patterns_json = encode_json(&params.ignore_patterns)?;

        let insert = self.connection().execute(
            "
            INSERT INTO repos (
              repo_id,
              repo_root,
              display_name,
              branch,
              head_commit,
              is_dirty,
              notes_json,
              warnings_json,
              ignore_patterns_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ",
            params![
                repo_id,
                repo_root,
                display_name,
                git_state.branch,
                git_state.head_commit,
                bool_to_sqlite(
                    !git_state.working_tree.dirty_tracked_files.is_empty()
                        || !git_state.working_tree.untracked_files.is_empty()
                        || !git_state.working_tree.deleted_files.is_empty()
                        || !git_state.working_tree.renamed_files.is_empty(),
                ),
                notes_json,
                warnings_json,
                ignore_patterns_json,
            ],
        );

        match insert {
            Ok(_) => {
                self.sync_repo_registry_backup()?;
                self.show_repo(&repo_id)
            }
            Err(error) if is_unique_constraint(&error) => {
                let existing = self.find_repo_by_root(&repo_root)?;
                let message = match existing {
                    Some(repo) => format!(
                        "repo root {} is already registered as {}",
                        repo.repo_root, repo.repo_id
                    ),
                    None => format!("repo root {repo_root} is already registered"),
                };
                Err(HyperindexError::Message(message))
            }
            Err(error) => Err(HyperindexError::Message(format!(
                "repo insert failed for {repo_root}: {error}"
            ))),
        }
    }

    pub fn list_repos(&self) -> HyperindexResult<Vec<RepoRecord>> {
        let mut statement = self
            .connection()
            .prepare(
                "
                SELECT
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
                FROM repos
                ORDER BY repo_id
                ",
            )
            .map_err(|error| HyperindexError::Message(format!("prepare failed: {error}")))?;
        let rows = statement
            .query_map([], decode_repo_row)
            .map_err(|error| HyperindexError::Message(format!("query failed: {error}")))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| HyperindexError::Message(format!("row decode failed: {error}")))
    }

    pub fn show_repo(&self, repo_id: &str) -> HyperindexResult<RepoRecord> {
        let mut statement = self
            .connection()
            .prepare(
                "
                SELECT
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
                FROM repos
                WHERE repo_id = ?1
                ",
            )
            .map_err(|error| HyperindexError::Message(format!("prepare failed: {error}")))?;

        statement
            .query_row([repo_id], decode_repo_row)
            .optional()
            .map_err(|error| HyperindexError::Message(format!("query failed: {error}")))?
            .ok_or_else(|| HyperindexError::Message(format!("repo {repo_id} was not found")))
    }

    pub fn remove_repo(&self, params: &ReposRemoveParams) -> HyperindexResult<ReposRemoveResponse> {
        let removed = self
            .connection()
            .execute("DELETE FROM repos WHERE repo_id = ?1", [&params.repo_id])
            .map_err(|error| HyperindexError::Message(format!("repo delete failed: {error}")))?
            > 0;

        if removed && params.purge_state {
            self.connection()
                .execute(
                    "DELETE FROM snapshot_manifests WHERE repo_id = ?1",
                    [&params.repo_id],
                )
                .map_err(|error| {
                    HyperindexError::Message(format!("snapshot purge failed: {error}"))
                })?;
            self.connection()
                .execute(
                    "DELETE FROM watch_events WHERE repo_id = ?1",
                    [&params.repo_id],
                )
                .map_err(|error| {
                    HyperindexError::Message(format!("event purge failed: {error}"))
                })?;
            self.connection()
                .execute(
                    "DELETE FROM scheduler_jobs WHERE repo_id = ?1",
                    [&params.repo_id],
                )
                .map_err(|error| HyperindexError::Message(format!("job purge failed: {error}")))?;
            self.connection()
                .execute("DELETE FROM buffers WHERE repo_id = ?1", [&params.repo_id])
                .map_err(|error| {
                    HyperindexError::Message(format!("buffer purge failed: {error}"))
                })?;
            purge_manifest_dir(&self.manifest_root, &params.repo_id)?;
        }

        if removed {
            self.sync_repo_registry_backup()?;
        }

        Ok(ReposRemoveResponse {
            repo_id: params.repo_id.clone(),
            removed,
            purged_state: removed && params.purge_state,
        })
    }

    fn find_repo_by_root(&self, repo_root: &str) -> HyperindexResult<Option<RepoRecord>> {
        let mut statement = self
            .connection()
            .prepare(
                "
                SELECT
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
                FROM repos
                WHERE repo_root = ?1
                ",
            )
            .map_err(|error| HyperindexError::Message(format!("prepare failed: {error}")))?;

        statement
            .query_row([repo_root], decode_repo_row)
            .optional()
            .map_err(|error| HyperindexError::Message(format!("query failed: {error}")))
    }
}

fn decode_repo_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepoRecord> {
    let notes_json: String = row.get(9)?;
    let warnings_json: String = row.get(10)?;
    let ignore_patterns_json: String = row.get(11)?;
    Ok(RepoRecord {
        repo_id: row.get(0)?,
        repo_root: row.get(1)?,
        display_name: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        branch: row.get(5)?,
        head_commit: row.get(6)?,
        is_dirty: row.get::<_, i64>(7)? != 0,
        last_snapshot_id: row.get(8)?,
        notes: decode_json(&notes_json)?,
        warnings: decode_json(&warnings_json)?,
        ignore_settings: RepoIgnoreSettings {
            patterns: decode_json(&ignore_patterns_json)?,
        },
    })
}

fn encode_json(values: &[String]) -> HyperindexResult<String> {
    serde_json::to_string(values)
        .map_err(|error| HyperindexError::Message(format!("json encode failed: {error}")))
}

fn decode_json(raw: &str) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            raw.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn bool_to_sqlite(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn canonical_string(path: &Path) -> HyperindexResult<String> {
    path_to_string(&path.canonicalize().map_err(|error| {
        HyperindexError::Message(format!(
            "failed to canonicalize {}: {error}",
            path.display()
        ))
    })?)
}

fn path_to_string(path: &Path) -> HyperindexResult<String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| HyperindexError::Message(format!("non-utf8 path: {}", path.display())))
}

fn purge_manifest_dir(manifest_root: &Path, repo_id: &str) -> HyperindexResult<()> {
    let repo_manifest_dir = manifest_root.join(repo_id);
    if !repo_manifest_dir.exists() {
        return Ok(());
    }
    fs::remove_dir_all(&repo_manifest_dir).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to purge manifest dir {}: {error}",
            repo_manifest_dir.display()
        ))
    })
}

fn default_display_name(repo_root: &Path) -> String {
    repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| repo_root.display().to_string())
}

fn repo_id_for_root(repo_root: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in repo_root.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("repo-{hash:016x}")
}

fn is_unique_constraint(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _)
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
                || code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use tempfile::tempdir;

    use super::RepoStore;
    use hyperindex_protocol::buffers::{BufferListParams, BufferSetParams};
    use hyperindex_protocol::repo::{ReposAddParams, ReposRemoveParams};
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, WorkingTreeOverlay,
    };

    #[test]
    fn add_list_show_remove_repo_roundtrips() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("demo-repo");
        init_repo(&repo_root);

        let store = RepoStore::open(tempdir.path().join("state")).unwrap();
        let added = store
            .add_repo(&ReposAddParams {
                repo_root: repo_root.display().to_string(),
                display_name: Some("Demo Repo".to_string()),
                notes: vec!["owned by search team".to_string()],
                ignore_patterns: vec!["coverage/**".to_string()],
                watch_on_add: false,
            })
            .unwrap();

        assert_eq!(added.display_name, "Demo Repo");
        assert_eq!(added.notes, vec!["owned by search team"]);
        assert_eq!(added.ignore_settings.patterns, vec!["coverage/**"]);
        assert!(added.branch.is_some());
        assert!(added.head_commit.is_some());
        assert!(!added.created_at.is_empty());
        assert_eq!(added.created_at, added.updated_at);

        let listed = store.list_repos().unwrap();
        assert_eq!(listed, vec![added.clone()]);

        let shown = store.show_repo(&added.repo_id).unwrap();
        assert_eq!(shown, added);

        let removed = store
            .remove_repo(&ReposRemoveParams {
                repo_id: added.repo_id.clone(),
                purge_state: true,
            })
            .unwrap();
        assert!(removed.removed);
        assert!(removed.purged_state);
        assert!(store.list_repos().unwrap().is_empty());
    }

    #[test]
    fn add_repo_rejects_duplicate_canonical_roots() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("demo-repo");
        let nested = repo_root.join("src");
        init_repo(&repo_root);
        fs::create_dir_all(&nested).unwrap();

        let store = RepoStore::open(tempdir.path().join("state")).unwrap();
        let first = store
            .add_repo(&ReposAddParams {
                repo_root: repo_root.display().to_string(),
                display_name: None,
                notes: Vec::new(),
                ignore_patterns: Vec::new(),
                watch_on_add: false,
            })
            .unwrap();
        let duplicate = store.add_repo(&ReposAddParams {
            repo_root: nested.display().to_string(),
            display_name: None,
            notes: Vec::new(),
            ignore_patterns: Vec::new(),
            watch_on_add: false,
        });

        let message = duplicate.unwrap_err().to_string();
        assert!(message.contains(&first.repo_id));
        assert_eq!(store.list_repos().unwrap().len(), 1);
    }

    #[test]
    fn remove_repo_with_purge_state_removes_manifests_and_buffers_from_disk() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("demo-repo");
        init_repo(&repo_root);

        let store_root = tempdir.path().join("state");
        let store = RepoStore::open(&store_root).unwrap();
        let repo = store
            .add_repo(&ReposAddParams {
                repo_root: repo_root.display().to_string(),
                display_name: Some("Demo Repo".to_string()),
                notes: Vec::new(),
                ignore_patterns: Vec::new(),
                watch_on_add: false,
            })
            .unwrap();
        store
            .set_buffer(&BufferSetParams {
                repo_id: repo.repo_id.clone(),
                buffer_id: "buffer-1".to_string(),
                path: "README.md".to_string(),
                version: 1,
                language: Some("markdown".to_string()),
                contents: "# buffered\n".to_string(),
            })
            .unwrap();
        store
            .persist_manifest(&ComposedSnapshot {
                version: 1,
                protocol_version: "repo-hyperindex.local/v1".to_string(),
                snapshot_id: "snap-1".to_string(),
                repo_id: repo.repo_id.clone(),
                repo_root: repo.repo_root.clone(),
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
            .unwrap();

        let manifest_dir = store.manifest_root.join(&repo.repo_id);
        assert!(manifest_dir.exists());

        let removed = store
            .remove_repo(&ReposRemoveParams {
                repo_id: repo.repo_id.clone(),
                purge_state: true,
            })
            .unwrap();
        assert!(removed.removed);
        assert!(removed.purged_state);
        assert!(!manifest_dir.exists());

        let reopened = RepoStore::open(&store_root).unwrap();
        assert!(reopened.list_repos().unwrap().is_empty());
        assert!(reopened.load_manifest("snap-1").unwrap().is_none());
        assert!(
            reopened
                .list_buffers(&BufferListParams {
                    repo_id: repo.repo_id.clone(),
                })
                .unwrap()
                .is_empty()
        );
    }

    fn init_repo(repo_root: &Path) {
        fs::create_dir_all(repo_root.join("src")).unwrap();
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["checkout", "-b", "trunk"]);
        fs::write(repo_root.join("README.md"), "# demo\n").unwrap();
        run_git(repo_root, &["add", "."]);
        commit_all(repo_root, "initial");
    }

    fn commit_all(repo_root: &Path, message: &str) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .arg("commit")
            .arg("-m")
            .arg(message)
            .env("GIT_AUTHOR_NAME", "Codex")
            .env("GIT_AUTHOR_EMAIL", "codex@example.com")
            .env("GIT_COMMITTER_NAME", "Codex")
            .env("GIT_COMMITTER_EMAIL", "codex@example.com")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_git(repo_root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
