use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use hyperindex_config::{LoadedConfig, load_or_default};
use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::api::{
    DaemonRequest, DaemonResponse, RequestBody, ResponseBody, SuccessPayload,
};
use hyperindex_protocol::config::TransportKind;
use hyperindex_protocol::errors::ProtocolError;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct DaemonClient {
    loaded_config: LoadedConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeCleanupReport {
    pub live_pid: Option<u32>,
    pub removed_pid_file: bool,
    pub removed_socket: bool,
}

impl DaemonClient {
    pub fn load(config_path: Option<&Path>) -> HyperindexResult<Self> {
        Ok(Self {
            loaded_config: load_or_default(config_path)?,
        })
    }

    pub fn loaded_config(&self) -> &LoadedConfig {
        &self.loaded_config
    }

    pub fn send(&self, body: RequestBody) -> HyperindexResult<SuccessPayload> {
        let request = DaemonRequest::new(next_request_id(&body), body);
        match self.loaded_config.config.transport.kind {
            TransportKind::UnixSocket => self.send_unix_socket(&request),
            TransportKind::Stdio => self.send_stdio(&request),
        }
    }

    fn send_unix_socket(&self, request: &DaemonRequest) -> HyperindexResult<SuccessPayload> {
        let socket_path = &self.loaded_config.config.transport.socket_path;
        let connect_timeout =
            Duration::from_millis(self.loaded_config.config.transport.connect_timeout_ms);
        let request_timeout =
            Duration::from_millis(self.loaded_config.config.transport.request_timeout_ms);
        let max_frame_bytes = self.loaded_config.config.transport.max_frame_bytes;

        let mut stream = connect_with_retry(socket_path, connect_timeout)?;
        stream
            .set_read_timeout(Some(request_timeout))
            .map_err(|error| transport_error(format!("failed to set read timeout: {error}")))?;
        stream
            .set_write_timeout(Some(request_timeout))
            .map_err(|error| transport_error(format!("failed to set write timeout: {error}")))?;

        let raw = serde_json::to_vec(request)
            .map_err(|error| transport_error(format!("request serialization failed: {error}")))?;
        if raw.len() > max_frame_bytes {
            return Err(transport_error(format!(
                "request exceeded max frame size of {max_frame_bytes} bytes"
            )));
        }

        stream
            .write_all(&raw)
            .map_err(|error| transport_error(format!("request write failed: {error}")))?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|error| transport_error(format!("request shutdown failed: {error}")))?;

        let mut encoded = Vec::new();
        let deadline = Instant::now() + request_timeout;
        let mut buffer = [0_u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => encoded.extend_from_slice(&buffer[..read]),
                Err(error)
                    if Instant::now() < deadline
                        && matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    return Err(transport_error(format!("response read failed: {error}")));
                }
            }
        }
        if encoded.len() > max_frame_bytes {
            return Err(transport_error(format!(
                "response exceeded max frame size of {max_frame_bytes} bytes"
            )));
        }
        decode_success_payload(&encoded)
    }

    fn send_stdio(&self, request: &DaemonRequest) -> HyperindexResult<SuccessPayload> {
        let mut command = daemon_command(&self.loaded_config.config_path)?;
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| transport_error(format!("failed to spawn hyperd: {error}")))?;

        let raw = serde_json::to_vec(request)
            .map_err(|error| transport_error(format!("request serialization failed: {error}")))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(&raw)
                .map_err(|error| transport_error(format!("failed to write request: {error}")))?;
        }

        let output = child.wait_with_output().map_err(|error| {
            transport_error(format!("failed to collect daemon output: {error}"))
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(transport_error(format!(
                "hyperd stdio request failed{}",
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            )));
        }

        decode_success_payload(&output.stdout)
    }
}

pub fn daemon_pid_file_path(loaded: &LoadedConfig) -> PathBuf {
    loaded.config.directories.runtime_root.join("hyperd.pid")
}

