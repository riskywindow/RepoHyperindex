use hyperindex_protocol::planner::PlannerRouteKind;
use hyperindex_protocol::snapshot::ComposedSnapshot;

use crate::common::scaffold_info;
use crate::daemon_integration::PlannerRuntimeContext;
use crate::route_adapters::{
    PlannerRouteAdapter, PlannerRouteCapabilityReport, PlannerRouteConstraints,
    PlannerRouteExecution, PlannerRouteExecutionState, PlannerRouteReadiness, PlannerRouteRequest,
    empty_filter_capabilities,
};

#[derive(Debug, Clone, Default)]
pub struct UnavailableExactRouteProvider;

impl UnavailableExactRouteProvider {
    pub fn unavailable_reason(&self) -> &'static str {
        "exact route remains a typed compatibility boundary in Phase 7"
    }
}

impl PlannerRouteAdapter for UnavailableExactRouteProvider {
    fn kind(&self) -> PlannerRouteKind {
        PlannerRouteKind::Exact
    }

    fn capability(
        &self,
        context: &PlannerRuntimeContext,
        _snapshot: &ComposedSnapshot,
    ) -> PlannerRouteCapabilityReport {
        let enabled = context.route_enabled(PlannerRouteKind::Exact);
        PlannerRouteCapabilityReport {
            route_kind: PlannerRouteKind::Exact,
            enabled,
            available: false,
            readiness: if enabled {
                PlannerRouteReadiness::Unavailable
            } else {
                PlannerRouteReadiness::Disabled
            },
            reason: Some(self.unavailable_reason().to_string()),
            supported_filters: empty_filter_capabilities(),
            constraints: PlannerRouteConstraints::default(),
            diagnostics: vec![scaffold_info(
                "exact_route_unavailable",
                self.unavailable_reason(),
            )],
            notes: vec![self.unavailable_reason().to_string()],
        }
    }

    fn execute(&self, _request: &PlannerRouteRequest<'_>) -> PlannerRouteExecution {
        PlannerRouteExecution {
            state: PlannerRouteExecutionState::Deferred,
            candidates: Vec::new(),
            diagnostics: vec![scaffold_info(
                "exact_route_execution_deferred",
                self.unavailable_reason(),
            )],
            notes: vec![self.unavailable_reason().to_string()],
            elapsed_ms: 0,
            ambiguity: None,
        }
    }
}
