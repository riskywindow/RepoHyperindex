use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::api::{ApiMethod, DaemonRequest, DaemonResponse};
use hyperindex_protocol::errors::ProtocolError;
use tracing::{error, info};

use crate::handlers::HandlerRegistry;
use crate::runtime::RuntimeState;

pub struct DaemonServer {
    runtime: RuntimeState,
    handlers: HandlerRegistry,
}

impl DaemonServer {
    pub fn new(runtime: RuntimeState) -> Self {
        let handlers = HandlerRegistry::new(runtime.state_manager.clone());
        Self { runtime, handlers }
    }

    pub async fn serve(&self) -> HyperindexResult<()> {
        match self.runtime.loaded_config.config.transport.kind {
            hyperindex_protocol::config::TransportKind::UnixSocket => {
                self.serve_unix_socket().await
            }
            hyperindex_protocol::config::TransportKind::Stdio => self.serve_stdio().await,
        }
    }

    pub async fn handle_raw_request(&self, raw: &[u8]) -> HyperindexResult<Vec<u8>> {
        self.runtime.state_manager.connected_client_opened()?;
        if matches!(
            self.runtime.state_manager.lifecycle()?,
            hyperindex_protocol::status::DaemonLifecycleState::Starting
        ) {
            self.runtime
                .state_manager
                .set_lifecycle(hyperindex_protocol::status::DaemonLifecycleState::Running)?;
        }
        let result = response_bytes_from_raw(raw, &self.handlers).await;
        self.runtime.state_manager.connected_client_closed()?;
        result
    }

    async fn serve_unix_socket(&self) -> HyperindexResult<()> {
        let _artifacts = self.runtime.acquire_runtime_artifacts()?;
        let listener = bind_listener(&self.runtime.socket_path())?;
        self.runtime
            .state_manager
            .set_lifecycle(hyperindex_protocol::status::DaemonLifecycleState::Running)?;
        info!(
            socket_path = %self.runtime.loaded_config.config.transport.socket_path.display(),
            pid = %std::process::id(),
            "hyperd runtime listening"
        );

        let mut shutdown_rx = self.runtime.state_manager.shutdown_receiver();
        let handlers = self.handlers.clone();
        let state_manager = self.runtime.state_manager.clone();
        let max_frame_bytes = self.runtime.loaded_config.config.transport.max_frame_bytes;

        tokio::task::spawn_blocking(move || {
            serve_blocking(
                listener,
                handlers,
                state_manager,
                &mut shutdown_rx,
                max_frame_bytes,
            )
        })
        .await
        .map_err(|error| {
            HyperindexError::Message(format!("daemon serve join failed: {error}"))
        })??;

        self.runtime
            .state_manager
            .set_lifecycle(hyperindex_protocol::status::DaemonLifecycleState::Stopping)?;
        self.runtime
            .state_manager
            .set_lifecycle(hyperindex_protocol::status::DaemonLifecycleState::Stopped)?;
        info!("hyperd runtime stopped cleanly");
        Ok(())
    }

    async fn serve_stdio(&self) -> HyperindexResult<()> {
        self.runtime
            .state_manager
            .set_lifecycle(hyperindex_protocol::status::DaemonLifecycleState::Running)?;
        let raw = tokio::task::spawn_blocking(|| {
            let mut raw = Vec::new();
            std::io::stdin()
                .read_to_end(&mut raw)
                .map_err(|error| HyperindexError::Message(format!("stdin read failed: {error}")))?;
            Ok::<_, HyperindexError>(raw)
        })
        .await
        .map_err(|error| HyperindexError::Message(format!("stdio read join failed: {error}")))??;

        let encoded = self.handle_raw_request(&raw).await?;
        tokio::task::spawn_blocking(move || {
            std::io::stdout()
                .write_all(&encoded)
                .map_err(|error| HyperindexError::Message(format!("stdout write failed: {error}")))
        })
        .await
        .map_err(|error| HyperindexError::Message(format!("stdio write join failed: {error}")))??;
        self.runtime
            .state_manager
            .set_lifecycle(hyperindex_protocol::status::DaemonLifecycleState::Stopped)?;
        Ok(())
    }
}

fn serve_blocking(
    listener: UnixListener,
    handlers: HandlerRegistry,
    state_manager: std::sync::Arc<crate::state::DaemonStateManager>,
    shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
    max_frame_bytes: usize,
) -> HyperindexResult<()> {
    listener
        .set_nonblocking(true)
        .map_err(|error| HyperindexError::Message(format!("set_nonblocking failed: {error}")))?;

    loop {
        if shutdown_rx.has_changed().unwrap_or(false) && *shutdown_rx.borrow() {
            break;
        }

        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = state_manager.connected_client_opened() {
                    error!("failed to record client open: {error}");
                }
                let result = serve_connection(stream, max_frame_bytes, &handlers);
                if let Err(error) = state_manager.connected_client_closed() {
                    error!("failed to record client close: {error}");
                }
                if let Err(error) = result {
                    error!("connection task failed: {error}");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                return Err(HyperindexError::Message(format!(
                    "unix socket accept failed: {error}"
                )));
            }
        }
    }

    Ok(())
}

fn serve_connection(
    mut stream: UnixStream,
    max_frame_bytes: usize,
    handlers: &HandlerRegistry,
) -> HyperindexResult<()> {
    let raw = read_request_bytes(&mut stream, max_frame_bytes)?;
    let encoded =
        tokio::runtime::Handle::current().block_on(response_bytes_from_raw(&raw, handlers))?;
    stream
        .write_all(&encoded)
        .map_err(|error| HyperindexError::Message(format!("response write failed: {error}")))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| HyperindexError::Message(format!("stream shutdown failed: {error}")))?;
    Ok(())
}

