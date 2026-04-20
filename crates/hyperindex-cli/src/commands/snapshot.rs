use std::path::Path;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::snapshot::{
    ComposedSnapshot, SnapshotCreateParams, SnapshotDiffParams, SnapshotDiffResponse,
    SnapshotReadFileParams, SnapshotShowParams, SnapshotSummary,
};

use crate::client::DaemonClient;

pub fn create(
    config_path: Option<&Path>,
    params: &SnapshotCreateParams,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::SnapshotsCreate(params.clone()))? {
        SuccessPayload::SnapshotsCreate(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected snapshot create response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else {
        Ok(render_snapshot_summary(&snapshot_summary(
            &response.snapshot,
        )))
    }
}

pub fn show(
    config_path: Option<&Path>,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::SnapshotsShow(SnapshotShowParams {
        snapshot_id: snapshot_id.to_string(),
    }))? {
        SuccessPayload::SnapshotsShow(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected snapshot show response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else {
        Ok(render_snapshot_summary(&snapshot_summary(
            &response.snapshot,
        )))
    }
}

pub fn diff(
    config_path: Option<&Path>,
    left_snapshot_id: &str,
    right_snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::SnapshotsDiff(SnapshotDiffParams {
        left_snapshot_id: left_snapshot_id.to_string(),
        right_snapshot_id: right_snapshot_id.to_string(),
    }))? {
        SuccessPayload::SnapshotsDiff(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected snapshot diff response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else {
        Ok(render_diff(&response))
    }
}

pub fn read_file(
    config_path: Option<&Path>,
    snapshot_id: &str,
    path: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::SnapshotsReadFile(SnapshotReadFileParams {
        snapshot_id: snapshot_id.to_string(),
        path: path.to_string(),
    }))? {
        SuccessPayload::SnapshotsReadFile(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected snapshot read-file response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else {
        Ok(response.contents)
    }
}

fn snapshot_summary(snapshot: &ComposedSnapshot) -> SnapshotSummary {
    SnapshotSummary {
        snapshot_id: snapshot.snapshot_id.clone(),
        repo_id: snapshot.repo_id.clone(),
        base_commit: snapshot.base.commit.clone(),
        working_tree_digest: snapshot.working_tree.digest.clone(),
        has_working_tree: !snapshot.working_tree.entries.is_empty(),
        buffer_count: snapshot.buffers.len(),
    }
}

fn render_snapshot_summary(summary: &SnapshotSummary) -> String {
    [
        format!("snapshot_id: {}", summary.snapshot_id),
        format!("repo_id: {}", summary.repo_id),
        format!("base_commit: {}", summary.base_commit),
        format!("working_tree_digest: {}", summary.working_tree_digest),
        format!("has_working_tree: {}", summary.has_working_tree),
        format!("buffer_count: {}", summary.buffer_count),
    ]
    .join("\n")
}

fn render_diff(diff: &SnapshotDiffResponse) -> String {
    [
        format!("left_snapshot_id: {}", diff.left_snapshot_id),
        format!("right_snapshot_id: {}", diff.right_snapshot_id),
        format!("changed_paths: {}", render_list(&diff.changed_paths)),
        format!("added_paths: {}", render_list(&diff.added_paths)),
        format!("deleted_paths: {}", render_list(&diff.deleted_paths)),
        format!(
            "buffer_only_changed_paths: {}",
            render_list(&diff.buffer_only_changed_paths)
        ),
    ]
    .join("\n")
}

#[cfg(test)]
fn render_read_file_source(
    response: &hyperindex_protocol::snapshot::SnapshotReadFileResponse,
) -> String {
    match response.resolved_from.kind {
        hyperindex_protocol::snapshot::SnapshotResolvedFileSourceKind::BufferOverlay => format!(
            "buffer_overlay({})",
            response
                .resolved_from
                .buffer_id
                .as_deref()
                .unwrap_or("unknown-buffer")
        ),
        hyperindex_protocol::snapshot::SnapshotResolvedFileSourceKind::WorkingTreeOverlay => {
            "working_tree_overlay".to_string()
        }
        hyperindex_protocol::snapshot::SnapshotResolvedFileSourceKind::BaseSnapshot => {
            "base_snapshot".to_string()
        }
    }
}

fn render_list(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use hyperindex_config::load_or_default;
    use hyperindex_core::{HyperindexError, HyperindexResult};
    use hyperindex_git_state::GitInspector;
    use hyperindex_protocol::buffers::BufferSetParams;
    use hyperindex_protocol::config::{RuntimeConfig, TransportKind};
    use hyperindex_protocol::repo::ReposAddParams;
    use hyperindex_protocol::snapshot::{
        ComposedSnapshot, SnapshotCreateParams, SnapshotReadFileResponse, WorkingTreeOverlay,
    };
    use hyperindex_repo_store::RepoStore;
    use hyperindex_snapshot::{
        SnapshotAssembler, build_base_snapshot, build_buffer_overlays, build_working_tree_overlay,
    };
    use tempfile::tempdir;

    use super::{create, diff, read_file, render_read_file_source, snapshot_summary};

    #[test]
    fn snapshot_commands_resolve_unsaved_buffer_overlays() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let store = RepoStore::open_from_config(
            &hyperindex_config::load_or_default(Some(&config_path))
                .unwrap()
                .config,
        )
        .unwrap();
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);
        let repo = store
            .add_repo(&ReposAddParams {
                repo_root: repo_root.display().to_string(),
                display_name: Some("Snapshot Repo".to_string()),
                notes: Vec::new(),
                ignore_patterns: Vec::new(),
                watch_on_add: false,
            })
            .unwrap();

        store
            .set_buffer(&BufferSetParams {
                repo_id: repo.repo_id.clone(),
                buffer_id: "buffer-1".to_string(),
                path: "src/app.ts".to_string(),
                version: 2,
                language: Some("typescript".to_string()),
                contents: "export const value = 'buffer';\n".to_string(),
            })
            .unwrap();

        let first = create(
            Some(&config_path),
            &SnapshotCreateParams {
                repo_id: repo.repo_id.clone(),
                include_working_tree: true,
                buffer_ids: vec!["buffer-1".to_string()],
            },
            true,
        )
        .unwrap();
        let created: serde_json::Value = serde_json::from_str(&first).unwrap();
        let snapshot: ComposedSnapshot =
            serde_json::from_value(created.get("snapshot").cloned().unwrap()).unwrap();
        assert_eq!(snapshot_summary(&snapshot).buffer_count, 1);

        let contents = read_file(
            Some(&config_path),
            &snapshot.snapshot_id,
            "src/app.ts",
            false,
        )
        .unwrap();
        assert_eq!(contents, "export const value = 'buffer';\n");

        let read_json = read_file(
            Some(&config_path),
            &snapshot.snapshot_id,
            "src/app.ts",
            true,
        )
        .unwrap();
        let read_response: SnapshotReadFileResponse = serde_json::from_str(&read_json).unwrap();
        assert_eq!(
            render_read_file_source(&read_response),
            "buffer_overlay(buffer-1)"
        );

        let clean = create(
            Some(&config_path),
            &SnapshotCreateParams {
                repo_id: repo.repo_id.clone(),
                include_working_tree: true,
                buffer_ids: Vec::new(),
            },
            true,
        )
        .unwrap();
        let clean_created: serde_json::Value = serde_json::from_str(&clean).unwrap();
        let clean_snapshot: ComposedSnapshot =
            serde_json::from_value(clean_created.get("snapshot").cloned().unwrap()).unwrap();
        let diff_output = diff(
            Some(&config_path),
            &clean_snapshot.snapshot_id,
            &snapshot.snapshot_id,
            true,
        )
        .unwrap();
        assert!(diff_output.contains("\"buffer_only_changed_paths\""));
        assert!(diff_output.contains("\"src/app.ts\""));
    }

    fn write_test_config(root: &Path) -> PathBuf {
        let config_path = root.join("config.toml");
        let runtime_root = root.join(".hyperindex");
        let state_dir = runtime_root.join("state");
        let manifests_dir = runtime_root.join("data/manifests");

        let mut config = RuntimeConfig::default();
        config.directories.runtime_root = runtime_root.clone();
        config.directories.state_dir = state_dir.clone();
        config.directories.data_dir = runtime_root.join("data");
        config.directories.manifests_dir = manifests_dir.clone();
        config.directories.logs_dir = runtime_root.join("logs");
        config.directories.temp_dir = runtime_root.join("tmp");
        config.transport.kind = TransportKind::Stdio;
        config.transport.socket_path = runtime_root.join("hyperd.sock");
        config.repo_registry.sqlite_path = state_dir.join("runtime.sqlite3");
        config.repo_registry.manifests_dir = manifests_dir;
        config.parser.artifact_dir = runtime_root.join("data/parse-artifacts");
        config.symbol_index.store_dir = runtime_root.join("data/symbols");
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        config_path
    }

    fn init_repo(repo_root: &Path) {
        fs::create_dir_all(repo_root.join("src")).unwrap();
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["checkout", "-b", "trunk"]);
        fs::write(
            repo_root.join("src/app.ts"),
            "export const value = 'disk';\n",
        )
        .unwrap();
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

    #[allow(dead_code)]
    fn _build_snapshot_direct(
        config_path: Option<&Path>,
        params: &SnapshotCreateParams,
    ) -> HyperindexResult<ComposedSnapshot> {
        let loaded = load_or_default(config_path)?;
        let store = RepoStore::open_from_config(&loaded.config)?;
        let repo = store.show_repo(&params.repo_id)?;
        let git_state = GitInspector.inspect_repo(&repo.repo_root)?;
        let head_commit = git_state.head_commit.clone().ok_or_else(|| {
            HyperindexError::Message(
                "snapshot create currently requires a git HEAD commit".to_string(),
            )
        })?;
        let base = build_base_snapshot(Path::new(&repo.repo_root), &head_commit)?;
        let working_tree = if params.include_working_tree {
            build_working_tree_overlay(Path::new(&repo.repo_root), &git_state)?
        } else {
            WorkingTreeOverlay {
                digest: hyperindex_snapshot::base::digest_snapshot_components(std::iter::empty()),
                entries: Vec::new(),
            }
        };
        let stored_buffers = store.load_buffers(&repo.repo_id, &params.buffer_ids)?;
        let buffer_overlays = build_buffer_overlays(&stored_buffers)?;
        let snapshot = SnapshotAssembler.compose(
            &repo.repo_id,
            &repo.repo_root,
            base,
            working_tree,
            buffer_overlays,
        )?;
        store.persist_manifest(&snapshot)?;
        Ok(snapshot)
    }
}
