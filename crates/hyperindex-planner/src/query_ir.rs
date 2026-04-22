use hyperindex_protocol::planner::{PlannerIntentDecision, PlannerQueryIr, PlannerQueryParams};
use hyperindex_protocol::snapshot::ComposedSnapshot;

use crate::common::normalize_query;
use crate::planner_model::{PlannerError, PlannerResult};

#[derive(Debug, Default, Clone)]
pub struct QueryIrBuilder;

impl QueryIrBuilder {
    pub fn build(
        &self,
        params: &PlannerQueryParams,
        snapshot: &ComposedSnapshot,
        intent: &PlannerIntentDecision,
    ) -> PlannerResult<PlannerQueryIr> {
        let normalized_query = normalize_query(&params.query.text);
        if normalized_query.is_empty() {
            return Err(PlannerError::InvalidQuery(
                "planner query text must not be empty".to_string(),
            ));
        }

        Ok(PlannerQueryIr {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            surface_query: params.query.text.clone(),
            normalized_query,
            intent: intent.selected_intent.clone(),
            limit: params.limit.max(1),
            path_globs: params.path_globs.clone(),
        })
    }
}