fn read_request_bytes(
    stream: &mut UnixStream,
    max_frame_bytes: usize,
) -> HyperindexResult<Vec<u8>> {
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| HyperindexError::Message(format!("request read failed: {error}")))?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
        if raw.len() > max_frame_bytes {
            return Err(HyperindexError::Message(format!(
                "request exceeded max frame size of {max_frame_bytes} bytes"
            )));
        }
    }
    Ok(raw)
}

async fn response_bytes_from_raw(
    raw: &[u8],
    handlers: &HandlerRegistry,
) -> HyperindexResult<Vec<u8>> {
    let response = match decode_request(raw) {
        Ok(request) => handlers.dispatch(request).await,
        Err((request_id, method, error)) => DaemonResponse::error(request_id, method, error),
    };

    serde_json::to_vec(&response).map_err(|error| {
        HyperindexError::Message(format!("response serialization failed: {error}"))
    })
}

fn decode_request(raw: &[u8]) -> Result<DaemonRequest, (String, ApiMethod, ProtocolError)> {
    match serde_json::from_slice::<DaemonRequest>(raw) {
        Ok(request) => Ok(request),
        Err(error) => {
            let (request_id, method) = request_metadata(raw);
            Err((
                request_id,
                method,
                ProtocolError::invalid_request(format!("invalid daemon request: {error}")),
            ))
        }
    }
}

fn request_metadata(raw: &[u8]) -> (String, ApiMethod) {
    let value = serde_json::from_slice::<serde_json::Value>(raw).ok();
    let request_id = value
        .as_ref()
        .and_then(|value| value.get("request_id"))
        .and_then(|value| value.as_str())
        .unwrap_or("invalid-request")
        .to_string();
    let method = value
        .as_ref()
        .and_then(|value| value.get("method"))
        .and_then(|value| value.as_str())
        .and_then(parse_api_method)
        .unwrap_or(ApiMethod::Version);
    (request_id, method)
}

fn parse_api_method(raw: &str) -> Option<ApiMethod> {
    Some(match raw {
        "health" => ApiMethod::Health,
        "version" => ApiMethod::Version,
        "daemon_status" => ApiMethod::DaemonStatus,
        "repos_add" => ApiMethod::ReposAdd,
        "repos_list" => ApiMethod::ReposList,
        "repos_remove" => ApiMethod::ReposRemove,
        "repos_show" => ApiMethod::ReposShow,
        "repo_status" => ApiMethod::RepoStatus,
        "watch_status" => ApiMethod::WatchStatus,
        "watch_events" => ApiMethod::WatchEvents,
        "snapshots_create" => ApiMethod::SnapshotsCreate,
        "snapshots_show" => ApiMethod::SnapshotsShow,
        "snapshots_list" => ApiMethod::SnapshotsList,
        "snapshots_diff" => ApiMethod::SnapshotsDiff,
        "snapshots_read_file" => ApiMethod::SnapshotsReadFile,
        "buffers_set" => ApiMethod::BuffersSet,
        "buffers_clear" => ApiMethod::BuffersClear,
        "buffers_list" => ApiMethod::BuffersList,
        "planner_status" => ApiMethod::PlannerStatus,
        "planner_query" => ApiMethod::PlannerQuery,
        "planner_explain" => ApiMethod::PlannerExplain,
        "planner_capabilities" => ApiMethod::PlannerCapabilities,
        "impact_status" => ApiMethod::ImpactStatus,
        "impact_analyze" => ApiMethod::ImpactAnalyze,
        "impact_explain" => ApiMethod::ImpactExplain,
        "shutdown" => ApiMethod::Shutdown,
        _ => return None,
    })
}

