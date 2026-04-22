use hyperindex_protocol::planner::{
    PlannerBudgetPolicy, PlannerModeDecision, PlannerQueryIr, PlannerQueryParams,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;

use crate::common::normalize_query;
use crate::daemon_integration::PlannerRuntimeContext;
use crate::planner_model::{PlannerError, PlannerResult};

#[derive(Debug, Default, Clone)]
pub struct QueryIrBuilder;

impl QueryIrBuilder {
    pub fn build(
        &self,
        context: &PlannerRuntimeContext,
        params: &PlannerQueryParams,
        snapshot: &ComposedSnapshot,
        mode: &PlannerModeDecision,
    ) -> PlannerResult<PlannerQueryIr> {
        let normalized_query = normalize_query(&params.query.text);
        if normalized_query.is_empty() {
            return Err(PlannerError::InvalidQuery(
                "planner query text must not be empty".to_string(),
            ));
        }

        let budgets = merge_budget_policy(&context.budget_policy, params.budgets.as_ref());
        let limit = params.limit.max(1).min(context.max_limit);

        Ok(PlannerQueryIr {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            surface_query: params.query.text.clone(),
            normalized_query,
            selected_mode: mode.selected_mode.clone(),
            limit,
            selected_context: params.selected_context.clone(),
            target_context: params.target_context.clone(),
            filters: params.filters.clone(),
            route_hints: params.route_hints.clone(),
            budgets,
        })
    }
}

fn merge_budget_policy(
    defaults: &PlannerBudgetPolicy,
    overrides: Option<&hyperindex_protocol::planner::PlannerBudgetHints>,
) -> PlannerBudgetPolicy {
    let Some(overrides) = overrides else {
        return defaults.clone();
    };

    let mut policy = defaults.clone();
    if let Some(total_timeout_ms) = overrides.total_timeout_ms {
        policy.total_timeout_ms = total_timeout_ms;
    }
    if let Some(max_groups) = overrides.max_groups {
        policy.max_groups = max_groups;
    }
    if !overrides.route_budgets.is_empty() {
        policy.route_budgets = overrides.route_budgets.clone();
    }
    policy
}
