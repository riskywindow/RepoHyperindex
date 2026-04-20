use std::fs::{self, OpenOptions};
use std::path::Path;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::config::TransportKind;
use hyperindex_protocol::status::TransportSummary;
use hyperindex_protocol::status::{
    DaemonLifecycleState, DaemonStatusParams, RuntimeStatus, ShutdownParams,
};
use hyperindex_protocol::{CONFIG_VERSION, PROTOCOL_VERSION};
use hyperindex_repo_store::RepoStore;
use hyperindex_scheduler::SchedulerService;
use serde_json::json;

use crate::client::{
    DaemonClient, cleanup_stale_runtime_artifacts, daemon_command, daemon_log_file_path,
    read_pid_file,
};
use hyperindex_daemon::impact::scan_impact_runtime_status;
use hyperindex_daemon::semantic::scan_semantic_runtime_status;

pub fn start(config_path: Option<&Path>, json_output: bool) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    require_socket_transport(client.loaded_config())?;

    if let Ok(status) = live_status(&client) {
        return render_lifecycle_result(
            "already_running",
            "daemon is already running",
            &status,
            Some(daemon_log_file_path(client.loaded_config())),
            json_output,
        );
    }

    let cleanup = cleanup_stale_runtime_artifacts(client.loaded_config())?;
    if let Some(pid) = cleanup.live_pid {
        return Err(HyperindexError::Message(format!(
            "daemon process {pid} is still running but not responding on {}; stop it first or inspect {}",
            client
                .loaded_config()
                .config
                .transport
                .socket_path
                .display(),
            daemon_log_file_path(client.loaded_config()).display()
        )));
    }

    fs::create_dir_all(&client.loaded_config().config.directories.logs_dir).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to create {}: {error}",
            client.loaded_config().config.directories.logs_dir.display()
        ))
    })?;
    let log_path = daemon_log_file_path(client.loaded_config());
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| {
            HyperindexError::Message(format!("failed to open {}: {error}", log_path.display()))
        })?;
    let stderr = stdout.try_clone().map_err(|error| {
        HyperindexError::Message(format!("failed to clone log handle: {error}"))
    })?;

    let mut command = daemon_command(&client.loaded_config().config_path)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let mut child = command
        .spawn()
        .map_err(|error| HyperindexError::Message(format!("failed to spawn hyperd: {error}")))?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            HyperindexError::Message(format!("failed to inspect hyperd: {error}"))
        })? {
            let log_excerpt = fs::read_to_string(&log_path)
                .ok()
                .and_then(|contents| contents.lines().last().map(str::to_string));
            let suffix = log_excerpt
                .map(|line| format!("; last log line: {line}"))
                .unwrap_or_default();
            return Err(HyperindexError::Message(format!(
                "daemon exited before becoming ready (status: {status}){suffix}"
            )));
        }
        if let Ok(status) = live_status(&client) {
            return render_lifecycle_result(
                "started",
                "daemon started",
                &status,
                Some(log_path),
                json_output,
            );
        }
        if Instant::now() >= deadline {
            return Err(HyperindexError::Message(format!(
                "daemon did not become ready within 5s; inspect {}",
                daemon_log_file_path(client.loaded_config()).display()
            )));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn status(config_path: Option<&Path>, json_output: bool) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let log_path = daemon_log_file_path(client.loaded_config());
    match live_status(&client) {
        Ok(status) => render_status_report(
            "daemon",
            true,
            "daemon is reachable",
            &status,
            Some(log_path),
            json_output,
        ),
        Err(_) => {
            let cleanup = cleanup_stale_runtime_artifacts(client.loaded_config())?;
            let local = local_stopped_status(client.loaded_config())?;
            render_status_report(
                "local",
                false,
                if cleanup.removed_pid_file || cleanup.removed_socket {
                    "daemon is not reachable; cleaned stale runtime artifacts"
                } else {
                    "daemon is not reachable"
                },
                &local,
                Some(log_path),
                json_output,
            )
        }
    }
}