pub fn daemon_log_file_path(loaded: &LoadedConfig) -> PathBuf {
    loaded.config.directories.logs_dir.join("hyperd.log")
}

pub fn read_pid_file(loaded: &LoadedConfig) -> Option<u32> {
    let raw = fs::read_to_string(daemon_pid_file_path(loaded)).ok()?;
    raw.trim().parse::<u32>().ok()
}

pub fn process_is_alive(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn cleanup_stale_runtime_artifacts(
    loaded: &LoadedConfig,
) -> HyperindexResult<RuntimeCleanupReport> {
    let mut report = RuntimeCleanupReport::default();
    let pid_path = daemon_pid_file_path(loaded);
    if let Some(pid) = read_pid_file(loaded).filter(|pid| process_is_alive(*pid)) {
        report.live_pid = Some(pid);
        return Ok(report);
    }

    if pid_path.exists() {
        fs::remove_file(&pid_path).map_err(|error| {
            transport_error(format!(
                "failed to remove stale daemon lockfile {}: {error}",
                pid_path.display()
            ))
        })?;
        report.removed_pid_file = true;
    }

    let socket_path = &loaded.config.transport.socket_path;
    if socket_path.exists() && UnixStream::connect(socket_path).is_err() {
        fs::remove_file(socket_path).map_err(|error| {
            transport_error(format!(
                "failed to remove stale socket {}: {error}",
                socket_path.display()
            ))
        })?;
        report.removed_socket = true;
    }

    Ok(report)
}

pub fn daemon_command(config_path: &Path) -> HyperindexResult<Command> {
    let config_path = config_path.to_path_buf();
    if let Some(explicit) = std::env::var_os("HYPERD_BIN") {
        let mut command = Command::new(explicit);
        command.arg("--config-path").arg(&config_path).arg("serve");
        return Ok(command);
    }

    if let Some(binary) = sibling_hyperd_path()? {
        let mut command = Command::new(binary);
        command.arg("--config-path").arg(&config_path).arg("serve");
        return Ok(command);
    }

    if let Some(workspace_root) = find_workspace_root()? {
        let mut command = Command::new("cargo");
        command
            .arg("run")
            .arg("--quiet")
            .arg("-p")
            .arg("hyperindex-daemon")
            .arg("--bin")
            .arg("hyperd")
            .arg("--")
            .arg("--config-path")
            .arg(&config_path)
            .arg("serve")
            .current_dir(workspace_root);
        return Ok(command);
    }

    Err(HyperindexError::Message(
        "could not resolve a hyperd launcher; build hyperd first or set HYPERD_BIN".to_string(),
    ))
}

fn decode_success_payload(encoded: &[u8]) -> HyperindexResult<SuccessPayload> {
    let response: DaemonResponse = serde_json::from_slice(encoded)
        .map_err(|error| transport_error(format!("response decode failed: {error}")))?;
    match response.body {
        ResponseBody::Success { result } => Ok(result),
        ResponseBody::Error { error, .. } => Err(protocol_error(error)),
    }
}

fn connect_with_retry(socket_path: &Path, timeout: Duration) -> HyperindexResult<UnixStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if Instant::now() < deadline
                    && matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound
                            | std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::WouldBlock
                    ) =>
            {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                let hint = if socket_path.exists() {
                    "; run `hyperctl doctor` to clean stale runtime artifacts if the daemon is stopped"
                } else {
                    ""
                };
                return Err(transport_error(format!(
                    "failed to connect to {}: {error}{hint}",
                    socket_path.display()
                )));
            }
        }
    }
}

