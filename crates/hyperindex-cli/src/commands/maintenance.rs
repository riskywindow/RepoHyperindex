use std::fs;
use std::path::{Path, PathBuf};

use hyperindex_config::load_or_default;
use hyperindex_core::{HyperindexError, HyperindexResult, normalize_repo_relative_path};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::status::{DaemonStatusParams, RuntimeStatus};
use hyperindex_repo_store::RepoStore;
use serde_json::json;

use crate::client::{DaemonClient, cleanup_stale_runtime_artifacts};

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorIssue {
    code: &'static str,
    message: String,
}

pub fn doctor(config_path: Option<&Path>, json_output: bool) -> HyperindexResult<String> {
    let loaded = load_or_default(config_path)?;
    let reachable_runtime = daemon_runtime_status(config_path).ok();
    let cleanup = if reachable_runtime.is_none() {
        cleanup_stale_runtime_artifacts(&loaded)?
    } else {
        Default::default()
    };
    let store = RepoStore::open_from_config(&loaded.config)?;
    let summary = store.summary()?;
    let repos = store.list_repos()?;

    let mut issues = Vec::new();
    let mut actions = Vec::new();

    if cleanup.removed_pid_file {
        actions.push("removed stale daemon lockfile".to_string());
    }
    if cleanup.removed_socket {
        actions.push("removed stale daemon socket".to_string());
    }
    if let Some(pid) = cleanup.live_pid {
        issues.push(DoctorIssue {
            code: "daemon_live",
            message: format!(
                "daemon pid {pid} is still running; stop it before cleanup or reset-runtime"
            ),
        });
    }

    for repo in &repos {
        if !Path::new(&repo.repo_root).exists() {
            issues.push(DoctorIssue {
                code: "repo_missing",
                message: format!(
                    "repo {} is missing at {}; restore the path or remove it with `hyperctl repos remove --repo-id {}`",
                    repo.repo_id, repo.repo_root, repo.repo_id
                ),
            });
        }

        for buffer in store.list_buffers(&hyperindex_protocol::buffers::BufferListParams {
            repo_id: repo.repo_id.clone(),
        })? {
            if let Err(error) = normalize_repo_relative_path(&buffer.path, "buffer overlay") {
                issues.push(DoctorIssue {
                    code: "buffer_invalid",
                    message: format!(
                        "repo {} buffer {} has invalid path {}: {}",
                        repo.repo_id, buffer.buffer_id, buffer.path, error
                    ),
                });
            }
        }
    }

    let temp_manifest_files =
        collect_temp_manifest_files(&loaded.config.repo_registry.manifests_dir)?;
    if !temp_manifest_files.is_empty() {
        issues.push(DoctorIssue {
            code: "manifest_temp_files",
            message: format!(
                "{} manifest temp file(s) can be removed with `hyperctl cleanup`",
                temp_manifest_files.len()
            ),
        });
    }

    let output = json!({
        "daemon_reachable": reachable_runtime.is_some(),
        "repo_count": summary.repo_count,
        "manifest_count": summary.manifest_count,
        "actions": actions,
        "issues": issues.iter().map(|issue| json!({
            "code": issue.code,
            "message": issue.message,
        })).collect::<Vec<_>>(),
        "runtime_root": loaded.config.directories.runtime_root.display().to_string(),
    });

    if json_output {
        return Ok(serde_json::to_string_pretty(&output).unwrap());
    }

    let mut lines = vec![
        format!(
            "daemon_reachable: {}",
            if reachable_runtime.is_some() {
                "true"
            } else {
                "false"
            }
        ),
        format!("repo_count: {}", summary.repo_count),
        format!("manifest_count: {}", summary.manifest_count),
    ];
    if actions.is_empty() {
        lines.push("actions: -".to_string());
    } else {
        lines.push(format!("actions: {}", actions.join("; ")));
    }
    if issues.is_empty() {
        lines.push("issues: none".to_string());
    } else {
        lines.push(format!("issues: {}", issues.len()));
        lines.extend(
            issues
                .iter()
                .map(|issue| format!("- [{}] {}", issue.code, issue.message)),
        );
    }
    Ok(lines.join("\n"))
}

