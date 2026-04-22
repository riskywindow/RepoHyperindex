use hyperindex_protocol::planner::{
    PlannerIntentDecision, PlannerQueryIr, PlannerQueryParams, PlannerQueryResponse,
    PlannerQueryStats, PlannerTrace,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use tracing::info;

use crate::common::{planner_version, scaffold_info, scaffold_warning};
use crate::daemon_integration::{PlannerRuntimeContext, runtime_diagnostics};
use crate::planner_model::{PlannerResult, PlannerWorkspace};
use crate::result_grouping::ResultGrouping;
use crate::route_registry::PlannerRoutePlan;
use crate::score_fusion::ScoreFusion;
use crate::trust_payloads::TrustPayloadFactory;

#[derive(Debug, Default, Clone)]
pub struct PlannerEngine;

impl PlannerEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn plan_scaffold(
        &self,
        _workspace: &PlannerWorkspace,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
        params: &PlannerQueryParams,
        intent: PlannerIntentDecision,
        ir: PlannerQueryIr,
        route_plan: PlannerRoutePlan,
        fusion: &ScoreFusion,
        grouping: &ResultGrouping,
        trust_payloads: &TrustPayloadFactory,
    ) -> PlannerResult<PlannerQueryResponse> {
        info!(
            repo_id = %snapshot.repo_id,
            snapshot_id = %snapshot.snapshot_id,
            query = %ir.normalized_query,
            intent = ?intent.selected_intent,
            "planning phase7 scaffold query"
        );

        let fused_groups = fusion.fuse_placeholder(&route_plan.traces);
        let grouped = grouping.group_placeholder(fused_groups);
        let groups = trust_payloads.decorate_placeholder(grouped);

        let mut diagnostics = runtime_diagnostics(context);
        diagnostics.push(scaffold_warning(
            "planner_scaffold_only",
            "planner route execution, score fusion, and grouping remain deferred in this scaffold",
        ));
        diagnostics.push(scaffold_info(
            "planner_front_door_wired",
            "planner protocol, daemon, and CLI front-door glue are wired for Phase 7 scaffolding",
        ));

        let routes_considered = route_plan.routes_considered();
        let routes_available = route_plan.routes_available();
        let groups_returned = groups.len() as u32;
        let trace = PlannerTrace {
            planner_version: planner_version(),
            events: vec![
                format!("snapshot={}", snapshot.snapshot_id),
                format!("intent_source={:?}", intent.source),
                format!("routes_considered={routes_considered}"),
            ],
            routes: route_plan.traces,
        };

        Ok(PlannerQueryResponse {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            intent,
            ir,
            groups,
            diagnostics,
            trace: params.include_trace.then_some(trace),
            stats: PlannerQueryStats {
                limit_requested: params.limit,
                routes_considered,
                routes_available,
                groups_returned,
                elapsed_ms: 0,
            },
        })
    }
}
