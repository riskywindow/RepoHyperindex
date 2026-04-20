use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hyperindex_config::{LoadedConfig, load_or_default};
use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_repo_store::RepoStore;

use crate::state::DaemonStateManager;

#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub loaded_config: LoadedConfig,
    pub state_manager: Arc<DaemonStateManager>,
}

#[derive(Debug)]
pub struct RuntimeArtifactsGuard {
    pid_file_path: PathBuf,
    socket_path: PathBuf,
    state_manager: Arc<DaemonStateManager>,
}

impl RuntimeState {
    pub fn bootstrap(config_path: Option<&Path>) -> HyperindexResult<Self> {
        let loaded_config = load_or_default(config_path)?;
        ensure_runtime_dirs(&loaded_config)?;
        RepoStore::open_from_config(&loaded_config.config)?;
        let state_manager = DaemonStateManager::new(loaded_config.clone());
        Ok(Self {
            loaded_config,
            state_manager,
        })
    }

    pub fn socket_path(&self) -> PathBuf {
        self.loaded_config.config.transport.socket_path.clone()
    }

    pub fn pid_file_path(&self) -> PathBuf {
        self.loaded_config
            .config
            .directories
            .runtime_root
            .join("hyperd.pid")
    }

    pub fn acquire_runtime_artifacts(&self) -> HyperindexResult<RuntimeArtifactsGuard> {
        let pid_file_path = self.pid_file_path();
        if let Some(parent) = pid_file_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                HyperindexError::Message(format!("failed to create {}: {error}", parent.display()))
            })?;
        }

        let mut file = acquire_pid_file(&pid_file_path)?;
        let pid = std::process::id();
        writeln!(file, "{pid}").map_err(|error| {
            HyperindexError::Message(format!(
                "failed to write daemon pid file {}: {error}",
                pid_file_path.display()
            ))
        })?;
        self.state_manager.set_pid(pid)?;

        let socket_path = self.socket_path();
        cleanup_stale_socket(&socket_path)?;

        Ok(RuntimeArtifactsGuard {
            pid_file_path,
            socket_path,
            state_manager: self.state_manager.clone(),
        })
    }
}

impl Drop for RuntimeArtifactsGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_file(&self.pid_file_path);
        let _ = self.state_manager.clear_pid();
    }
}

fn ensure_runtime_dirs(loaded_config: &LoadedConfig) -> HyperindexResult<()> {
    let directories = &loaded_config.config.directories;
    for path in [
        directories.runtime_root.as_path(),
        directories.state_dir.as_path(),
        directories.data_dir.as_path(),
        directories.manifests_dir.as_path(),
        directories.logs_dir.as_path(),
        directories.temp_dir.as_path(),
    ] {
        fs::create_dir_all(path).map_err(|error| {
            HyperindexError::Message(format!("failed to create {}: {error}", path.display()))
        })?;
    }
    if let Some(parent) = loaded_config.config.transport.socket_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            HyperindexError::Message(format!("failed to create {}: {error}", parent.display()))
        })?;
    }
    Ok(())
}

fn acquire_pid_file(pid_file_path: &Path) -> HyperindexResult<std::fs::File> {
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(pid_file_path)
    {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing_pid = read_pid_file(pid_file_path);
            if let Some(pid) = existing_pid.filter(|pid| process_is_alive(*pid)) {
                return Err(HyperindexError::Message(format!(
                    "daemon lockfile {} is still owned by pid {pid}; stop the daemon first or run `hyperctl doctor` after the process exits",
                    pid_file_path.display()
                )));
            }

            fs::remove_file(pid_file_path).map_err(|remove_error| {
                HyperindexError::Message(format!(
                    "failed to remove stale daemon lockfile {}: {remove_error}",
                    pid_file_path.display()
                ))
            })?;
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(pid_file_path)
                .map_err(|open_error| {
                    HyperindexError::Message(format!(
                        "failed to reacquire daemon lockfile {}: {open_error}",
                        pid_file_path.display()
                    ))
                })
        }
        Err(error) => Err(HyperindexError::Message(format!(
            "failed to acquire daemon lockfile {}: {error}",
            pid_file_path.display()
        ))),
    }
}

fn cleanup_stale_socket(socket_path: &Path) -> HyperindexResult<()> {
    if !socket_path.exists() {
        return Ok(());
    }
    fs::remove_file(socket_path).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to remove stale socket {}: {error}",
            socket_path.display()
        ))
    })
}

fn read_pid_file(pid_file_path: &Path) -> Option<u32> {
    let raw = fs::read_to_string(pid_file_path).ok()?;
    raw.trim().parse::<u32>().ok()
}

fn process_is_alive(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use hyperindex_protocol::config::{RuntimeConfig, TransportKind};

    use super::RuntimeState;

    #[test]
    fn bootstrap_recovers_stale_socket_and_lockfile() {
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
        fs::create_dir_all(&runtime_root).unwrap();
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        fs::write(runtime_root.join("hyperd.pid"), "999999\n").unwrap();
        fs::write(runtime_root.join("hyperd.sock"), "stale socket").unwrap();

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let pid_file_path = runtime.pid_file_path();
        {
            let _guard = runtime.acquire_runtime_artifacts().unwrap();
            let pid = fs::read_to_string(&pid_file_path).unwrap();
            assert_eq!(pid.trim(), std::process::id().to_string());
            assert!(!runtime.socket_path().exists());
        }
        assert!(!pid_file_path.exists());
    }
}
