pub mod cli_integration;
pub mod common;
pub mod daemon_integration;
pub mod exact_route;
pub mod intent_router;
pub mod planner_engine;
pub mod planner_model;
pub mod query_ir;
pub mod result_grouping;
pub mod route_registry;
pub mod score_fusion;
pub mod trust_payloads;

pub use daemon_integration::PlannerRuntimeContext;
pub use planner_model::{PlannerError, PlannerResult, PlannerWorkspace};

#[cfg(test)]
mod tests {
    use hyperindex_protocol::planner::{
        PlannerMode, PlannerQueryFilters, PlannerQueryParams, PlannerRouteKind, PlannerRouteStatus,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use crate::{PlannerRuntimeContext, PlannerWorkspace};

    fn scaffold_snapshot() -> ComposedSnapshot {
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
                digest: "working-tree".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    #[test]
    fn planner_workspace_emits_scaffold_trace_for_impact_query() {
        let response = PlannerWorkspace::default()
            .plan(
                &PlannerRuntimeContext::default(),
                &PlannerQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: hyperindex_protocol::planner::PlannerUserQuery {
                        text: "where do we invalidate sessions?".to_string(),
                    },
                    mode_override: None,
                    selected_context: None,
                    target_context: None,
                    filters: PlannerQueryFilters {
                        path_globs: vec!["packages/**".to_string()],
                        package_names: Vec::new(),
                        package_roots: Vec::new(),
                        workspace_roots: Vec::new(),
                        languages: Vec::new(),
                        extensions: Vec::new(),
                        symbol_kinds: Vec::new(),
                    },
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 10,
                    explain: false,
                    include_trace: true,
                },
                &scaffold_snapshot(),
            )
            .unwrap();

        assert_eq!(response.mode.selected_mode, PlannerMode::Impact);
        assert!(response.groups.is_empty());
        let trace = response.trace.unwrap();
        assert!(
            trace
                .routes
                .iter()
                .any(|route| route.route_kind == PlannerRouteKind::Impact
                    && route.status == PlannerRouteStatus::Deferred
                    && route.selected)
        );
    }

    #[test]
    fn explicit_mode_override_overrides_heuristics() {
        let response = PlannerWorkspace::default()
            .plan(
                &PlannerRuntimeContext::default(),
                &PlannerQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: hyperindex_protocol::planner::PlannerUserQuery {
                        text: "invalidateSession".to_string(),
                    },
                    mode_override: Some(PlannerMode::Symbol),
                    selected_context: None,
                    target_context: None,
                    filters: PlannerQueryFilters::default(),
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 5,
                    explain: false,
                    include_trace: false,
                },
                &scaffold_snapshot(),
            )
            .unwrap();

        assert_eq!(response.mode.selected_mode, PlannerMode::Symbol);
        assert!(response.trace.is_none());
    }
}
