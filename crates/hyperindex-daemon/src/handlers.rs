use std::sync::Arc;

use hyperindex_core::{HyperindexError, normalize_repo_relative_path};
use hyperindex_protocol::api::{
    ApiMethod, DaemonRequest, DaemonResponse, RequestBody, SuccessPayload,
};
use hyperindex_protocol::errors::ProtocolError;
use hyperindex_protocol::impact::{
    ImpactAnalyzeResponse, ImpactExplainResponse, ImpactStatusResponse,
};
use hyperindex_protocol::repo::{
    RepoShowResponse, ReposAddResponse, ReposListResponse, ReposRemoveResponse,
};
use hyperindex_protocol::semantic::{
    SemanticBuildResponse, SemanticInspectChunkResponse, SemanticQueryResponse,
    SemanticStatusResponse,
};
use hyperindex_protocol::snapshot::{
    SnapshotCreateResponse, SnapshotDiffResponse, SnapshotListResponse, SnapshotReadFileResponse,
    SnapshotResolvedFileSource, SnapshotResolvedFileSourceKind, SnapshotShowResponse,
};
use hyperindex_protocol::status::{HealthResponse, HealthState, ShutdownResponse, VersionResponse};
use hyperindex_protocol::symbols::{
    DefinitionLookupResponse, ParseBuildResponse, ParseInspectFileResponse, ParseStatusResponse,
    ReferenceLookupResponse, SymbolIndexBuildResponse, SymbolIndexStatusResponse,
    SymbolResolveResponse, SymbolSearchResponse, SymbolShowResponse,
};
use hyperindex_protocol::{CONFIG_VERSION, PROTOCOL_VERSION};
use hyperindex_repo_store::RepoStore;
use hyperindex_scheduler::jobs::JobKind;
use tracing::{error, info};

use crate::impact::{ImpactService, build_graph_from_store};
use crate::semantic::SemanticService;
use crate::state::DaemonStateManager;
use crate::symbols::ParserSymbolService;

#[derive(Debug, Clone)]
pub struct HandlerRegistry {
    state_manager: Arc<DaemonStateManager>,
}

impl HandlerRegistry {
    pub fn new(state_manager: Arc<DaemonStateManager>) -> Self {
        Self { state_manager }
    }

    pub async fn dispatch(&self, request: DaemonRequest) -> DaemonResponse {
        let request_id = request.request_id.clone();
        let method = method_for(&request.body);
        info!(request_id = %request_id, method = %method_name(&method), "dispatching daemon request");

        if request.protocol_version != PROTOCOL_VERSION {
            return DaemonResponse::error(
                request_id,
                method,
                ProtocolError::unsupported_protocol_version(request.protocol_version),
            );
        }

        match self.handle_body(request.body).await {
            Ok(result) => DaemonResponse::success(request_id, result),
            Err(error) => {
                error!(
                    request_id = %request_id,
                    method = %method_name(&method),
                    code = ?error.code,
                    "daemon request failed"
                );
                DaemonResponse::error(request_id, method, error)
            }
        }
    }

