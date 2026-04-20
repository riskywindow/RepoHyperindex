use std::path::Path;
use std::time::Duration;

use hyperindex_config::load_or_default;
use hyperindex_core::HyperindexResult;
use hyperindex_protocol::watch::WatchEventsResponse;
use hyperindex_repo_store::RepoStore;
use hyperindex_watcher::WatcherService;

pub fn once(
    config_path: Option<&Path>,
    repo_id: &str,
    timeout_ms: u64,
    json_output: bool,
) -> HyperindexResult<String> {
    let loaded = load_or_default(config_path)?;
    let store = RepoStore::open_from_config(&loaded.config)?;
    let repo = store.show_repo(repo_id)?;
    let mut ignore_patterns = loaded.config.ignores.global_patterns.clone();
    ignore_patterns.extend(loaded.config.ignores.repo_patterns.clone());
    ignore_patterns.extend(repo.ignore_settings.patterns.clone());

    let mut watcher = WatcherService::polling(
        &repo.repo_root,
        loaded.config.watch.clone(),
        ignore_patterns,
    )?;
    let run = watcher.watch_once(Duration::from_millis(timeout_ms))?;
    let response = WatchEventsResponse {
        repo_id: repo.repo_id.clone(),
        next_cursor: run.events.last().map(|event| event.sequence),
        events: run.events,
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else if response.events.is_empty() {
        Ok(format!(
            "No events observed for repo {} within {} ms.",
            response.repo_id, timeout_ms
        ))
    } else {
        Ok(response
            .events
            .iter()
            .map(|event| match &event.previous_path {
                Some(previous_path) => format!(
                    "{} | {:?} | {} <= {}",
                    event.sequence, event.kind, event.path, previous_path
                ),
                None => format!("{} | {:?} | {}", event.sequence, event.kind, event.path),
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::Duration;

    use hyperindex_protocol::config::{RuntimeConfig, WatchBackend};
    use hyperindex_protocol::repo::ReposAddParams;
    use hyperindex_repo_store::RepoStore;
    use tempfile::tempdir;

    use super::once;

    #[test]
    fn watch_once_renders_observed_events() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let store = RepoStore::open_from_config(
            &hyperindex_config::load_or_default(Some(&config_path))
                .unwrap()
                .config,
        )
        .unwrap();
        let repo_root = tempdir.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();
        let repo = store
            .add_repo(&ReposAddParams {
                repo_root: repo_root.display().to_string(),
                display_name: Some("Watch Repo".to_string()),
                notes: Vec::new(),
                ignore_patterns: vec!["ignored/**".to_string()],
                watch_on_add: false,
            })
            .unwrap();

        let writer_root = repo_root.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            fs::write(writer_root.join("created.txt"), "hello\n").unwrap();
        });

        let output = once(Some(&config_path), &repo.repo_id, 250, false).unwrap();
        assert!(output.contains("Created"));
        assert!(output.contains("created.txt"));
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
        config.watch.backend = WatchBackend::Poll;
        config.watch.poll_interval_ms = 20;
        config.watch.debounce_ms = 60;
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        config_path
    }
}
