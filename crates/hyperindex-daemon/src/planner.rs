use hyperindex_config::LoadedConfig;
use hyperindex_planner::{PlannerError, PlannerRuntimeContext, PlannerWorkspace};
use hyperindex_protocol::errors::ProtocolError;
use hyperindex_protocol::planner::{
    PlannerCapabilitiesParams, PlannerCapabilitiesResponse, PlannerExplainParams,
    PlannerExplainResponse, PlannerQueryParams, PlannerQueryResponse, PlannerStatusParams,
    PlannerStatusResponse,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use tracing::info;

#[derive(Debug, Default, Clone)]
pub struct PlannerService;

impl PlannerService {
    pub fn status(
        &self,
        loaded: &LoadedConfig,
        snapshot: &ComposedSnapshot,
        params: &PlannerStatusParams,
    ) -> Result<PlannerStatusResponse, ProtocolError> {
        validate_snapshot_scope(snapshot, &params.repo_id, &params.snapshot_id)?;
        let context = runtime_context(loaded);
        Ok(PlannerStatusResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            state: context.query_state(),
            capabilities: context.capabilities(),
            diagnostics: hyperindex_planner::daemon_integration::runtime_diagnostics(&context),
        })
    }

    pub fn capabilities(
        &self,
        loaded: &LoadedConfig,
        snapshot: &ComposedSnapshot,
        params: &PlannerCapabilitiesParams,
    ) -> Result<PlannerCapabilitiesResponse, ProtocolError> {
        validate_snapshot_scope(snapshot, &params.repo_id, &params.snapshot_id)?;
        let context = runtime_context(loaded);
        Ok(PlannerCapabilitiesResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            default_mode: context.default_mode.clone(),
            default_limit: context.default_limit,
            max_limit: context.max_limit,
            budgets: context.budget_policy.clone(),
            capabilities: context.capabilities(),
            diagnostics: hyperindex_planner::daemon_integration::runtime_diagnostics(&context),
        })
    }

    pub fn query(
        &self,
        loaded: &LoadedConfig,
        snapshot: &ComposedSnapshot,
        params: &PlannerQueryParams,
    ) -> Result<PlannerQueryResponse, ProtocolError> {
        info!(
            repo_id = %params.repo_id,
            snapshot_id = %params.snapshot_id,
            include_trace = params.include_trace,
            explain = params.explain,
            "serving phase7 unified planner query"
        );

        let context = runtime_context(loaded);
        PlannerWorkspace::default()
            .plan(&context, params, snapshot)
            .map_err(map_planner_error)
    }

    pub fn explain(
        &self,
        loaded: &LoadedConfig,
        snapshot: &ComposedSnapshot,
        params: &PlannerExplainParams,
    ) -> Result<PlannerExplainResponse, ProtocolError> {
        info!(
            repo_id = %params.query.repo_id,
            snapshot_id = %params.query.snapshot_id,
            "serving phase7 planner explain trace"
        );

        let context = runtime_context(loaded);
        PlannerWorkspace::default()
            .explain(&context, &params.query, snapshot)
            .map_err(map_planner_error)
    }
}

fn runtime_context(loaded: &LoadedConfig) -> PlannerRuntimeContext {
    PlannerRuntimeContext {
        planner_enabled: loaded.config.planner.enabled,
        exact_enabled: loaded.config.planner.routes.exact_enabled,
        symbol_available: loaded.config.symbol_index.enabled
            && loaded.config.planner.routes.symbol_enabled,
        semantic_available: loaded.config.semantic.enabled
            && loaded.config.planner.routes.semantic_enabled,
        impact_available: loaded.config.impact.enabled
            && loaded.config.planner.routes.impact_enabled,
        exact_available: false,
        default_mode: loaded.config.planner.default_mode.clone(),
        default_limit: loaded.config.planner.default_limit as u32,
        max_limit: loaded.config.planner.max_limit as u32,
        budget_policy: loaded.config.planner.budgets.clone(),
    }
}

