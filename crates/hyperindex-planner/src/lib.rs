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
        PlannerIntentKind, PlannerQueryParams, PlannerQueryText, PlannerRouteKind,
        PlannerRouteStatus,
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
    fn planner_workspace_emits_scaffold_trace_for_hybrid_query() {
        let response = PlannerWorkspace::default()
            .plan(
                &PlannerRuntimeContext::default(),
                &PlannerQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: PlannerQueryText {
                        text: "where do we invalidate sessions?".to_string(),
                    },
                    intent_hint: None,
                    path_globs: vec!["packages/**".to_string()],
                    limit: 10,
                    include_trace: true,
                },
                &scaffold_snapshot(),
            )
            .unwrap();

        assert_eq!(response.intent.selected_intent, PlannerIntentKind::Hybrid);
        assert!(response.groups.is_empty());
        let trace = response.trace.unwrap();
        assert!(
            trace
                .routes
                .iter()
                .any(|route| route.route_kind == PlannerRouteKind::Exact
                    && route.status == PlannerRouteStatus::Skipped)
        );
    }

    #[test]
    fn explicit_intent_hint_overrides_heuristics() {
        let response = PlannerWorkspace::default()
            .plan(
                &PlannerRuntimeContext::default(),
                &PlannerQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: PlannerQueryText {
                        text: "invalidateSession".to_string(),
                    },
                    intent_hint: Some(PlannerIntentKind::Impact),
                    path_globs: Vec::new(),
                    limit: 5,
                    include_trace: false,
                },
                &scaffold_snapshot(),
            )
            .unwrap();

        assert_eq!(response.intent.selected_intent, PlannerIntentKind::Impact);
        assert!(response.trace.is_none());
    }
}