    async fn handle_body(&self, body: RequestBody) -> Result<SuccessPayload, ProtocolError> {
        match body {
            RequestBody::Health(_) => Ok(SuccessPayload::Health(HealthResponse {
                status: match self.state_manager.lifecycle().map_err(internal_error)? {
                    hyperindex_protocol::status::DaemonLifecycleState::Starting => {
                        HealthState::Starting
                    }
                    hyperindex_protocol::status::DaemonLifecycleState::Stopping
                    | hyperindex_protocol::status::DaemonLifecycleState::Stopped => {
                        HealthState::Degraded
                    }
                    hyperindex_protocol::status::DaemonLifecycleState::Running => HealthState::Ok,
                },
                message: "hyperd is healthy".to_string(),
            })),
            RequestBody::Version(_) => Ok(SuccessPayload::Version(VersionResponse {
                daemon_version: env!("CARGO_PKG_VERSION").to_string(),
                protocol_version: PROTOCOL_VERSION.to_string(),
                config_version: CONFIG_VERSION,
            })),
            RequestBody::DaemonStatus(_) => Ok(SuccessPayload::DaemonStatus(
                self.state_manager
                    .runtime_status()
                    .map_err(internal_error)?,
            )),
            RequestBody::ReposAdd(params) => {
                let store = self.open_store()?;
                let repo = store.add_repo(&params).map_err(repo_store_error)?;
                self.state_manager
                    .clear_repo_error_code(&repo.repo_id)
                    .map_err(internal_error)?;

                let repo_job_id = self
                    .state_manager
                    .enqueue_job(Some(&repo.repo_id), JobKind::RepoRefresh)
                    .map_err(internal_error)?;
                self.state_manager
                    .mark_job_running(&repo_job_id)
                    .map_err(internal_error)?;
                self.state_manager
                    .mark_job_succeeded(&repo_job_id)
                    .map_err(internal_error)?;

                if params.watch_on_add {
                    let watch_job_id = self
                        .state_manager
                        .enqueue_job(Some(&repo.repo_id), JobKind::WatchIngest)
                        .map_err(internal_error)?;
                    self.state_manager
                        .mark_job_running(&watch_job_id)
                        .map_err(internal_error)?;
                    self.state_manager.attach_watcher(&repo).map_err(|error| {
                        let protocol_error = internal_error(error);
                        let _ = self.state_manager.set_repo_error_code(
                            &repo.repo_id,
                            format!("{:?}", protocol_error.code),
                        );
                        protocol_error
                    })?;
                    self.state_manager
                        .mark_job_succeeded(&watch_job_id)
                        .map_err(internal_error)?;
                }

                Ok(SuccessPayload::ReposAdd(ReposAddResponse { repo }))
            }
            RequestBody::ReposList(_) => {
                let repos = self.open_store()?.list_repos().map_err(repo_store_error)?;
                Ok(SuccessPayload::ReposList(ReposListResponse {
                    protocol_version: PROTOCOL_VERSION.to_string(),
                    repos,
                }))
            }
            RequestBody::ReposRemove(params) => {
                let response = self
                    .open_store()?
                    .remove_repo(&params)
                    .map_err(repo_store_error)?;
                self.state_manager
                    .detach_watcher(&params.repo_id)
                    .map_err(internal_error)?;
                self.state_manager
                    .clear_repo_error_code(&params.repo_id)
                    .map_err(internal_error)?;
                Ok(SuccessPayload::ReposRemove(ReposRemoveResponse {
                    repo_id: response.repo_id,
                    removed: response.removed,
                    purged_state: response.purged_state,
                }))
            }
            RequestBody::ReposShow(params) => Ok(SuccessPayload::ReposShow(RepoShowResponse {
                repo: self.show_repo(&params.repo_id)?,
            })),
            RequestBody::RepoStatus(params) => Ok(SuccessPayload::RepoStatus(
                self.state_manager
                    .build_repo_status(&params.repo_id)
                    .map_err(repo_or_internal_error)?,
            )),
            RequestBody::SnapshotsCreate(params) => {
                let repo = self.show_repo(&params.repo_id)?;
                let job_id = self
                    .state_manager
                    .enqueue_job(Some(&repo.repo_id), JobKind::SnapshotCapture)
                    .map_err(internal_error)?;
                self.state_manager
                    .mark_job_running(&job_id)
                    .map_err(internal_error)?;

                let snapshot = match self.state_manager.create_snapshot(
                    &repo,
                    params.include_working_tree,
                    &params.buffer_ids,
                ) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        let protocol_error = snapshot_or_buffer_error(error);
                        let _ = self.state_manager.set_repo_error_code(
                            &repo.repo_id,
                            format!("{:?}", protocol_error.code),
                        );
                        let _ = self.state_manager.mark_job_failed(&job_id);
                        return Err(protocol_error);
                    }
                };

                self.state_manager
                    .clear_repo_error_code(&repo.repo_id)
                    .map_err(internal_error)?;
                self.state_manager
                    .mark_job_succeeded(&job_id)
                    .map_err(internal_error)?;

                Ok(SuccessPayload::SnapshotsCreate(SnapshotCreateResponse {
                    snapshot,
                }))
            }
            RequestBody::SnapshotsShow(params) => {
                let snapshot = self.load_manifest(&params.snapshot_id)?;
                Ok(SuccessPayload::SnapshotsShow(SnapshotShowResponse {
                    snapshot,
                }))
            }
            RequestBody::SnapshotsList(params) => {
                let store = self.open_store()?;
                let snapshots = store
                    .list_manifests(&params.repo_id, params.limit)
                    .map_err(repo_store_error)?;
                Ok(SuccessPayload::SnapshotsList(SnapshotListResponse {
                    repo_id: params.repo_id,
                    snapshots,
                }))
            }
            RequestBody::SnapshotsDiff(params) => {
                let left = self.load_manifest(&params.left_snapshot_id)?;
                let right = self.load_manifest(&params.right_snapshot_id)?;
                let diff: SnapshotDiffResponse =
                    self.state_manager.snapshot_assembler().diff(&left, &right);
                Ok(SuccessPayload::SnapshotsDiff(diff))
            }
            RequestBody::SnapshotsReadFile(params) => {
                let normalized_path = normalize_repo_relative_path(&params.path, "snapshot read")
                    .map_err(validation_error)?;
                let snapshot = self.load_manifest(&params.snapshot_id)?;
                let resolved = self
                    .state_manager
                    .snapshot_assembler()
                    .resolve_file(&snapshot, &normalized_path)
                    .ok_or_else(|| {
                        ProtocolError::snapshot_not_found(format!(
                            "{}:{}",
                            params.snapshot_id, normalized_path
                        ))
                    })?;
                Ok(SuccessPayload::SnapshotsReadFile(
                    SnapshotReadFileResponse {
                        snapshot_id: params.snapshot_id,
                        path: resolved.path,
                        resolved_from: snapshot_source(&resolved.resolved_from),
                        contents: resolved.contents,
                    },
                ))
            }
            RequestBody::BuffersSet(params) => {
                self.show_repo(&params.repo_id)?;
                let response = self
                    .open_store()?
                    .set_buffer(&params)
                    .map_err(repo_store_error)?;
                Ok(SuccessPayload::BuffersSet(response))
            }
            RequestBody::BuffersClear(params) => {
                self.show_repo(&params.repo_id)?;
                let response = self
                    .open_store()?
                    .clear_buffer(&params)
                    .map_err(repo_store_error)?;
                Ok(SuccessPayload::BuffersClear(response))
            }
            RequestBody::BuffersList(params) => {
                self.show_repo(&params.repo_id)?;
                let buffers = self
                    .open_store()?
                    .list_buffers(&params)
                    .map_err(repo_store_error)?;
                Ok(SuccessPayload::BuffersList(
                    hyperindex_protocol::buffers::BufferListResponse {
                        repo_id: params.repo_id,
                        buffers,
                    },
                ))
            }
            RequestBody::ParseBuild(params) => {
                Ok(SuccessPayload::ParseBuild(self.parse_build(&params)?))
            }
            RequestBody::ParseStatus(params) => {
                Ok(SuccessPayload::ParseStatus(self.parse_status(&params)?))
            }
            RequestBody::ParseInspectFile(params) => Ok(SuccessPayload::ParseInspectFile(
                self.parse_inspect_file(&params)?,
            )),
            RequestBody::SymbolIndexBuild(params) => Ok(SuccessPayload::SymbolIndexBuild(
                self.symbol_index_build(&params)?,
            )),
            RequestBody::SymbolIndexStatus(params) => Ok(SuccessPayload::SymbolIndexStatus(
                self.symbol_index_status(&params)?,
            )),
            RequestBody::SymbolSearch(params) => {
                Ok(SuccessPayload::SymbolSearch(self.symbol_search(&params)?))
            }
            RequestBody::SymbolShow(params) => {
                Ok(SuccessPayload::SymbolShow(self.symbol_show(&params)?))
            }
            RequestBody::DefinitionLookup(params) => Ok(SuccessPayload::DefinitionLookup(
                self.definition_lookup(&params)?,
            )),
            RequestBody::ReferenceLookup(params) => Ok(SuccessPayload::ReferenceLookup(
                self.reference_lookup(&params)?,
            )),
            RequestBody::SymbolResolve(params) => {
                Ok(SuccessPayload::SymbolResolve(self.symbol_resolve(&params)?))
            }
            RequestBody::SemanticStatus(params) => Ok(SuccessPayload::SemanticStatus(
                self.semantic_status(&params)?,
            )),
            RequestBody::SemanticBuild(params) => {
                Ok(SuccessPayload::SemanticBuild(self.semantic_build(&params)?))
            }
            RequestBody::SemanticQuery(params) => {
                Ok(SuccessPayload::SemanticQuery(self.semantic_query(&params)?))
            }
            RequestBody::SemanticInspectChunk(params) => Ok(SuccessPayload::SemanticInspectChunk(
                self.semantic_inspect_chunk(&params)?,
            )),
            RequestBody::ImpactStatus(params) => {
                Ok(SuccessPayload::ImpactStatus(self.impact_status(&params)?))
            }
            RequestBody::ImpactAnalyze(params) => {
                Ok(SuccessPayload::ImpactAnalyze(self.impact_analyze(&params)?))
            }
            RequestBody::ImpactExplain(params) => {
                Ok(SuccessPayload::ImpactExplain(self.impact_explain(&params)?))
            }
            RequestBody::Shutdown(_) => {
                let lifecycle = self.state_manager.lifecycle().map_err(internal_error)?;
                if matches!(
                    lifecycle,
                    hyperindex_protocol::status::DaemonLifecycleState::Stopping
                        | hyperindex_protocol::status::DaemonLifecycleState::Stopped
                ) {
                    return Err(ProtocolError::shutdown_in_progress());
                }
                let accepted = self.state_manager.request_shutdown();
                Ok(SuccessPayload::Shutdown(ShutdownResponse {
                    accepted,
                    message: Some("shutdown requested".to_string()),
                }))
            }
            RequestBody::WatchStatus(_) => Err(ProtocolError::not_implemented("watch_status")),
            RequestBody::WatchEvents(_) => Err(ProtocolError::not_implemented("watch_events")),
        }
    }

    fn open_store(&self) -> Result<RepoStore, ProtocolError> {
        self.state_manager.open_store().map_err(repo_store_error)
    }

    fn show_repo(
        &self,
        repo_id: &str,
    ) -> Result<hyperindex_protocol::repo::RepoRecord, ProtocolError> {
        self.open_store()?
            .show_repo(repo_id)
            .map_err(|error| repo_or_internal_error_with_id(error, repo_id))
    }

    fn load_manifest(
        &self,
        snapshot_id: &str,
    ) -> Result<hyperindex_protocol::snapshot::ComposedSnapshot, ProtocolError> {
        self.open_store()?
            .load_manifest(snapshot_id)
            .map_err(repo_store_error)?
            .ok_or_else(|| ProtocolError::snapshot_not_found(snapshot_id))
    }

    fn parse_build(
        &self,
        params: &hyperindex_protocol::symbols::ParseBuildParams,
    ) -> Result<ParseBuildResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .parse_build(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn parse_status(
        &self,
        params: &hyperindex_protocol::symbols::ParseStatusParams,
    ) -> Result<ParseStatusResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .parse_status(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn parse_inspect_file(
        &self,
        params: &hyperindex_protocol::symbols::ParseInspectFileParams,
    ) -> Result<ParseInspectFileResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .parse_inspect_file(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn symbol_index_build(
        &self,
        params: &hyperindex_protocol::symbols::SymbolIndexBuildParams,
    ) -> Result<SymbolIndexBuildResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .symbol_index_build(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn symbol_index_status(
        &self,
        params: &hyperindex_protocol::symbols::SymbolIndexStatusParams,
    ) -> Result<SymbolIndexStatusResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .symbol_index_status(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn symbol_search(
        &self,
        params: &hyperindex_protocol::symbols::SymbolSearchParams,
    ) -> Result<SymbolSearchResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .search(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn symbol_show(
        &self,
        params: &hyperindex_protocol::symbols::SymbolShowParams,
    ) -> Result<SymbolShowResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .show(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn definition_lookup(
        &self,
        params: &hyperindex_protocol::symbols::DefinitionLookupParams,
    ) -> Result<DefinitionLookupResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .definitions(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn reference_lookup(
        &self,
        params: &hyperindex_protocol::symbols::ReferenceLookupParams,
    ) -> Result<ReferenceLookupResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .references(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn symbol_resolve(
        &self,
        params: &hyperindex_protocol::symbols::SymbolResolveParams,
    ) -> Result<SymbolResolveResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        self.symbol_service()
            .resolve(&repo, &snapshot, params)
            .map_err(internal_error)
    }

    fn impact_analyze(
        &self,
        params: &hyperindex_protocol::impact::ImpactAnalyzeParams,
    ) -> Result<ImpactAnalyzeResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        let impact_store_root = &self.state_manager.loaded_config().config.impact.store_dir;
        let symbol_store_root = &self
            .state_manager
            .loaded_config()
            .config
            .symbol_index
            .store_dir;
        let repo_store = self.open_store()?;
        let graph = build_graph_from_store(symbol_store_root, &repo.repo_id, &snapshot)?;
        ImpactService.analyze(
            impact_store_root,
            &repo_store,
            &self.state_manager.loaded_config().config.impact,
            &graph,
            &snapshot,
            params,
        )
    }

    fn impact_status(
        &self,
        params: &hyperindex_protocol::impact::ImpactStatusParams,
    ) -> Result<ImpactStatusResponse, ProtocolError> {
        let impact_store_root = &self.state_manager.loaded_config().config.impact.store_dir;
        let symbol_store_root = &self
            .state_manager
            .loaded_config()
            .config
            .symbol_index
            .store_dir;
        ImpactService.status(
            impact_store_root,
            symbol_store_root,
            &self.state_manager.loaded_config().config.impact,
            params,
        )
    }

    fn semantic_status(
        &self,
        params: &hyperindex_protocol::semantic::SemanticStatusParams,
    ) -> Result<SemanticStatusResponse, ProtocolError> {
        let semantic_store_root = &self.state_manager.loaded_config().config.semantic.store_dir;
        let symbol_store_root = &self
            .state_manager
            .loaded_config()
            .config
            .symbol_index
            .store_dir;
        SemanticService.status(
            semantic_store_root,
            symbol_store_root,
            &self.state_manager.loaded_config().config.semantic,
            params,
        )
    }

    fn semantic_build(
        &self,
        params: &hyperindex_protocol::semantic::SemanticBuildParams,
    ) -> Result<SemanticBuildResponse, ProtocolError> {
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        let repo_store = self.open_store()?;
        let semantic_store_root = &self.state_manager.loaded_config().config.semantic.store_dir;
        let symbol_store_root = &self
            .state_manager
            .loaded_config()
            .config
            .symbol_index
            .store_dir;
        SemanticService.build(
            &repo_store,
            semantic_store_root,
            symbol_store_root,
            &self.state_manager.loaded_config().config.semantic,
            &snapshot,
            params,
        )
    }

    fn semantic_query(
        &self,
        params: &hyperindex_protocol::semantic::SemanticQueryParams,
    ) -> Result<SemanticQueryResponse, ProtocolError> {
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        let semantic_store_root = &self.state_manager.loaded_config().config.semantic.store_dir;
        let symbol_store_root = &self
            .state_manager
            .loaded_config()
            .config
            .symbol_index
            .store_dir;
        SemanticService.query(
            semantic_store_root,
            symbol_store_root,
            &self.state_manager.loaded_config().config.semantic,
            &hyperindex_protocol::semantic::SemanticQueryParams {
                repo_id: snapshot.repo_id,
                snapshot_id: snapshot.snapshot_id,
                query: params.query.clone(),
                filters: params.filters.clone(),
                limit: params.limit,
                rerank_mode: params.rerank_mode.clone(),
            },
        )
    }

    fn semantic_inspect_chunk(
        &self,
        params: &hyperindex_protocol::semantic::SemanticInspectChunkParams,
    ) -> Result<SemanticInspectChunkResponse, ProtocolError> {
        let semantic_store_root = &self.state_manager.loaded_config().config.semantic.store_dir;
        SemanticService.inspect_chunk(
            semantic_store_root,
            &self.state_manager.loaded_config().config.semantic,
            params,
        )
    }

    fn impact_explain(
        &self,
        params: &hyperindex_protocol::impact::ImpactExplainParams,
    ) -> Result<ImpactExplainResponse, ProtocolError> {
        let repo = self.show_repo(&params.repo_id)?;
        let snapshot = self.load_symbol_snapshot(&params.repo_id, &params.snapshot_id)?;
        let impact_store_root = &self.state_manager.loaded_config().config.impact.store_dir;
        let symbol_store_root = &self
            .state_manager
            .loaded_config()
            .config
            .symbol_index
            .store_dir;
        let repo_store = self.open_store()?;
        let graph = build_graph_from_store(symbol_store_root, &repo.repo_id, &snapshot)?;
        ImpactService.explain(
            impact_store_root,
            &repo_store,
            &self.state_manager.loaded_config().config.impact,
            &graph,
            &snapshot,
            params,
        )
    }

    fn load_symbol_snapshot(
        &self,
        repo_id: &str,
        snapshot_id: &str,
    ) -> Result<hyperindex_protocol::snapshot::ComposedSnapshot, ProtocolError> {
        let snapshot = self.load_manifest(snapshot_id)?;
        if snapshot.repo_id != repo_id {
            return Err(ProtocolError::invalid_request(format!(
                "snapshot {snapshot_id} does not belong to repo {repo_id}"
            )));
        }
        Ok(snapshot)
    }

    fn symbol_service(&self) -> ParserSymbolService {
        ParserSymbolService::from_loaded_config(self.state_manager.loaded_config())
    }
}

fn method_for(body: &RequestBody) -> ApiMethod {
    match body {
        RequestBody::Health(_) => ApiMethod::Health,
        RequestBody::Version(_) => ApiMethod::Version,
        RequestBody::DaemonStatus(_) => ApiMethod::DaemonStatus,
        RequestBody::ReposAdd(_) => ApiMethod::ReposAdd,
        RequestBody::ReposList(_) => ApiMethod::ReposList,
        RequestBody::ReposRemove(_) => ApiMethod::ReposRemove,
        RequestBody::ReposShow(_) => ApiMethod::ReposShow,
        RequestBody::RepoStatus(_) => ApiMethod::RepoStatus,
        RequestBody::WatchStatus(_) => ApiMethod::WatchStatus,
        RequestBody::WatchEvents(_) => ApiMethod::WatchEvents,
        RequestBody::SnapshotsCreate(_) => ApiMethod::SnapshotsCreate,
        RequestBody::SnapshotsShow(_) => ApiMethod::SnapshotsShow,
        RequestBody::SnapshotsList(_) => ApiMethod::SnapshotsList,
        RequestBody::SnapshotsDiff(_) => ApiMethod::SnapshotsDiff,
        RequestBody::SnapshotsReadFile(_) => ApiMethod::SnapshotsReadFile,
        RequestBody::BuffersSet(_) => ApiMethod::BuffersSet,
        RequestBody::BuffersClear(_) => ApiMethod::BuffersClear,
        RequestBody::BuffersList(_) => ApiMethod::BuffersList,
        RequestBody::ParseBuild(_) => ApiMethod::ParseBuild,
        RequestBody::ParseStatus(_) => ApiMethod::ParseStatus,
        RequestBody::ParseInspectFile(_) => ApiMethod::ParseInspectFile,
        RequestBody::SymbolIndexBuild(_) => ApiMethod::SymbolIndexBuild,
        RequestBody::SymbolIndexStatus(_) => ApiMethod::SymbolIndexStatus,
        RequestBody::SymbolSearch(_) => ApiMethod::SymbolSearch,
        RequestBody::SymbolShow(_) => ApiMethod::SymbolShow,
        RequestBody::DefinitionLookup(_) => ApiMethod::DefinitionLookup,
        RequestBody::ReferenceLookup(_) => ApiMethod::ReferenceLookup,
        RequestBody::SymbolResolve(_) => ApiMethod::SymbolResolve,
        RequestBody::SemanticStatus(_) => ApiMethod::SemanticStatus,
        RequestBody::SemanticBuild(_) => ApiMethod::SemanticBuild,
        RequestBody::SemanticQuery(_) => ApiMethod::SemanticQuery,
        RequestBody::SemanticInspectChunk(_) => ApiMethod::SemanticInspectChunk,
        RequestBody::ImpactStatus(_) => ApiMethod::ImpactStatus,
        RequestBody::ImpactAnalyze(_) => ApiMethod::ImpactAnalyze,
        RequestBody::ImpactExplain(_) => ApiMethod::ImpactExplain,
        RequestBody::Shutdown(_) => ApiMethod::Shutdown,
    }
}

fn method_name(method: &ApiMethod) -> &'static str {
    match method {
        ApiMethod::Health => "health",
        ApiMethod::Version => "version",
        ApiMethod::DaemonStatus => "daemon_status",
        ApiMethod::ReposAdd => "repos_add",
        ApiMethod::ReposList => "repos_list",
        ApiMethod::ReposRemove => "repos_remove",
        ApiMethod::ReposShow => "repos_show",
        ApiMethod::RepoStatus => "repo_status",
        ApiMethod::WatchStatus => "watch_status",
        ApiMethod::WatchEvents => "watch_events",
        ApiMethod::SnapshotsCreate => "snapshots_create",
        ApiMethod::SnapshotsShow => "snapshots_show",
        ApiMethod::SnapshotsList => "snapshots_list",
        ApiMethod::SnapshotsDiff => "snapshots_diff",
        ApiMethod::SnapshotsReadFile => "snapshots_read_file",
        ApiMethod::BuffersSet => "buffers_set",
        ApiMethod::BuffersClear => "buffers_clear",
        ApiMethod::BuffersList => "buffers_list",
        ApiMethod::ParseBuild => "parse_build",
        ApiMethod::ParseStatus => "parse_status",
        ApiMethod::ParseInspectFile => "parse_inspect_file",
        ApiMethod::SymbolIndexBuild => "symbol_index_build",
        ApiMethod::SymbolIndexStatus => "symbol_index_status",
        ApiMethod::SymbolSearch => "symbol_search",
        ApiMethod::SymbolShow => "symbol_show",
        ApiMethod::DefinitionLookup => "definition_lookup",
        ApiMethod::ReferenceLookup => "reference_lookup",
        ApiMethod::SymbolResolve => "symbol_resolve",
        ApiMethod::SemanticStatus => "semantic_status",
        ApiMethod::SemanticBuild => "semantic_build",
        ApiMethod::SemanticQuery => "semantic_query",
        ApiMethod::SemanticInspectChunk => "semantic_inspect_chunk",
        ApiMethod::ImpactStatus => "impact_status",
        ApiMethod::ImpactAnalyze => "impact_analyze",
        ApiMethod::ImpactExplain => "impact_explain",
        ApiMethod::Shutdown => "shutdown",
    }
}

fn repo_or_internal_error(error: HyperindexError) -> ProtocolError {
    if let HyperindexError::Message(message) = &error {
        if let Some(repo_id) = parse_missing_identifier(message, "repo ") {
            return ProtocolError::repo_not_found(repo_id);
        }
        if let Some((repo_id, repo_root)) = parse_missing_repo_root(message) {
            return ProtocolError::repo_state_unavailable(repo_id, repo_root, message.clone());
        }
        if let Some(repo_root) = parse_missing_path(message) {
            return ProtocolError::repo_state_unavailable(
                "unregistered-repo",
                repo_root,
                message.clone(),
            );
        }
    }
    internal_error(error)
}

fn repo_or_internal_error_with_id(error: HyperindexError, repo_id: &str) -> ProtocolError {
    if matches!(error, HyperindexError::Message(_)) {
        ProtocolError::repo_not_found(repo_id)
    } else {
        internal_error(error)
    }
}

fn snapshot_or_buffer_error(error: HyperindexError) -> ProtocolError {
    match error {
        HyperindexError::Message(message) => {
            if let Some(snapshot_id) = parse_missing_identifier(&message, "snapshot ") {
                return ProtocolError::snapshot_not_found(snapshot_id);
            }
            if let Some((buffer_id, repo_id)) = parse_buffer_not_found(&message) {
                return ProtocolError::buffer_not_found(repo_id, buffer_id);
            }
            if looks_like_validation_error(&message) {
                return ProtocolError::invalid_request(message);
            }
            ProtocolError::internal(message)
        }
        other => internal_error(other),
    }
}

fn repo_store_error(error: HyperindexError) -> ProtocolError {
    match error {
        HyperindexError::InvalidConfig(message) => ProtocolError::config_invalid(message),
        HyperindexError::NotImplemented(area) => ProtocolError::not_implemented(area),
        HyperindexError::Message(message) => {
            if message.contains("already registered") {
                ProtocolError::repo_already_exists(message)
            } else if looks_like_validation_error(&message) {
                ProtocolError::invalid_request(message)
            } else if let Some(repo_root) = parse_missing_path(&message) {
                ProtocolError::repo_state_unavailable("unregistered-repo", repo_root, message)
            } else if message.contains("sqlite")
                || message.contains("manifest")
                || message.contains("query failed")
                || message.contains("prepare failed")
            {
                ProtocolError::storage(message)
            } else {
                ProtocolError::internal(message)
            }
        }
    }
}

fn internal_error(error: HyperindexError) -> ProtocolError {
    match error {
        HyperindexError::InvalidConfig(message) => ProtocolError::config_invalid(message),
        HyperindexError::NotImplemented(area) => ProtocolError::not_implemented(area),
        HyperindexError::Message(message) => {
            if looks_like_validation_error(&message) {
                ProtocolError::invalid_request(message)
            } else {
                ProtocolError::internal(message)
            }
        }
    }
}

fn validation_error(error: HyperindexError) -> ProtocolError {
    match error {
        HyperindexError::Message(message) => ProtocolError::invalid_request(message),
        other => internal_error(other),
    }
}

fn parse_missing_identifier(message: &str, prefix: &str) -> Option<String> {
    message
        .strip_prefix(prefix)
        .and_then(|rest| rest.split_once(" was not found"))
        .map(|(id, _)| id.to_string())
}

fn parse_buffer_not_found(message: &str) -> Option<(String, String)> {
    let rest = message.strip_prefix("buffer ")?;
    let (buffer_id, repo_rest) = rest.split_once(" was not found for repo ")?;
    Some((buffer_id.to_string(), repo_rest.to_string()))
}

fn parse_missing_repo_root(message: &str) -> Option<(String, String)> {
    let rest = message.strip_prefix("repo ")?;
    let (repo_id, repo_root) = rest.split_once(" root is missing at ")?;
    let repo_root = repo_root
        .split_once(';')
        .map(|(value, _)| value)
        .unwrap_or(repo_root);
    Some((repo_id.to_string(), repo_root.to_string()))
}

fn parse_missing_path(message: &str) -> Option<String> {
    message
        .strip_prefix("path does not exist: ")
        .map(str::to_string)
}

fn looks_like_validation_error(message: &str) -> bool {
    message.contains("path must be repo-relative")
        || message.contains("cannot escape the repo root")
        || message.contains("is empty; use a repo-relative path")
        || message.contains("was requested more than once")
}

fn snapshot_source(
    resolved_from: &hyperindex_snapshot::ResolvedFrom,
) -> SnapshotResolvedFileSource {
    match resolved_from {
        hyperindex_snapshot::ResolvedFrom::BufferOverlay(buffer_id) => SnapshotResolvedFileSource {
            kind: SnapshotResolvedFileSourceKind::BufferOverlay,
            buffer_id: Some(buffer_id.clone()),
        },
        hyperindex_snapshot::ResolvedFrom::WorkingTreeOverlay => SnapshotResolvedFileSource {
            kind: SnapshotResolvedFileSourceKind::WorkingTreeOverlay,
            buffer_id: None,
        },
        hyperindex_snapshot::ResolvedFrom::BaseSnapshot => SnapshotResolvedFileSource {
            kind: SnapshotResolvedFileSourceKind::BaseSnapshot,
            buffer_id: None,
        },
    }
}