fn validate_snapshot_scope(
    snapshot: &ComposedSnapshot,
    repo_id: &str,
    snapshot_id: &str,
) -> Result<(), ProtocolError> {
    if snapshot.repo_id != repo_id || snapshot.snapshot_id != snapshot_id {
        return Err(ProtocolError::invalid_request(format!(
            "planner snapshot mismatch: requested repo={repo_id} snapshot={snapshot_id}, loaded repo={} snapshot={}",
            snapshot.repo_id, snapshot.snapshot_id
        )));
    }
    Ok(())
}

fn map_planner_error(error: PlannerError) -> ProtocolError {
    match error {
        PlannerError::InvalidQuery(message) => ProtocolError::invalid_field(
            "query.text",
            message,
            Some("non-empty planner query text".to_string()),
        ),
        PlannerError::SnapshotMismatch { .. } => ProtocolError::invalid_request(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use hyperindex_config::LoadedConfig;
    use hyperindex_protocol::config::RuntimeConfig;
    use hyperindex_protocol::planner::{
        PlannerCapabilitiesParams, PlannerExplainParams, PlannerMode, PlannerQueryFilters,
        PlannerQueryParams, PlannerStatusParams, PlannerUserQuery,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use super::PlannerService;

    fn loaded_config() -> LoadedConfig {
        LoadedConfig {
            config_path: PathBuf::from("/tmp/hyperindex-config.toml"),
            config: RuntimeConfig::default(),
        }
    }

    fn snapshot() -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            repo_id: "repo-123".to_string(),
            repo_root: "/tmp/repo".to_string(),
            snapshot_id: "snap-123".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "deadbeef".to_string(),
                digest: "base".to_string(),
                file_count: 0,
                files: Vec::new(),
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    #[test]
    fn planner_service_returns_contract_query_response() {
        let response = PlannerService
            .query(
                &loaded_config(),
                &snapshot(),
                &PlannerQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: PlannerUserQuery {
                        text: "where do we invalidate sessions?".to_string(),
                    },
                    mode_override: Some(PlannerMode::Auto),
                    selected_context: None,
                    target_context: None,
                    filters: PlannerQueryFilters::default(),
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 8,
                    explain: false,
                    include_trace: true,
                },
            )
            .unwrap();

        assert_eq!(response.repo_id, "repo-123");
        assert!(response.trace.is_some());
        assert!(response.groups.is_empty());
        assert!(response.no_answer.is_some());
    }

    #[test]
    fn planner_service_returns_contract_explain_response() {
        let response = PlannerService
            .explain(
                &loaded_config(),
                &snapshot(),
                &PlannerExplainParams {
                    query: PlannerQueryParams {
                        repo_id: "repo-123".to_string(),
                        snapshot_id: "snap-123".to_string(),
                        query: PlannerUserQuery {
                            text: "SessionStore".to_string(),
                        },
                        mode_override: Some(PlannerMode::Symbol),
                        selected_context: None,
                        target_context: None,
                        filters: PlannerQueryFilters::default(),
                        route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                        budgets: None,
                        limit: 5,
                        explain: true,
                        include_trace: true,
                    },
                },
            )
            .unwrap();

        assert!(response.trace.is_some());
        assert!(response.candidates.is_empty());
    }

    #[test]
    fn planner_service_reports_status_and_capabilities() {
        let status = PlannerService
            .status(
                &loaded_config(),
                &snapshot(),
                &PlannerStatusParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                },
            )
            .unwrap();
        let capabilities = PlannerService
            .capabilities(
                &loaded_config(),
                &snapshot(),
                &PlannerCapabilitiesParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                },
            )
            .unwrap();

        assert_eq!(status.repo_id, "repo-123");
        assert!(capabilities.capabilities.query);
        assert!(
            capabilities
                .capabilities
                .routes
                .iter()
                .any(|route| route.route_kind
                    == hyperindex_protocol::planner::PlannerRouteKind::Exact
                    && !route.available)
        );
    }
}