pub fn cleanup(config_path: Option<&Path>, json_output: bool) -> HyperindexResult<String> {
    let loaded = load_or_default(config_path)?;
    ensure_daemon_stopped(config_path)?;

    let cleanup = cleanup_stale_runtime_artifacts(&loaded)?;
    let mut removed = Vec::new();
    if cleanup.removed_pid_file {
        removed.push("daemon lockfile".to_string());
    }
    if cleanup.removed_socket {
        removed.push("daemon socket".to_string());
    }

    for path in collect_temp_manifest_files(&loaded.config.repo_registry.manifests_dir)? {
        fs::remove_file(&path).map_err(|error| {
            HyperindexError::Message(format!("failed to remove {}: {error}", path.display()))
        })?;
        removed.push(path.display().to_string());
    }

    removed.extend(remove_dir_contents(&loaded.config.directories.temp_dir)?);
    RepoStore::open_from_config(&loaded.config)?;

    let output = json!({
        "status": "cleaned",
        "removed": removed,
    });
    if json_output {
        return Ok(serde_json::to_string_pretty(&output).unwrap());
    }

    Ok(if removed.is_empty() {
        "No stale runtime artifacts found.".to_string()
    } else {
        format!("Removed: {}", removed.join(", "))
    })
}

pub fn reset_runtime(config_path: Option<&Path>, json_output: bool) -> HyperindexResult<String> {
    let loaded = load_or_default(config_path)?;
    ensure_daemon_stopped(config_path)?;

    let mut removed = Vec::new();
    for directory in [
        loaded.config.directories.state_dir.clone(),
        loaded.config.directories.data_dir.clone(),
        loaded.config.directories.logs_dir.clone(),
        loaded.config.directories.temp_dir.clone(),
    ] {
        if directory.exists() {
            fs::remove_dir_all(&directory).map_err(|error| {
                HyperindexError::Message(format!(
                    "failed to remove {}: {error}",
                    directory.display()
                ))
            })?;
            removed.push(directory.display().to_string());
        }
    }
    for path in [
        loaded.config.transport.socket_path.clone(),
        loaded.config.directories.runtime_root.join("hyperd.pid"),
    ] {
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                HyperindexError::Message(format!("failed to remove {}: {error}", path.display()))
            })?;
            removed.push(path.display().to_string());
        }
    }

    RepoStore::open_from_config(&loaded.config)?;
    ensure_runtime_dirs(&loaded)?;

    let output = json!({
        "status": "reset",
        "removed": removed,
    });
    if json_output {
        return Ok(serde_json::to_string_pretty(&output).unwrap());
    }
    Ok(format!(
        "Runtime reset complete. Removed: {}",
        if removed.is_empty() {
            "-".to_string()
        } else {
            removed.join(", ")
        }
    ))
}

fn daemon_runtime_status(config_path: Option<&Path>) -> HyperindexResult<RuntimeStatus> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::DaemonStatus(DaemonStatusParams::default()))? {
        SuccessPayload::DaemonStatus(status) => Ok(status),
        other => Err(HyperindexError::Message(format!(
            "unexpected daemon status response: {other:?}"
        ))),
    }
}

fn ensure_daemon_stopped(config_path: Option<&Path>) -> HyperindexResult<()> {
    if let Ok(status) = daemon_runtime_status(config_path) {
        return Err(HyperindexError::Message(format!(
            "daemon is still running with pid {}; stop it before mutating runtime state",
            status
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )));
    }
    Ok(())
}

fn collect_temp_manifest_files(root: &Path) -> HyperindexResult<Vec<PathBuf>> {
    let mut collected = Vec::new();
    collect_matching_files(root, &mut collected, |path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.ends_with(".json.tmp"))
            .unwrap_or(false)
    })?;
    Ok(collected)
}