pub fn stop(config_path: Option<&Path>, json_output: bool) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    require_socket_transport(client.loaded_config())?;

    let before = match live_status(&client) {
        Ok(status) => status,
        Err(_) => {
            let cleanup = cleanup_stale_runtime_artifacts(client.loaded_config())?;
            let local = local_stopped_status(client.loaded_config())?;
            return render_lifecycle_result(
                if cleanup.removed_pid_file || cleanup.removed_socket {
                    "stale_runtime_cleaned"
                } else {
                    "already_stopped"
                },
                if cleanup.removed_pid_file || cleanup.removed_socket {
                    "daemon was already stopped; cleaned stale runtime artifacts"
                } else {
                    "daemon is already stopped"
                },
                &local,
                Some(daemon_log_file_path(client.loaded_config())),
                json_output,
            );
        }
    };

    match client.send(RequestBody::Shutdown(ShutdownParams {
        graceful: true,
        timeout_ms: Some(5_000),
    }))? {
        SuccessPayload::Shutdown(_) => {}
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected shutdown response: {other:?}"
            )));
        }
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if live_status(&client).is_err() {
            let local = local_stopped_status(client.loaded_config())?;
            return render_lifecycle_result(
                "stopped",
                "daemon stopped",
                &local,
                Some(daemon_log_file_path(client.loaded_config())),
                json_output,
            );
        }
        thread::sleep(Duration::from_millis(50));
    }

    render_lifecycle_result(
        "shutdown_requested",
        "shutdown requested but daemon is still reachable",
        &before,
        Some(daemon_log_file_path(client.loaded_config())),
        json_output,
    )
}

fn live_status(client: &DaemonClient) -> HyperindexResult<RuntimeStatus> {
    match client.send(RequestBody::DaemonStatus(DaemonStatusParams::default()))? {
        SuccessPayload::DaemonStatus(status) => Ok(status),
        other => Err(HyperindexError::Message(format!(
            "unexpected daemon status response: {other:?}"
        ))),
    }
}

fn local_stopped_status(
    loaded: &hyperindex_config::LoadedConfig,
) -> HyperindexResult<RuntimeStatus> {
    let store = RepoStore::open_from_config(&loaded.config)?;
    let summary = store.summary()?;
    let scheduler = SchedulerService::new();
    Ok(RuntimeStatus {
        protocol_version: PROTOCOL_VERSION.to_string(),
        config_version: CONFIG_VERSION,
        runtime_root: loaded.config.directories.runtime_root.display().to_string(),
        state_dir: loaded.config.directories.state_dir.display().to_string(),
        socket_path: loaded.config.transport.socket_path.display().to_string(),
        daemon_state: DaemonLifecycleState::Stopped,
        pid: read_pid_file(loaded),
        transport: TransportSummary {
            kind: loaded.config.transport.kind.clone(),
            socket_path: Some(loaded.config.transport.socket_path.display().to_string()),
            connected_clients: 0,
        },
        repo_count: summary.repo_count,
        manifest_count: summary.manifest_count,
        scheduler: scheduler.status(),
        parser: None,
        symbol_index: None,
        impact: Some(scan_impact_runtime_status(loaded)?),
        semantic: Some(scan_semantic_runtime_status(loaded)?),
    })
}

fn require_socket_transport(loaded: &hyperindex_config::LoadedConfig) -> HyperindexResult<()> {
    if loaded.config.transport.kind != TransportKind::UnixSocket {
        return Err(HyperindexError::Message(
            "daemon lifecycle commands require transport.kind = \"unix_socket\"".to_string(),
        ));
    }
    Ok(())
}