fn bind_listener(socket_path: &Path) -> HyperindexResult<UnixListener> {
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            HyperindexError::Message(format!("failed to create {}: {error}", parent.display()))
        })?;
    }
    UnixListener::bind(socket_path)
        .map_err(|error| HyperindexError::Message(format!("bind failed: {error}")))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tempfile::tempdir;

    use hyperindex_config::write_default_config;
    use hyperindex_protocol::api::{
        DaemonRequest, DaemonResponse, RequestBody, ResponseBody, SuccessPayload,
    };
    use hyperindex_protocol::buffers::BufferSetParams;
    use hyperindex_protocol::errors::ErrorCode;
    use hyperindex_protocol::impact::{
        ImpactAnalyzeParams, ImpactChangeScenario, ImpactEntityRef, ImpactExplainParams,
        ImpactStatusParams, ImpactTargetRef,
    };
    use hyperindex_protocol::repo::ReposAddParams;
    use hyperindex_protocol::semantic::{
        SemanticBuildParams, SemanticInspectChunkParams, SemanticQueryFilters, SemanticQueryParams,
        SemanticQueryText, SemanticRerankMode, SemanticStatusParams,
    };
    use hyperindex_protocol::snapshot::{
        SnapshotCreateParams, SnapshotDiffParams, SnapshotReadFileParams,
        SnapshotResolvedFileSourceKind,
    };
    use hyperindex_protocol::status::{DaemonStatusParams, EmptyParams, ShutdownParams};
    use hyperindex_protocol::status::{HealthState, ShutdownResponse};
    use hyperindex_protocol::symbols::{
        DefinitionLookupParams, ParseBuildParams, ReferenceLookupParams, SymbolIndexBuildParams,
        SymbolLocationSelector, SymbolResolveParams, SymbolSearchMode, SymbolSearchParams,
        SymbolSearchQuery, SymbolShowParams,
    };

    use crate::runtime::RuntimeState;
    use crate::server::DaemonServer;

    #[test]
    fn daemon_server_constructs_from_bootstrap_state() {
        let tempdir = tempdir().unwrap();
        let config_path = tempdir.path().join("config.toml");
        write_default_config(Some(&config_path), false).unwrap();
        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let _server = DaemonServer::new(runtime);
    }

    #[tokio::test]
    async fn daemon_smoke_flow_serves_repo_buffer_snapshot_requests() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);

        let mut config = hyperindex_config::load_or_default(Some(&config_path))
            .unwrap()
            .config;
        config.transport.kind = hyperindex_protocol::config::TransportKind::Stdio;
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let server = DaemonServer::new(runtime);

        let health = send_request(
            &server,
            DaemonRequest::new("req-health", RequestBody::Health(EmptyParams::default())),
        )
        .await;
        match health.body {
            ResponseBody::Success {
                result: SuccessPayload::Health(response),
            } => assert_eq!(response.status, HealthState::Ok),
            other => panic!("unexpected health response: {other:?}"),
        }

        let added = send_request(
            &server,
            DaemonRequest::new(
                "req-add",
                RequestBody::ReposAdd(ReposAddParams {
                    repo_root: repo_root.display().to_string(),
                    display_name: Some("Daemon Repo".to_string()),
                    notes: vec!["smoke".to_string()],
                    ignore_patterns: vec!["dist/**".to_string()],
                    watch_on_add: true,
                }),
            ),
        )
        .await;
        let repo_id = match added.body {
            ResponseBody::Success {
                result: SuccessPayload::ReposAdd(response),
            } => response.repo.repo_id,
            other => panic!("unexpected add response: {other:?}"),
        };

        let status = send_request(
            &server,
            DaemonRequest::new(
                "req-status",
                RequestBody::DaemonStatus(DaemonStatusParams::default()),
            ),
        )
        .await;
        match status.body {
            ResponseBody::Success {
                result: SuccessPayload::DaemonStatus(response),
            } => {
                assert_eq!(response.repo_count, 1);
                assert!(response.transport.connected_clients <= 1);
                assert_eq!(
                    response.parser.as_ref().map(|parser| parser.build_count),
                    Some(0)
                );
                assert_eq!(
                    response
                        .symbol_index
                        .as_ref()
                        .map(|symbol_index| symbol_index.indexed_snapshot_count),
                    Some(0)
                );
                assert_eq!(
                    response
                        .impact
                        .as_ref()
                        .map(|impact| impact.materialized_snapshot_count),
                    Some(0)
                );
            }
            other => panic!("unexpected status response: {other:?}"),
        }

        let repo_status = send_request(
            &server,
            DaemonRequest::new(
                "req-repo-status",
                RequestBody::RepoStatus(hyperindex_protocol::repo::RepoStatusParams {
                    repo_id: repo_id.clone(),
                }),
            ),
        )
        .await;
        match repo_status.body {
            ResponseBody::Success {
                result: SuccessPayload::RepoStatus(response),
            } => assert!(response.watch_attached),
            other => panic!("unexpected repo status response: {other:?}"),
        }

        let snapshot_clean = send_request(
            &server,
            DaemonRequest::new(
                "req-snapshot-clean",
                RequestBody::SnapshotsCreate(SnapshotCreateParams {
                    repo_id: repo_id.clone(),
                    include_working_tree: true,
                    buffer_ids: Vec::new(),
                }),
            ),
        )
        .await;
        let clean_snapshot_id = match snapshot_clean.body {
            ResponseBody::Success {
                result: SuccessPayload::SnapshotsCreate(response),
            } => response.snapshot.snapshot_id,
            other => panic!("unexpected clean snapshot response: {other:?}"),
        };

        let parse_build = send_request(
            &server,
            DaemonRequest::new(
                "req-parse-build",
                RequestBody::ParseBuild(ParseBuildParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    force: false,
                }),
            ),
        )
        .await;
        match parse_build.body {
            ResponseBody::Success {
                result: SuccessPayload::ParseBuild(response),
            } => assert_eq!(response.build.counts.reused_file_count, 0),
            other => panic!("unexpected parse build response: {other:?}"),
        }

        let symbol_build = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-build",
                RequestBody::SymbolIndexBuild(SymbolIndexBuildParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    force: false,
                }),
            ),
        )
        .await;
        match symbol_build.body {
            ResponseBody::Success {
                result: SuccessPayload::SymbolIndexBuild(response),
            } => {
                assert_eq!(response.build.stats.file_count, 1);
                assert_eq!(response.build.refresh_mode.as_deref(), Some("full_rebuild"));
            }
            other => panic!("unexpected symbol build response: {other:?}"),
        }

        let search = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-search",
                RequestBody::SymbolSearch(SymbolSearchParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    query: SymbolSearchQuery {
                        text: "createSession".to_string(),
                        mode: SymbolSearchMode::Exact,
                        kinds: Vec::new(),
                        path_prefix: None,
                    },
                    limit: 5,
                }),
            ),
        )
        .await;
        let clean_symbol_id = match search.body {
            ResponseBody::Success {
                result: SuccessPayload::SymbolSearch(response),
            } => {
                assert_eq!(response.hits.len(), 1);
                response.hits[0].symbol.symbol_id.clone()
            }
            other => panic!("unexpected symbol search response: {other:?}"),
        };

        let show = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-show",
                RequestBody::SymbolShow(SymbolShowParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    symbol_id: clean_symbol_id.clone(),
                }),
            ),
        )
        .await;
        match show.body {
            ResponseBody::Success {
                result: SuccessPayload::SymbolShow(response),
            } => assert_eq!(response.symbol.display_name, "createSession"),
            other => panic!("unexpected symbol show response: {other:?}"),
        }

        let definitions = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-defs",
                RequestBody::DefinitionLookup(DefinitionLookupParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    symbol_id: clean_symbol_id.clone(),
                }),
            ),
        )
        .await;
        match definitions.body {
            ResponseBody::Success {
                result: SuccessPayload::DefinitionLookup(response),
            } => assert_eq!(response.definitions.len(), 1),
            other => panic!("unexpected definition response: {other:?}"),
        }

        let references = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-refs",
                RequestBody::ReferenceLookup(ReferenceLookupParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    symbol_id: clean_symbol_id.clone(),
                    limit: None,
                }),
            ),
        )
        .await;
        match references.body {
            ResponseBody::Success {
                result: SuccessPayload::ReferenceLookup(response),
            } => assert!(response.references.len() >= 1),
            other => panic!("unexpected reference response: {other:?}"),
        };

        let buffer_set = send_request(
            &server,
            DaemonRequest::new(
                "req-buffer",
                RequestBody::BuffersSet(BufferSetParams {
                    repo_id: repo_id.clone(),
                    buffer_id: "buffer-1".to_string(),
                    path: "src/app.ts".to_string(),
                    version: 1,
                    language: Some("typescript".to_string()),
                    contents: "export function createBufferedSession() {\n  return \"buffer\";\n}\n\nexport function run() {\n  return createBufferedSession();\n}\n".to_string(),
                }),
            ),
        )
        .await;
        match buffer_set.body {
            ResponseBody::Success {
                result: SuccessPayload::BuffersSet(response),
            } => assert_eq!(response.buffer.buffer_id, "buffer-1"),
            other => panic!("unexpected buffer response: {other:?}"),
        }

        let snapshot_buffered = send_request(
            &server,
            DaemonRequest::new(
                "req-snapshot-buffered",
                RequestBody::SnapshotsCreate(SnapshotCreateParams {
                    repo_id: repo_id.clone(),
                    include_working_tree: true,
                    buffer_ids: vec!["buffer-1".to_string()],
                }),
            ),
        )
        .await;
        let buffered_snapshot_id = match snapshot_buffered.body {
            ResponseBody::Success {
                result: SuccessPayload::SnapshotsCreate(response),
            } => response.snapshot.snapshot_id,
            other => panic!("unexpected buffered snapshot response: {other:?}"),
        };

        let refreshed_build = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-build-buffered",
                RequestBody::SymbolIndexBuild(SymbolIndexBuildParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: buffered_snapshot_id.clone(),
                    force: false,
                }),
            ),
        )
        .await;
        match refreshed_build.body {
            ResponseBody::Success {
                result: SuccessPayload::SymbolIndexBuild(response),
            } => assert_eq!(response.build.refresh_mode.as_deref(), Some("incremental")),
            other => panic!("unexpected refreshed symbol build response: {other:?}"),
        }

        let refreshed_search = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-search-buffered",
                RequestBody::SymbolSearch(SymbolSearchParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: buffered_snapshot_id.clone(),
                    query: SymbolSearchQuery {
                        text: "createBufferedSession".to_string(),
                        mode: SymbolSearchMode::Exact,
                        kinds: Vec::new(),
                        path_prefix: None,
                    },
                    limit: 5,
                }),
            ),
        )
        .await;
        let buffered_symbol_id = match refreshed_search.body {
            ResponseBody::Success {
                result: SuccessPayload::SymbolSearch(response),
            } => {
                assert_eq!(response.hits.len(), 1);
                response.hits[0].symbol.symbol_id.clone()
            }
            other => panic!("unexpected buffered search response: {other:?}"),
        };

        let resolve = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-resolve",
                RequestBody::SymbolResolve(SymbolResolveParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: buffered_snapshot_id.clone(),
                    selector: SymbolLocationSelector::LineColumn {
                        path: "src/app.ts".to_string(),
                        line: 6,
                        column: 10,
                    },
                }),
            ),
        )
        .await;
        match resolve.body {
            ResponseBody::Success {
                result: SuccessPayload::SymbolResolve(response),
            } => assert_eq!(
                response
                    .resolution
                    .as_ref()
                    .map(|resolution| resolution.symbol.symbol_id.clone()),
                Some(buffered_symbol_id.clone())
            ),
            other => panic!("unexpected resolve response: {other:?}"),
        }

        let diff = send_request(
            &server,
            DaemonRequest::new(
                "req-diff",
                RequestBody::SnapshotsDiff(SnapshotDiffParams {
                    left_snapshot_id: clean_snapshot_id,
                    right_snapshot_id: buffered_snapshot_id.clone(),
                }),
            ),
        )
        .await;
        match diff.body {
            ResponseBody::Success {
                result: SuccessPayload::SnapshotsDiff(response),
            } => assert_eq!(response.buffer_only_changed_paths, vec!["src/app.ts"]),
            other => panic!("unexpected diff response: {other:?}"),
        }

        let read_file = send_request(
            &server,
            DaemonRequest::new(
                "req-read-file",
                RequestBody::SnapshotsReadFile(SnapshotReadFileParams {
                    snapshot_id: buffered_snapshot_id,
                    path: "src/app.ts".to_string(),
                }),
            ),
        )
        .await;
        match read_file.body {
            ResponseBody::Success {
                result: SuccessPayload::SnapshotsReadFile(response),
            } => {
                assert_eq!(
                    response.contents,
                    "export function createBufferedSession() {\n  return \"buffer\";\n}\n\nexport function run() {\n  return createBufferedSession();\n}\n"
                );
                assert_eq!(
                    response.resolved_from.kind,
                    SnapshotResolvedFileSourceKind::BufferOverlay
                );
                assert_eq!(
                    response.resolved_from.buffer_id.as_deref(),
                    Some("buffer-1")
                );
            }
            other => panic!("unexpected read-file response: {other:?}"),
        }

        let clear = send_request(
            &server,
            DaemonRequest::new(
                "req-clear",
                RequestBody::BuffersClear(hyperindex_protocol::buffers::BufferClearParams {
                    repo_id: repo_id.clone(),
                    buffer_id: "buffer-1".to_string(),
                }),
            ),
        )
        .await;
        match clear.body {
            ResponseBody::Success {
                result: SuccessPayload::BuffersClear(response),
            } => assert!(response.cleared),
            other => panic!("unexpected buffer clear response: {other:?}"),
        }

        let shutdown = send_request(
            &server,
            DaemonRequest::new(
                "req-shutdown",
                RequestBody::Shutdown(ShutdownParams {
                    graceful: true,
                    timeout_ms: Some(1_000),
                }),
            ),
        )
        .await;
        match shutdown.body {
            ResponseBody::Success {
                result: SuccessPayload::Shutdown(ShutdownResponse { accepted, .. }),
            } => assert!(accepted),
            other => panic!("unexpected shutdown response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn daemon_semantic_north_star_query_refreshes_after_overlay() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("repo");
        init_impact_repo(&repo_root);

        let mut config = hyperindex_config::load_or_default(Some(&config_path))
            .unwrap()
            .config;
        config.transport.kind = hyperindex_protocol::config::TransportKind::Stdio;
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let server = DaemonServer::new(runtime);

        let added = send_request(
            &server,
            DaemonRequest::new(
                "req-add",
                RequestBody::ReposAdd(ReposAddParams {
                    repo_root: repo_root.display().to_string(),
                    display_name: Some("Semantic Repo".to_string()),
                    notes: Vec::new(),
                    ignore_patterns: Vec::new(),
                    watch_on_add: true,
                }),
            ),
        )
        .await;
        let repo_id = match added.body {
            ResponseBody::Success {
                result: SuccessPayload::ReposAdd(response),
            } => response.repo.repo_id,
            other => panic!("unexpected add response: {other:?}"),
        };

        let snapshot_clean = send_request(
            &server,
            DaemonRequest::new(
                "req-snapshot-clean",
                RequestBody::SnapshotsCreate(SnapshotCreateParams {
                    repo_id: repo_id.clone(),
                    include_working_tree: true,
                    buffer_ids: Vec::new(),
                }),
            ),
        )
        .await;
        let clean_snapshot_id = match snapshot_clean.body {
            ResponseBody::Success {
                result: SuccessPayload::SnapshotsCreate(response),
            } => response.snapshot.snapshot_id,
            other => panic!("unexpected clean snapshot response: {other:?}"),
        };

        let semantic_status_before = send_request(
            &server,
            DaemonRequest::new(
                "req-semantic-status-before",
                RequestBody::SemanticStatus(SemanticStatusParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    build_id: None,
                }),
            ),
        )
        .await;
        match semantic_status_before.body {
            ResponseBody::Success {
                result: SuccessPayload::SemanticStatus(response),
            } => assert!(matches!(
                response.state,
                hyperindex_protocol::semantic::SemanticAnalysisState::NotReady
            )),
            other => panic!("unexpected semantic status response: {other:?}"),
        }

        let symbol_build = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-build-clean",
                RequestBody::SymbolIndexBuild(SymbolIndexBuildParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    force: false,
                }),
            ),
        )
        .await;
        match symbol_build.body {
            ResponseBody::Success {
                result: SuccessPayload::SymbolIndexBuild(response),
            } => assert_eq!(response.build.refresh_mode.as_deref(), Some("full_rebuild")),
            other => panic!("unexpected clean symbol build response: {other:?}"),
        }

        let semantic_build = send_request(
            &server,
            DaemonRequest::new(
                "req-semantic-build-clean",
                RequestBody::SemanticBuild(SemanticBuildParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    force: false,
                }),
            ),
        )
        .await;
        match semantic_build.body {
            ResponseBody::Success {
                result: SuccessPayload::SemanticBuild(response),
            } => {
                assert_eq!(response.build.refresh_mode.as_deref(), Some("full_rebuild"));
                assert!(!response.build.loaded_from_existing_build);
                assert!(
                    response
                        .build
                        .refresh_stats
                        .as_ref()
                        .unwrap()
                        .chunks_rebuilt
                        > 0
                );
            }
            other => panic!("unexpected clean semantic build response: {other:?}"),
        }

        let semantic_status_ready = send_request(
            &server,
            DaemonRequest::new(
                "req-semantic-status-ready",
                RequestBody::SemanticStatus(SemanticStatusParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    build_id: None,
                }),
            ),
        )
        .await;
        match semantic_status_ready.body {
            ResponseBody::Success {
                result: SuccessPayload::SemanticStatus(response),
            } => assert!(matches!(
                response.state,
                hyperindex_protocol::semantic::SemanticAnalysisState::Ready
            )),
            other => panic!("unexpected semantic ready status response: {other:?}"),
        }

        let semantic_query_clean = send_request(
            &server,
            DaemonRequest::new(
                "req-semantic-query-clean",
                RequestBody::SemanticQuery(SemanticQueryParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    query: SemanticQueryText {
                        text: "where do we invalidate sessions?".to_string(),
                    },
                    filters: SemanticQueryFilters::default(),
                    limit: 4,
                    rerank_mode: SemanticRerankMode::Hybrid,
                }),
            ),
        )
        .await;
        let (top_clean_chunk_id, clean_hit_signature) = match semantic_query_clean.body {
            ResponseBody::Success {
                result: SuccessPayload::SemanticQuery(response),
            } => {
                let top_hit = response.hits.first().expect("expected semantic hit");
                assert_eq!(top_hit.chunk.path, "packages/auth/src/session/service.ts");
                assert_eq!(
                    top_hit.chunk.symbol_display_name.as_deref(),
                    Some("invalidateSession")
                );
                assert!(
                    response
                        .hits
                        .iter()
                        .any(|hit| hit.chunk.path == "packages/api/src/routes/logout.ts")
                );
                (
                    top_hit.chunk.chunk_id.clone(),
                    response
                        .hits
                        .iter()
                        .map(|hit| format!("{}:{}", hit.chunk.path, hit.chunk.chunk_id.0))
                        .collect::<Vec<_>>(),
                )
            }
            other => panic!("unexpected clean semantic query response: {other:?}"),
        };

        let inspect_clean = send_request(
            &server,
            DaemonRequest::new(
                "req-semantic-inspect-clean",
                RequestBody::SemanticInspectChunk(SemanticInspectChunkParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: clean_snapshot_id.clone(),
                    chunk_id: top_clean_chunk_id,
                    build_id: None,
                }),
            ),
        )
        .await;
        match inspect_clean.body {
            ResponseBody::Success {
                result: SuccessPayload::SemanticInspectChunk(response),
            } => {
                assert!(response.chunk.serialized_text.contains("invalidateSession"));
                assert_eq!(
                    response.chunk.metadata.path,
                    "packages/auth/src/session/service.ts"
                );
            }
            other => panic!("unexpected semantic inspect response: {other:?}"),
        }

        let buffer_set = send_request(
            &server,
            DaemonRequest::new(
                "req-buffer",
                RequestBody::BuffersSet(BufferSetParams {
                    repo_id: repo_id.clone(),
                    buffer_id: "buffer-1".to_string(),
                    path: "packages/api/src/routes/logout.ts".to_string(),
                    version: 1,
                    language: Some("typescript".to_string()),
                    contents:
                        "export function logout(userId: string) {\n  return `local:${userId}`;\n}\n"
                            .to_string(),
                }),
            ),
        )
        .await;
        match buffer_set.body {
            ResponseBody::Success {
                result: SuccessPayload::BuffersSet(response),
            } => assert_eq!(response.buffer.buffer_id, "buffer-1"),
            other => panic!("unexpected buffer response: {other:?}"),
        }

        let snapshot_buffered = send_request(
            &server,
            DaemonRequest::new(
                "req-snapshot-buffered",
                RequestBody::SnapshotsCreate(SnapshotCreateParams {
                    repo_id: repo_id.clone(),
                    include_working_tree: true,
                    buffer_ids: vec!["buffer-1".to_string()],
                }),
            ),
        )
        .await;
        let buffered_snapshot_id = match snapshot_buffered.body {
            ResponseBody::Success {
                result: SuccessPayload::SnapshotsCreate(response),
            } => response.snapshot.snapshot_id,
            other => panic!("unexpected buffered snapshot response: {other:?}"),
        };

        let symbol_build_buffered = send_request(
            &server,
            DaemonRequest::new(
                "req-symbol-build-buffered",
                RequestBody::SymbolIndexBuild(SymbolIndexBuildParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: buffered_snapshot_id.clone(),
                    force: false,
                }),
            ),
        )
        .await;
        match symbol_build_buffered.body {
            ResponseBody::Success {
                result: SuccessPayload::SymbolIndexBuild(response),
            } => assert_eq!(response.build.refresh_mode.as_deref(), Some("incremental")),
            other => panic!("unexpected buffered symbol build response: {other:?}"),
        }

        let semantic_build_buffered = send_request(
            &server,
            DaemonRequest::new(
                "req-semantic-build-buffered",
                RequestBody::SemanticBuild(SemanticBuildParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: buffered_snapshot_id.clone(),
                    force: false,
                }),
            ),
        )
        .await;
        match semantic_build_buffered.body {
            ResponseBody::Success {
                result: SuccessPayload::SemanticBuild(response),
            } => {
                let stats = response.build.refresh_stats.unwrap();
                assert_eq!(response.build.refresh_mode.as_deref(), Some("incremental"));
                assert!(!response.build.loaded_from_existing_build);
                assert_eq!(stats.files_touched, 1);
                assert!(stats.chunks_rebuilt > 0);
                assert!(
                    stats.chunks_rebuilt
                        < response
                            .build
                            .manifest
                            .as_ref()
                            .unwrap()
                            .indexed_chunk_count
                );
            }
            other => panic!("unexpected buffered semantic build response: {other:?}"),
        }

        let semantic_query = send_request(
            &server,
            DaemonRequest::new(
                "req-semantic-query-buffered",
                RequestBody::SemanticQuery(SemanticQueryParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: buffered_snapshot_id.clone(),
                    query: SemanticQueryText {
                        text: "where do we invalidate sessions?".to_string(),
                    },
                    filters: SemanticQueryFilters::default(),
                    limit: 4,
                    rerank_mode: SemanticRerankMode::Hybrid,
                }),
            ),
        )
        .await;
        match semantic_query.body {
            ResponseBody::Success {
                result: SuccessPayload::SemanticQuery(response),
            } => {
                let top_hit = response.hits.first().expect("expected semantic hit");
                assert_eq!(top_hit.chunk.path, "packages/auth/src/session/service.ts");
                assert_eq!(
                    top_hit.chunk.symbol_display_name.as_deref(),
                    Some("invalidateSession")
                );
                assert!(
                    response
                        .hits
                        .iter()
                        .any(|hit| hit.chunk.path == "packages/auth/src/session/service.ts")
                );
                let buffered_hit_signature = response
                    .hits
                    .iter()
                    .map(|hit| format!("{}:{}", hit.chunk.path, hit.chunk.chunk_id.0))
                    .collect::<Vec<_>>();
                assert_ne!(buffered_hit_signature, clean_hit_signature);
            }
            other => panic!("unexpected buffered semantic query response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn daemon_impact_request_requires_a_known_repo() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let mut config = hyperindex_config::load_or_default(Some(&config_path))
            .unwrap()
            .config;
        config.transport.kind = hyperindex_protocol::config::TransportKind::Stdio;
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let server = DaemonServer::new(runtime);

        let response = send_request(
            &server,
            DaemonRequest::new(
                "req-impact",
                RequestBody::ImpactAnalyze(ImpactAnalyzeParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: "snap-1".to_string(),
                    target: ImpactTargetRef::Symbol {
                        value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                        symbol_id: None,
                        path: Some("packages/auth/src/session/service.ts".to_string()),
                    },
                    change_hint: ImpactChangeScenario::ModifyBehavior,
                    limit: 20,
                    include_transitive: true,
                    include_reason_paths: true,
                    max_transitive_depth: None,
                    max_nodes_visited: None,
                    max_edges_traversed: None,
                    max_candidates_considered: None,
                }),
            ),
        )
        .await;

        match response.body {
            ResponseBody::Error { error, .. } => assert_eq!(error.code, ErrorCode::RepoNotFound),
            other => panic!("unexpected impact response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn daemon_impact_status_request_returns_contract_metadata() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let mut config = hyperindex_config::load_or_default(Some(&config_path))
            .unwrap()
            .config;
        config.transport.kind = hyperindex_protocol::config::TransportKind::Stdio;
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let server = DaemonServer::new(runtime);

        let response = send_request(
            &server,
            DaemonRequest::new(
                "req-impact-status",
                RequestBody::ImpactStatus(ImpactStatusParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: "snap-1".to_string(),
                }),
            ),
        )
        .await;

        match response.body {
            ResponseBody::Success {
                result: SuccessPayload::ImpactStatus(status),
            } => {
                assert_eq!(status.repo_id, "repo-1");
                assert!(!status.capabilities.analyze);
            }
            other => panic!("unexpected impact status response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn daemon_impact_explain_request_returns_reason_paths() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("impact-repo");
        init_impact_repo(&repo_root);
        let mut config = hyperindex_config::load_or_default(Some(&config_path))
            .unwrap()
            .config;
        config.transport.kind = hyperindex_protocol::config::TransportKind::Stdio;
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let server = DaemonServer::new(runtime);

        let added = send_request(
            &server,
            DaemonRequest::new(
                "req-impact-add",
                RequestBody::ReposAdd(ReposAddParams {
                    repo_root: repo_root.display().to_string(),
                    display_name: Some("Impact Repo".to_string()),
                    notes: Vec::new(),
                    ignore_patterns: Vec::new(),
                    watch_on_add: false,
                }),
            ),
        )
        .await;
        let repo_id = match added.body {
            ResponseBody::Success {
                result: SuccessPayload::ReposAdd(response),
            } => response.repo.repo_id,
            other => panic!("unexpected impact add response: {other:?}"),
        };
        let created = send_request(
            &server,
            DaemonRequest::new(
                "req-impact-snapshot",
                RequestBody::SnapshotsCreate(SnapshotCreateParams {
                    repo_id: repo_id.clone(),
                    include_working_tree: true,
                    buffer_ids: Vec::new(),
                }),
            ),
        )
        .await;
        let snapshot_id = match created.body {
            ResponseBody::Success {
                result: SuccessPayload::SnapshotsCreate(response),
            } => response.snapshot.snapshot_id,
            other => panic!("unexpected impact snapshot response: {other:?}"),
        };
        let built = send_request(
            &server,
            DaemonRequest::new(
                "req-impact-symbol-build",
                RequestBody::SymbolIndexBuild(SymbolIndexBuildParams {
                    repo_id: repo_id.clone(),
                    snapshot_id: snapshot_id.clone(),
                    force: false,
                }),
            ),
        )
        .await;
        match built.body {
            ResponseBody::Success {
                result: SuccessPayload::SymbolIndexBuild(_),
            } => {}
            other => panic!("unexpected impact symbol build response: {other:?}"),
        }

        let response = send_request(
            &server,
            DaemonRequest::new(
                "req-impact-explain",
                RequestBody::ImpactExplain(ImpactExplainParams {
                    repo_id,
                    snapshot_id,
                    target: ImpactTargetRef::Symbol {
                        value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                        symbol_id: None,
                        path: None,
                    },
                    change_hint: ImpactChangeScenario::ModifyBehavior,
                    impacted: ImpactEntityRef::File {
                        path: "packages/api/src/routes/logout.ts".to_string(),
                    },
                    max_reason_paths: 4,
                }),
            ),
        )
        .await;

        match response.body {
            ResponseBody::Success {
                result: SuccessPayload::ImpactExplain(response),
            } => {
                assert_eq!(
                    response.impacted,
                    ImpactEntityRef::File {
                        path: "packages/api/src/routes/logout.ts".to_string(),
                    }
                );
                assert!(!response.reason_paths.is_empty());
            }
            other => panic!("unexpected impact explain response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn restart_bootstrap_preserves_repo_registry_and_snapshot_metadata() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);

        let mut config = hyperindex_config::load_or_default(Some(&config_path))
            .unwrap()
            .config;
        config.transport.kind = hyperindex_protocol::config::TransportKind::Stdio;
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let server = DaemonServer::new(runtime);
        let added = send_request(
            &server,
            DaemonRequest::new(
                "req-add-restart",
                RequestBody::ReposAdd(ReposAddParams {
                    repo_root: repo_root.display().to_string(),
                    display_name: Some("Restart Repo".to_string()),
                    notes: Vec::new(),
                    ignore_patterns: Vec::new(),
                    watch_on_add: false,
                }),
            ),
        )
        .await;
        let repo_id = match added.body {
            ResponseBody::Success {
                result: SuccessPayload::ReposAdd(response),
            } => response.repo.repo_id,
            other => panic!("unexpected add response: {other:?}"),
        };

        let created = send_request(
            &server,
            DaemonRequest::new(
                "req-snapshot-restart",
                RequestBody::SnapshotsCreate(SnapshotCreateParams {
                    repo_id: repo_id.clone(),
                    include_working_tree: true,
                    buffer_ids: Vec::new(),
                }),
            ),
        )
        .await;
        let snapshot_id = match created.body {
            ResponseBody::Success {
                result: SuccessPayload::SnapshotsCreate(response),
            } => response.snapshot.snapshot_id,
            other => panic!("unexpected snapshot response: {other:?}"),
        };

        let restarted = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let store = restarted.state_manager.open_store().unwrap();
        let summary = store.summary().unwrap();
        assert_eq!(summary.repo_count, 1);
        assert_eq!(summary.manifest_count, 1);
        assert_eq!(
            store.show_repo(&repo_id).unwrap().last_snapshot_id,
            Some(snapshot_id)
        );
    }

    #[tokio::test]
    async fn repos_add_reports_invalid_repo_paths_clearly() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let server = DaemonServer::new(runtime);
        let response = send_request(
            &server,
            DaemonRequest::new(
                "req-invalid-repo-path",
                RequestBody::ReposAdd(ReposAddParams {
                    repo_root: tempdir.path().join("missing-repo").display().to_string(),
                    display_name: Some("Missing Repo".to_string()),
                    notes: Vec::new(),
                    ignore_patterns: Vec::new(),
                    watch_on_add: false,
                }),
            ),
        )
        .await;

        match response.body {
            ResponseBody::Error { error, .. } => {
                assert_eq!(error.code, ErrorCode::RepoStateUnavailable);
                assert!(error.message.contains("path does not exist"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn repo_status_fails_clearly_when_repo_root_is_missing() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let server = DaemonServer::new(runtime);
        let added = send_request(
            &server,
            DaemonRequest::new(
                "req-add-missing-repo",
                RequestBody::ReposAdd(ReposAddParams {
                    repo_root: repo_root.display().to_string(),
                    display_name: Some("Missing Root Repo".to_string()),
                    notes: Vec::new(),
                    ignore_patterns: Vec::new(),
                    watch_on_add: false,
                }),
            ),
        )
        .await;
        let repo_id = match added.body {
            ResponseBody::Success {
                result: SuccessPayload::ReposAdd(response),
            } => response.repo.repo_id,
            other => panic!("unexpected add response: {other:?}"),
        };

        fs::remove_dir_all(&repo_root).unwrap();
        let response = send_request(
            &server,
            DaemonRequest::new(
                "req-status-missing-repo",
                RequestBody::RepoStatus(hyperindex_protocol::repo::RepoStatusParams { repo_id }),
            ),
        )
        .await;

        match response.body {
            ResponseBody::Error { error, .. } => {
                assert_eq!(error.code, ErrorCode::RepoStateUnavailable);
                assert!(error.message.contains("root is missing"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn buffers_set_rejects_bad_overlay_input() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);

        let runtime = RuntimeState::bootstrap(Some(&config_path)).unwrap();
        let server = DaemonServer::new(runtime);
        let added = send_request(
            &server,
            DaemonRequest::new(
                "req-add-buffer-input",
                RequestBody::ReposAdd(ReposAddParams {
                    repo_root: repo_root.display().to_string(),
                    display_name: Some("Buffer Repo".to_string()),
                    notes: Vec::new(),
                    ignore_patterns: Vec::new(),
                    watch_on_add: false,
                }),
            ),
        )
        .await;
        let repo_id = match added.body {
            ResponseBody::Success {
                result: SuccessPayload::ReposAdd(response),
            } => response.repo.repo_id,
            other => panic!("unexpected add response: {other:?}"),
        };

        let response = send_request(
            &server,
            DaemonRequest::new(
                "req-bad-buffer-path",
                RequestBody::BuffersSet(BufferSetParams {
                    repo_id,
                    buffer_id: "buffer-1".to_string(),
                    path: "../outside.ts".to_string(),
                    version: 1,
                    language: Some("typescript".to_string()),
                    contents: "export const bad = true;\n".to_string(),
                }),
            ),
        )
        .await;

        match response.body {
            ResponseBody::Error { error, .. } => {
                assert_eq!(error.code, ErrorCode::InvalidRequest);
                assert!(error.message.contains("cannot escape the repo root"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    async fn send_request(server: &DaemonServer, request: DaemonRequest) -> DaemonResponse {
        let raw = serde_json::to_vec(&request).unwrap();
        let encoded = server.handle_raw_request(&raw).await.unwrap();
        serde_json::from_slice(&encoded).unwrap()
    }

    fn write_test_config(root: &Path) -> PathBuf {
        let config_path = root.join("config.toml");
        let runtime_root = root.join(".hyperindex");
        let state_dir = runtime_root.join("state");
        let manifests_dir = runtime_root.join("data/manifests");

        let mut config = hyperindex_protocol::config::RuntimeConfig::default();
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
        config.impact.store_dir = runtime_root.join("data/impact");
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        config_path
    }

    fn init_repo(repo_root: &Path) {
        fs::create_dir_all(repo_root.join("src")).unwrap();
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["checkout", "-b", "trunk"]);
        fs::write(
            repo_root.join("src/app.ts"),
            "export function createSession() {\n  return \"disk\";\n}\n\nexport function run() {\n  return createSession();\n}\n",
        )
        .unwrap();
        run_git(repo_root, &["add", "."]);
        commit_all(repo_root, "initial");
    }

    fn init_impact_repo(repo_root: &Path) {
        fs::create_dir_all(repo_root.join("packages/auth/src/session")).unwrap();
        fs::create_dir_all(repo_root.join("packages/api/src/routes")).unwrap();
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["checkout", "-b", "trunk"]);
        fs::write(
            repo_root.join("packages/auth/src/session/service.ts"),
            "export function invalidateSession(userId: string) {\n  return `invalidated:${userId}`;\n}\n",
        )
        .unwrap();
        fs::write(
            repo_root.join("packages/api/src/routes/logout.ts"),
            "import { invalidateSession } from \"../../../auth/src/session/service\";\n\nexport function logout(userId: string) {\n  return invalidateSession(userId);\n}\n",
        )
        .unwrap();
        run_git(repo_root, &["add", "."]);
        commit_all(repo_root, "impact-initial");
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
