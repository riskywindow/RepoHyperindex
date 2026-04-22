use hyperindex_config::LoadedConfig;
use hyperindex_planner::{PlannerError, PlannerRuntimeContext, PlannerWorkspace};
use hyperindex_protocol::errors::ProtocolError;
use hyperindex_protocol::planner::{PlannerQueryParams, PlannerQueryResponse};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use tracing::info;

#[derive(Debug, Default, Clone)]
pub struct PlannerService;

impl PlannerService {
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
            "serving phase7 planner scaffold query"
        );

        let context = PlannerRuntimeContext {
            symbol_available: true,
            semantic_available: loaded.config.semantic.enabled,
            impact_available: loaded.config.impact.enabled,
            exact_available: false,
        };

        PlannerWorkspace::default()
            .plan(&context, params, snapshot)
            .map_err(map_planner_error)
    }
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
    use hyperindex_protocol::planner::{PlannerIntentKind, PlannerQueryParams, PlannerQueryText};
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
    fn planner_service_returns_scaffold_response() {
        let response = PlannerService
            .query(
                &loaded_config(),
                &snapshot(),
                &PlannerQueryParams {
                    repo_id: "repo-123".to_string(),
                    snapshot_id: "snap-123".to_string(),
                    query: PlannerQueryText {
                        text: "where do we invalidate sessions?".to_string(),
                    },
                    intent_hint: Some(PlannerIntentKind::Hybrid),
                    path_globs: vec!["packages/**".to_string()],
                    limit: 8,
                    include_trace: true,
                },
            )
            .unwrap();

        assert_eq!(response.repo_id, "repo-123");
        assert!(response.trace.is_some());
        assert!(response.groups.is_empty());
    }
}
