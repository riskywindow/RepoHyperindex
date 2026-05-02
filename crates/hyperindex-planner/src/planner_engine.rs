use hyperindex_protocol::planner::{
    PlannerCandidate, PlannerExplainResponse, PlannerModeDecision, PlannerNoAnswer,
    PlannerNoAnswerReason, PlannerQueryIr, PlannerQueryParams, PlannerQueryResponse,
    PlannerQueryStats, PlannerTrace, PlannerTraceStep,
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
    candidates: Vec<PlannerCandidate>,
    groups: Vec<hyperindex_protocol::planner::PlannerResultGroup>,
    diagnostics: Vec<hyperindex_protocol::planner::PlannerDiagnostic>,
    trace: PlannerTrace,
    no_answer: Option<PlannerNoAnswer>,
    ambiguity: Option<hyperindex_protocol::planner::PlannerAmbiguity>,
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
            no_answer: scaffold.no_answer,
            ambiguity: scaffold.ambiguity,
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
            candidates: scaffold.candidates,
            groups: scaffold.groups,
            diagnostics: scaffold.diagnostics,
            trace: Some(scaffold.trace),
            no_answer: scaffold.no_answer,
            ambiguity: scaffold.ambiguity,
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

        let fused_candidates = fusion.fuse(
            &route_plan.candidates,
            &route_plan.traces,
            &route_plan.route_policy,
        );
        let grouped = grouping.group(fused_candidates, params.limit);
        let groups = trust_payloads.decorate(grouped);
        let candidates = route_plan
            .candidates
            .iter()
            .map(|candidate| candidate.to_public_candidate())
            .collect::<Vec<_>>();

        let mut diagnostics = runtime_diagnostics(context);
        diagnostics.extend(route_plan.diagnostics.clone());
        if candidates.is_empty() && groups.is_empty() {
            diagnostics.push(scaffold_warning(
                "planner_execution_deferred",
                "planner route execution, score fusion, and grouping remain deferred in this contract slice",
            ));
        }
        diagnostics.push(scaffold_info(
            "planner_contract_ready",
            "planner protocol, config, daemon, and CLI surfaces are wired for Phase 7 implementation work",
        ));
        if !candidates.is_empty() {
            diagnostics.push(scaffold_info(
                "planner_candidates_materialized",
                format!(
                    "planner explain materialized {} normalized candidate(s) across {} route(s), fused into {} group(s)",
                    candidates.len(),
                    route_plan
                        .traces
                        .iter()
                        .filter(|trace| trace.status
                            == hyperindex_protocol::planner::PlannerRouteStatus::Executed)
                        .count(),
                    groups.len()
                ),
            ));
        }

        let routes_considered = route_plan.routes_considered();
        let routes_available = route_plan.routes_available();
        let groups_returned = groups.len() as u32;
        let no_answer = no_answer_for(context, params, &route_plan, groups_returned);
        let ambiguity = route_plan.ambiguity.clone();
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
                    code: "route_policy".to_string(),
                    message: format!(
                        "policy={:?} low_signal={} budget_exhausted={} early_stopped={} partial_results={}",
                        route_plan.route_policy,
                        route_plan.low_signal,
                        route_plan.budget_exhausted,
                        route_plan.early_stopped,
                        route_plan.partial_results
                    ),
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
                PlannerTraceStep {
                    code: "candidate_summary".to_string(),
                    message: format!(
                        "raw_candidates={} normalized_candidates={}",
                        route_plan.raw_candidate_count,
                        candidates.len()
                    ),
                },
            ],
            routes: route_plan.traces.clone(),
        };

        Ok(PlannerScaffoldOutput {
            mode,
            ir,
            candidates,
            groups,
            diagnostics,
            trace,
            no_answer,
            ambiguity,
            stats: PlannerQueryStats {
                limit_requested: params.limit,
                routes_considered,
                routes_available,
                candidates_considered: route_plan.candidates.len() as u32,
                groups_returned,
                elapsed_ms: 0,
            },
        })
    }
}

fn no_answer_for(
    context: &PlannerRuntimeContext,
    params: &PlannerQueryParams,
    route_plan: &PlannerRoutePlan,
    groups_returned: u32,
) -> Option<PlannerNoAnswer> {
    if route_plan.ambiguity.is_some() {
        return None;
    }
    if params.explain && !route_plan.candidates.is_empty() {
        return None;
    }
    if !context.planner_enabled {
        return Some(PlannerNoAnswer {
            reason: PlannerNoAnswerReason::PlannerDisabled,
            details: vec!["planner configuration is disabled for this runtime".to_string()],
        });
    }
    if groups_returned > 0 {
        return None;
    }
    if !route_plan.candidates.is_empty() {
        return Some(PlannerNoAnswer {
            reason: PlannerNoAnswerReason::ExecutionDeferred,
            details: vec![
                "normalized candidates are available, but score fusion and grouping remain deferred"
                    .to_string(),
                "use planner_explain to inspect route-level candidates during bring-up"
                    .to_string(),
            ],
        });
    }
    if route_plan.selected_routes_available() == 0 {
        let mut details = vec!["no selected planner route is currently available".to_string()];
        if route_plan.budget_exhausted {
            details.push(
                "the configured route budget exhausted lower-priority routes before execution"
                    .to_string(),
            );
        }
        return Some(PlannerNoAnswer {
            reason: PlannerNoAnswerReason::NoRouteAvailable,
            details,
        });
    }
    if route_plan.has_deferred_routes() {
        return Some(PlannerNoAnswer {
            reason: PlannerNoAnswerReason::ExecutionDeferred,
            details: vec![
                "selected route execution remains deferred in this planner workspace".to_string(),
            ],
        });
    }
    if route_plan.raw_candidate_count > 0 && route_plan.candidates.is_empty() {
        return Some(PlannerNoAnswer {
            reason: PlannerNoAnswerReason::FiltersExcludedAllCandidates,
            details: vec![
                "route execution produced candidates, but planner filters removed them all"
                    .to_string(),
            ],
        });
    }
    let mut details = vec!["no selected route produced a normalized candidate".to_string()];
    if route_plan.low_signal {
        details.push(
            "add a quoted literal, exact symbol, file path, or selected context to ground the planner deterministically"
                .to_string(),
        );
    }
    if route_plan.budget_exhausted {
        details.push(
            "the planner route budget exhausted lower-priority routes before they were attempted"
                .to_string(),
        );
    }
    Some(PlannerNoAnswer {
        reason: PlannerNoAnswerReason::NoCandidateMatched,
        details,
    })
}