fn remove_dir_contents(root: &Path) -> HyperindexResult<Vec<String>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut removed = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to read temp dir {}: {error}",
            root.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            HyperindexError::Message(format!("failed to read temp dir entry: {error}"))
        })?;
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path).map_err(|error| {
                HyperindexError::Message(format!("failed to remove {}: {error}", path.display()))
            })?;
        } else {
            fs::remove_file(&path).map_err(|error| {
                HyperindexError::Message(format!("failed to remove {}: {error}", path.display()))
            })?;
        }
        removed.push(path.display().to_string());
    }
    Ok(removed)
}

fn collect_matching_files(
    root: &Path,
    collected: &mut Vec<PathBuf>,
    predicate: impl Fn(&Path) -> bool + Copy,
) -> HyperindexResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|error| {
        HyperindexError::Message(format!("failed to read {}: {error}", root.display()))
    })? {
        let entry = entry.map_err(|error| {
            HyperindexError::Message(format!("failed to read directory entry: {error}"))
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_matching_files(&path, collected, predicate)?;
        } else if predicate(&path) {
            collected.push(path);
        }
    }
    Ok(())
}

fn ensure_runtime_dirs(loaded: &hyperindex_config::LoadedConfig) -> HyperindexResult<()> {
    for path in [
        loaded.config.directories.runtime_root.as_path(),
        loaded.config.directories.state_dir.as_path(),
        loaded.config.directories.data_dir.as_path(),
        loaded.config.directories.manifests_dir.as_path(),
        loaded.config.directories.logs_dir.as_path(),
        loaded.config.directories.temp_dir.as_path(),
    ] {
        fs::create_dir_all(path).map_err(|error| {
            HyperindexError::Message(format!("failed to create {}: {error}", path.display()))
        })?;
    }
    if let Some(parent) = loaded.config.transport.socket_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            HyperindexError::Message(format!("failed to create {}: {error}", parent.display()))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use hyperindex_protocol::config::{RuntimeConfig, TransportKind};
    use tempfile::tempdir;

    use super::{cleanup, doctor, reset_runtime};

    #[test]
    fn maintenance_commands_report_and_reset_local_runtime() {
        let tempdir = tempdir().unwrap();
        let config_path = tempdir.path().join("config.toml");
        let runtime_root = tempdir.path().join(".hyperindex");

        let mut config = RuntimeConfig::default();
        config.directories.runtime_root = runtime_root.clone();
        config.directories.state_dir = runtime_root.join("state");
        config.directories.data_dir = runtime_root.join("data");
        config.directories.manifests_dir = runtime_root.join("data/manifests");
        config.directories.logs_dir = runtime_root.join("logs");
        config.directories.temp_dir = runtime_root.join("tmp");
        config.transport.kind = TransportKind::UnixSocket;
        config.transport.socket_path = runtime_root.join("hyperd.sock");
        config.repo_registry.sqlite_path = runtime_root.join("state/runtime.sqlite3");
        config.repo_registry.manifests_dir = runtime_root.join("data/manifests");
        fs::create_dir_all(config.directories.temp_dir.clone()).unwrap();
        fs::create_dir_all(config.repo_registry.manifests_dir.join("repo-1")).unwrap();
        fs::write(
            config
                .repo_registry
                .manifests_dir
                .join("repo-1/stale.json.tmp"),
            "tmp",
        )
        .unwrap();
        fs::write(
            config.directories.runtime_root.join("hyperd.pid"),
            "999999\n",
        )
        .unwrap();
        fs::write(config.transport.socket_path.clone(), "stale").unwrap();
        fs::write(config.directories.temp_dir.join("scratch.txt"), "scratch").unwrap();
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

        let doctor_output = doctor(Some(&config_path), false).unwrap();
        assert!(doctor_output.contains("removed stale daemon lockfile"));

        let cleanup_output = cleanup(Some(&config_path), false).unwrap();
        assert!(cleanup_output.contains("scratch.txt"));

        let reset_output = reset_runtime(Some(&config_path), false).unwrap();
        assert!(reset_output.contains("Runtime reset complete"));
        assert!(config.directories.state_dir.exists());
    }
}