fn next_request_id(body: &RequestBody) -> String {
    let ordinal = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let method = match body {
        RequestBody::Health(_) => "health",
        RequestBody::Version(_) => "version",
        RequestBody::DaemonStatus(_) => "daemon-status",
        RequestBody::ReposAdd(_) => "repos-add",
        RequestBody::ReposList(_) => "repos-list",
        RequestBody::ReposRemove(_) => "repos-remove",
        RequestBody::ReposShow(_) => "repos-show",
        RequestBody::RepoStatus(_) => "repo-status",
        RequestBody::WatchStatus(_) => "watch-status",
        RequestBody::WatchEvents(_) => "watch-events",
        RequestBody::SnapshotsCreate(_) => "snapshots-create",
        RequestBody::SnapshotsShow(_) => "snapshots-show",
        RequestBody::SnapshotsList(_) => "snapshots-list",
        RequestBody::SnapshotsDiff(_) => "snapshots-diff",
        RequestBody::SnapshotsReadFile(_) => "snapshots-read-file",
        RequestBody::BuffersSet(_) => "buffers-set",
        RequestBody::BuffersClear(_) => "buffers-clear",
        RequestBody::BuffersList(_) => "buffers-list",
        RequestBody::ParseBuild(_) => "parse-build",
        RequestBody::ParseStatus(_) => "parse-status",
        RequestBody::ParseInspectFile(_) => "parse-inspect-file",
        RequestBody::SymbolIndexBuild(_) => "symbol-index-build",
        RequestBody::SymbolIndexStatus(_) => "symbol-index-status",
        RequestBody::SymbolSearch(_) => "symbol-search",
        RequestBody::SymbolShow(_) => "symbol-show",
        RequestBody::DefinitionLookup(_) => "definition-lookup",
        RequestBody::ReferenceLookup(_) => "reference-lookup",
        RequestBody::SymbolResolve(_) => "symbol-resolve",
        RequestBody::SemanticStatus(_) => "semantic-status",
        RequestBody::SemanticBuild(_) => "semantic-build",
        RequestBody::SemanticQuery(_) => "semantic-query",
        RequestBody::SemanticInspectChunk(_) => "semantic-inspect-chunk",
        RequestBody::PlannerStatus(_) => "planner-status",
        RequestBody::PlannerQuery(_) => "planner-query",
        RequestBody::PlannerExplain(_) => "planner-explain",
        RequestBody::PlannerCapabilities(_) => "planner-capabilities",
        RequestBody::ImpactStatus(_) => "impact-status",
        RequestBody::ImpactAnalyze(_) => "impact-analyze",
        RequestBody::ImpactExplain(_) => "impact-explain",
        RequestBody::Shutdown(_) => "shutdown",
    };
    format!("{method}-{ordinal}")
}

fn protocol_error(error: ProtocolError) -> HyperindexError {
    let code = format!("{:?}", error.code).to_lowercase();
    let category = format!("{:?}", error.category).to_lowercase();
    let details = error
        .payload
        .map(|details| {
            format!(
                " payload={}",
                serde_json::to_string(&details).unwrap_or_default()
            )
        })
        .unwrap_or_default();
    HyperindexError::Message(format!("{category}/{code}: {}{details}", error.message))
}

fn transport_error(message: impl Into<String>) -> HyperindexError {
    HyperindexError::Message(message.into())
}

fn sibling_hyperd_path() -> HyperindexResult<Option<PathBuf>> {
    let current_exe = std::env::current_exe().map_err(|error| {
        transport_error(format!("failed to inspect current executable: {error}"))
    })?;
    let mut candidates = Vec::new();
    if let Some(parent) = current_exe.parent() {
        if parent.file_name() != Some(OsString::from("deps").as_os_str()) {
            candidates.push(parent.join("hyperd"));
        }
    }
    Ok(candidates.into_iter().find(|candidate| candidate.exists()))
}

fn find_workspace_root() -> HyperindexResult<Option<PathBuf>> {
    let mut current = std::env::current_dir()
        .map_err(|error| transport_error(format!("failed to read current directory: {error}")))?;
    loop {
        if current.join("Cargo.toml").exists() {
            return Ok(Some(current));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}