fn render_lifecycle_result(
    status_label: &str,
    message: &str,
    status: &RuntimeStatus,
    log_path: Option<std::path::PathBuf>,
    json_output: bool,
) -> HyperindexResult<String> {
    if json_output {
        return Ok(serde_json::to_string_pretty(&json!({
            "status": status_label,
            "message": message,
            "runtime": status,
            "pid_file": daemon_pid_file_path_from_status(status),
            "log_path": log_path.as_ref().map(|path| path.display().to_string()),
        }))
        .unwrap());
    }

    Ok([
        format!("status: {status_label}"),
        format!("message: {message}"),
        format!("daemon_state: {:?}", status.daemon_state).to_lowercase(),
        format!(
            "pid: {}",
            status
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!("socket_path: {}", status.socket_path),
        format!("repo_count: {}", status.repo_count),
        format!("manifest_count: {}", status.manifest_count),
        format!(
            "parse_build_count: {}",
            status
                .parser
                .as_ref()
                .map(|parser| parser.build_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "indexed_snapshot_count: {}",
            status
                .symbol_index
                .as_ref()
                .map(|symbol| symbol.indexed_snapshot_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "impact_materialized_snapshot_count: {}",
            status
                .impact
                .as_ref()
                .map(|impact| impact.materialized_snapshot_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "impact_ready_build_count: {}",
            status
                .impact
                .as_ref()
                .map(|impact| impact.ready_build_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "impact_stale_build_count: {}",
            status
                .impact
                .as_ref()
                .map(|impact| impact.stale_build_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "semantic_materialized_snapshot_count: {}",
            status
                .semantic
                .as_ref()
                .map(|semantic| semantic.materialized_snapshot_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "semantic_ready_build_count: {}",
            status
                .semantic
                .as_ref()
                .map(|semantic| semantic.ready_build_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "log_path: {}",
            log_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
    ]
    .join("\n"))
}

fn render_status_report(
    source: &str,
    reachable: bool,
    message: &str,
    status: &RuntimeStatus,
    log_path: Option<std::path::PathBuf>,
    json_output: bool,
) -> HyperindexResult<String> {
    if json_output {
        return Ok(serde_json::to_string_pretty(&json!({
            "source": source,
            "reachable": reachable,
            "message": message,
            "runtime": status,
            "pid_file": daemon_pid_file_path_from_status(status),
            "log_path": log_path.as_ref().map(|path| path.display().to_string()),
        }))
        .unwrap());
    }

    Ok([
        format!("source: {source}"),
        format!("reachable: {reachable}"),
        format!("message: {message}"),
        format!("daemon_state: {:?}", status.daemon_state).to_lowercase(),
        format!(
            "pid: {}",
            status
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!("socket_path: {}", status.socket_path),
        format!("repo_count: {}", status.repo_count),
        format!("manifest_count: {}", status.manifest_count),
        format!(
            "parse_build_count: {}",
            status
                .parser
                .as_ref()
                .map(|parser| parser.build_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "indexed_snapshot_count: {}",
            status
                .symbol_index
                .as_ref()
                .map(|symbol| symbol.indexed_snapshot_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "impact_materialized_snapshot_count: {}",
            status
                .impact
                .as_ref()
                .map(|impact| impact.materialized_snapshot_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "impact_ready_build_count: {}",
            status
                .impact
                .as_ref()
                .map(|impact| impact.ready_build_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "impact_stale_build_count: {}",
            status
                .impact
                .as_ref()
                .map(|impact| impact.stale_build_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "semantic_materialized_snapshot_count: {}",
            status
                .semantic
                .as_ref()
                .map(|semantic| semantic.materialized_snapshot_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "semantic_ready_build_count: {}",
            status
                .semantic
                .as_ref()
                .map(|semantic| semantic.ready_build_count.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!("connected_clients: {}", status.transport.connected_clients),
        format!(
            "log_path: {}",
            log_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
    ]
    .join("\n"))
}

fn daemon_pid_file_path_from_status(status: &RuntimeStatus) -> String {
    Path::new(&status.runtime_root)
        .join("hyperd.pid")
        .display()
        .to_string()
}
