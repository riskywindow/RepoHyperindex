use std::path::Path;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::repo::{
    RepoRecord, RepoShowParams, RepoStatusParams, RepoStatusResponse, ReposAddParams,
    ReposListParams, ReposRemoveParams,
};
use serde_json::json;

use crate::client::DaemonClient;

pub fn add(
    config_path: Option<&Path>,
    repo_root: &Path,
    display_name: Option<String>,
    notes: Vec<String>,
    ignore_patterns: Vec<String>,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::ReposAdd(ReposAddParams {
        repo_root: repo_root.display().to_string(),
        display_name,
        notes,
        ignore_patterns,
        watch_on_add: false,
    }))? {
        SuccessPayload::ReposAdd(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected repos add response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else {
        Ok(render_repo_detail(&response.repo))
    }
}

pub fn list(config_path: Option<&Path>, json_output: bool) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::ReposList(ReposListParams {
        include_removed: false,
    }))? {
        SuccessPayload::ReposList(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected repos list response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else if response.repos.is_empty() {
        Ok("No repos registered.".to_string())
    } else {
        Ok(response
            .repos
            .iter()
            .map(render_repo_summary)
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

pub fn show(
    config_path: Option<&Path>,
    repo_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::ReposShow(RepoShowParams {
        repo_id: repo_id.to_string(),
    }))? {
        SuccessPayload::ReposShow(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected repos show response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else {
        Ok(render_repo_detail(&response.repo))
    }
}

pub fn remove(
    config_path: Option<&Path>,
    repo_id: &str,
    purge_state: bool,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::ReposRemove(ReposRemoveParams {
        repo_id: repo_id.to_string(),
        purge_state,
    }))? {
        SuccessPayload::ReposRemove(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected repos remove response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else if response.removed {
        Ok(format!(
            "Removed repo {} (purged_state: {})",
            response.repo_id, response.purged_state
        ))
    } else {
        Ok(format!("Repo {} was not registered", response.repo_id))
    }
}

pub fn status(
    config_path: Option<&Path>,
    repo_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let status = build_repo_status(config_path, repo_id)?;
    if json_output {
        Ok(serde_json::to_string_pretty(&status).unwrap())
    } else {
        Ok(render_repo_status(&status))
    }
}

pub fn head(
    config_path: Option<&Path>,
    repo_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let status = build_repo_status(config_path, repo_id)?;
    let head = json!({
        "repo_id": status.repo_id,
        "repo_root": status.repo_root,
        "display_name": status.display_name,
        "branch": status.branch,
        "head_commit": status.head_commit,
        "working_tree_digest": status.working_tree_digest,
        "is_dirty": status.is_dirty,
    });

    if json_output {
        Ok(serde_json::to_string_pretty(&head).unwrap())
    } else {
        Ok([
            format!("repo_id: {}", head["repo_id"].as_str().unwrap_or("-")),
            format!(
                "display_name: {}",
                head["display_name"].as_str().unwrap_or("-")
            ),
            format!("repo_root: {}", head["repo_root"].as_str().unwrap_or("-")),
            format!("branch: {}", head["branch"].as_str().unwrap_or("-")),
            format!(
                "head_commit: {}",
                head["head_commit"].as_str().unwrap_or("-")
            ),
            format!(
                "working_tree_digest: {}",
                head["working_tree_digest"].as_str().unwrap_or("-")
            ),
            format!("is_dirty: {}", head["is_dirty"].as_bool().unwrap_or(false)),
        ]
        .join("\n"))
    }
}

fn build_repo_status(
    config_path: Option<&Path>,
    repo_id: &str,
) -> HyperindexResult<RepoStatusResponse> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::RepoStatus(RepoStatusParams {
        repo_id: repo_id.to_string(),
    }))? {
        SuccessPayload::RepoStatus(response) => Ok(response),
        other => Err(HyperindexError::Message(format!(
            "unexpected repo status response: {other:?}"
        ))),
    }
}

fn render_repo_summary(repo: &RepoRecord) -> String {
    format!(
        "{} | {} | {} | {} | {}",
        repo.repo_id,
        repo.display_name,
        repo.branch.as_deref().unwrap_or("-"),
        if repo.is_dirty { "dirty" } else { "clean" },
        repo.repo_root
    )
}

fn render_repo_detail(repo: &RepoRecord) -> String {
    [
        format!("repo_id: {}", repo.repo_id),
        format!("display_name: {}", repo.display_name),
        format!("repo_root: {}", repo.repo_root),
        format!("created_at: {}", repo.created_at),
        format!("updated_at: {}", repo.updated_at),
        format!("branch: {}", repo.branch.as_deref().unwrap_or("-")),
        format!(
            "head_commit: {}",
            repo.head_commit.as_deref().unwrap_or("-")
        ),
        format!("is_dirty: {}", repo.is_dirty),
        format!(
            "last_snapshot_id: {}",
            repo.last_snapshot_id.as_deref().unwrap_or("-")
        ),
        format!("notes: {}", render_list(&repo.notes)),
        format!("warnings: {}", render_list(&repo.warnings)),
        format!(
            "ignore_patterns: {}",
            render_list(&repo.ignore_settings.patterns)
        ),
    ]
    .join("\n")
}

fn render_repo_status(status: &RepoStatusResponse) -> String {
    [
        format!("repo_id: {}", status.repo_id),
        format!("display_name: {}", status.display_name),
        format!("repo_root: {}", status.repo_root),
        format!("branch: {}", status.branch.as_deref().unwrap_or("-")),
        format!(
            "head_commit: {}",
            status.head_commit.as_deref().unwrap_or("-")
        ),
        format!("working_tree_digest: {}", status.working_tree_digest),
        format!("is_dirty: {}", status.is_dirty),
        format!("dirty_path_count: {}", status.dirty_path_count),
        format!(
            "dirty_tracked_files: {}",
            render_list(&status.dirty_tracked_files)
        ),
        format!("untracked_files: {}", render_list(&status.untracked_files)),
        format!("deleted_files: {}", render_list(&status.deleted_files)),
        format!(
            "renamed_files: {}",
            if status.renamed_files.is_empty() {
                "-".to_string()
            } else {
                status
                    .renamed_files
                    .iter()
                    .map(|rename| format!("{}=>{}", rename.from, rename.to))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ),
        format!("ignored_files: {}", render_list(&status.ignored_files)),
        format!("watch_attached: {}", status.watch_attached),
    ]
    .join("\n")
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
    use hyperindex_core::HyperindexResult;
    use hyperindex_git_state::GitInspector;
    use hyperindex_protocol::config::RuntimeConfig;
    use hyperindex_protocol::repo::{RepoStatusResponse, ReposAddParams, ReposRemoveParams};
    use hyperindex_repo_store::RepoStore;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{render_repo_detail, render_repo_status, render_repo_summary};

    #[test]
    fn repo_renderers_cover_human_and_json_shapes() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("demo-repo");
        init_repo(&repo_root);

        let store = open_store_direct(Some(&config_path)).unwrap();
        let repo = store
            .add_repo(&ReposAddParams {
                repo_root: repo_root.display().to_string(),
                display_name: Some("Demo Repo".to_string()),
                notes: vec!["owned by search team".to_string()],
                ignore_patterns: vec!["coverage/**".to_string()],
                watch_on_add: false,
            })
            .unwrap();
        let added = render_repo_detail(&repo);
        assert!(added.contains("display_name: Demo Repo"));

        let listed = store
            .list_repos()
            .unwrap()
            .iter()
            .map(render_repo_summary)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(listed.contains("Demo Repo"));
        assert!(listed.contains("| clean |"));

        let listed_json = serde_json::to_string_pretty(&json!({
            "repos": store.list_repos().unwrap()
        }))
        .unwrap();
        assert!(listed_json.contains("\"repos\""));
        assert!(listed_json.contains("\"display_name\": \"Demo Repo\""));

        let shown = render_repo_detail(&store.show_repo(&repo.repo_id).unwrap());
        assert!(shown.contains("repo_id:"));
        assert!(shown.contains("ignore_patterns: coverage/**"));

        fs::write(repo_root.join("src/changed.ts"), "const changed = true;\n").unwrap();
        fs::write(repo_root.join("scratch.txt"), "temp\n").unwrap();
        let status_output = render_repo_status(
            &build_repo_status_direct(Some(&config_path), &repo.repo_id).unwrap(),
        );
        assert!(status_output.contains("dirty_tracked_files:"));
        assert!(status_output.contains("src/changed.ts"));
        assert!(status_output.contains("untracked_files:"));
        assert!(status_output.contains("scratch.txt"));

        let head_output = serde_json::to_string_pretty(&json!({
            "working_tree_digest": build_repo_status_direct(Some(&config_path), &repo.repo_id)
                .unwrap()
                .working_tree_digest,
            "head_commit": build_repo_status_direct(Some(&config_path), &repo.repo_id)
                .unwrap()
                .head_commit,
        }))
        .unwrap();
        assert!(head_output.contains("\"working_tree_digest\""));
        assert!(head_output.contains("\"head_commit\""));

        let removed_json = serde_json::to_string_pretty(
            &store
                .remove_repo(&ReposRemoveParams {
                    repo_id: repo.repo_id,
                    purge_state: true,
                })
                .unwrap(),
        )
        .unwrap();
        assert!(removed_json.contains("\"removed\": true"));
        assert!(removed_json.contains("\"purged_state\": true"));
    }

    fn open_store_direct(config_path: Option<&Path>) -> HyperindexResult<RepoStore> {
        let loaded = load_or_default(config_path)?;
        RepoStore::open_from_config(&loaded.config)
    }

    fn build_repo_status_direct(
        config_path: Option<&Path>,
        repo_id: &str,
    ) -> HyperindexResult<RepoStatusResponse> {
        let store = open_store_direct(config_path)?;
        let repo = store.show_repo(repo_id)?;
        let git_state = GitInspector.inspect_repo(&repo.repo_root)?;
        let dirty_path_count = git_state.working_tree.dirty_tracked_files.len()
            + git_state.working_tree.untracked_files.len()
            + git_state.working_tree.deleted_files.len()
            + git_state.working_tree.renamed_files.len();
        Ok(RepoStatusResponse {
            repo_id: repo.repo_id,
            repo_root: repo.repo_root,
            display_name: repo.display_name,
            branch: git_state.branch,
            head_commit: git_state.head_commit,
            working_tree_digest: git_state.working_tree.digest.clone(),
            is_dirty: dirty_path_count > 0,
            watch_attached: false,
            dirty_path_count,
            dirty_tracked_files: git_state.working_tree.dirty_tracked_files,
            untracked_files: git_state.working_tree.untracked_files,
            deleted_files: git_state.working_tree.deleted_files,
            renamed_files: git_state.working_tree.renamed_files,
            ignored_files: git_state.working_tree.ignored_files,
            last_snapshot_id: repo.last_snapshot_id,
            active_job: None,
            last_error_code: None,
        })
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
        fs::write(repo_root.join("README.md"), "# demo\n").unwrap();
        fs::write(repo_root.join("src/changed.ts"), "const changed = false;\n").unwrap();
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
