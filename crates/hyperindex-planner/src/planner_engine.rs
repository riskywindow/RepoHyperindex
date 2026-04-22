use hyperindex_protocol::planner::{
    PlannerExplainResponse, PlannerModeDecision, PlannerNoAnswer, PlannerNoAnswerReason,
    PlannerQueryIr, PlannerQueryParams, PlannerQueryResponse, PlannerQueryStats, PlannerTrace,
    PlannerTraceStep,
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

#[derive(Debug, Clone)]
struct PlannerScaffoldOutput {
    mode: PlannerModeDecision,
    ir: PlannerQueryIr,
    groups: Vec<hyperindex_protocol::planner::PlannerResultGroup>,
    diagnostics: Vec<hyperindex_protocol::planner::PlannerDiagnostic>,
    trace: PlannerTrace,
    no_answer: PlannerNoAnswer,
    stats: PlannerQueryStats,
}

#[derive(Debug, Default, Clone)]
pub struct PlannerEngine;

impl PlannerEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn query_scaffold(
        &self,
        workspace: &PlannerWorkspace,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
        params: &PlannerQueryParams,
        mode: PlannerModeDecision,
        ir: PlannerQueryIr,
        route_plan: PlannerRoutePlan,
        fusion: &ScoreFusion,
        grouping: &ResultGrouping,
        trust_payloads: &TrustPayloadFactory,
    ) -> PlannerResult<PlannerQueryResponse> {
        let scaffold = self.build_scaffold(
            workspace,
            context,
            snapshot,
            params,
            mode,
            ir,
            route_plan,
            fusion,
            grouping,
            trust_payloads,
        )?;

        Ok(PlannerQueryResponse {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            mode: scaffold.mode,
            ir: scaffold.ir,
            groups: scaffold.groups,
            diagnostics: scaffold.diagnostics,
            trace: (params.include_trace || params.explain).then_some(scaffold.trace),
            no_answer: Some(scaffold.no_answer),
            ambiguity: None,
            stats: scaffold.stats,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn explain_scaffold(
        &self,
        workspace: &PlannerWorkspace,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
        params: &PlannerQueryParams,
        mode: PlannerModeDecision,
        ir: PlannerQueryIr,
        route_plan: PlannerRoutePlan,
        fusion: &ScoreFusion,
        grouping: &ResultGrouping,
        trust_payloads: &TrustPayloadFactory,
    ) -> PlannerResult<PlannerExplainResponse> {
        let scaffold = self.build_scaffold(
            workspace,
            context,
            snapshot,
            params,
            mode,
            ir,
            route_plan,
            fusion,
            grouping,
            trust_payloads,
        )?;

        Ok(PlannerExplainResponse {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            mode: scaffold.mode,
            ir: scaffold.ir,
            candidates: Vec::new(),
            groups: scaffold.groups,
            diagnostics: scaffold.diagnostics,
            trace: Some(scaffold.trace),
            no_answer: Some(scaffold.no_answer),
            ambiguity: None,
            stats: scaffold.stats,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn build_scaffold(
        &self,
        _workspace: &PlannerWorkspace,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
        params: &PlannerQueryParams,
        mode: PlannerModeDecision,
        ir: PlannerQueryIr,
        route_plan: PlannerRoutePlan,
        fusion: &ScoreFusion,
        grouping: &ResultGrouping,
        trust_payloads: &TrustPayloadFactory,
    ) -> PlannerResult<PlannerScaffoldOutput> {
        info!(
            repo_id = %snapshot.repo_id,
            snapshot_id = %snapshot.snapshot_id,
            query = %ir.normalized_query,
            mode = ?mode.selected_mode,
            "planning phase7 unified query scaffold"
        );

        let fused_groups = fusion.fuse_placeholder(&route_plan.traces);
        let grouped = grouping.group_placeholder(fused_groups);
        let groups = trust_payloads.decorate_placeholder(grouped);

        let mut diagnostics = runtime_diagnostics(context);
        diagnostics.push(scaffold_warning(
            "planner_execution_deferred",
            "planner route execution, score fusion, and grouping remain deferred in this contract slice",
        ));
        diagnostics.push(scaffold_info(
            "planner_contract_ready",
            "planner protocol, config, daemon, and CLI surfaces are wired for Phase 7 implementation work",
        ));

        let routes_considered = route_plan.routes_considered();
        let routes_available = route_plan.routes_available();
        let groups_returned = groups.len() as u32;
        let trace = PlannerTrace {
            planner_version: planner_version(),
            selected_mode: mode.selected_mode.clone(),
            steps: vec![
                PlannerTraceStep {
                    code: "mode_selected".to_string(),
                    message: format!("selected_mode={:?}", mode.selected_mode),
                },
                PlannerTraceStep {
                    code: "query_styles".to_string(),
                    message: format!(
                        "primary_style={:?} candidate_styles={:?}",
                        ir.primary_style, ir.candidate_styles
                    ),
                },
                PlannerTraceStep {
                    code: "planned_routes".to_string(),
                    message: format!("planned_routes={:?}", ir.planned_routes),
                },
                PlannerTraceStep {
                    code: "snapshot_bound".to_string(),
                    message: format!("snapshot={}", snapshot.snapshot_id),
                },
                PlannerTraceStep {
                    code: "trace_request".to_string(),
                    message: format!(
                        "include_trace={} explain={}",
                        params.include_trace, params.explain
                    ),
                },
            ],
            routes: route_plan.traces,
        };

        Ok(PlannerScaffoldOutput {
            mode,
            ir,
            groups,
            diagnostics,
            trace,
            no_answer: PlannerNoAnswer {
                reason: PlannerNoAnswerReason::ExecutionDeferred,
                details: vec![
                    "The public planner contract is implemented before live route execution."
                        .to_string(),
                    "Use planner_explain for deterministic trace inspection during bring-up."
                        .to_string(),
                ],
            },
            stats: PlannerQueryStats {
                limit_requested: params.limit,
                routes_considered,
                routes_available,
                candidates_considered: 0,
                groups_returned,
                elapsed_ms: 0,
            },
        })
    }
}
