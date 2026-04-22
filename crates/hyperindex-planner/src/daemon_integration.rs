use hyperindex_protocol::planner::{PlannerDiagnostic, PlannerRouteKind};

use crate::common::{scaffold_info, scaffold_warning};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerRuntimeContext {
    pub symbol_available: bool,
    pub semantic_available: bool,
    pub impact_available: bool,
    pub exact_available: bool,
}

impl Default for PlannerRuntimeContext {
    fn default() -> Self {
        Self {
            symbol_available: true,
            semantic_available: true,
            impact_available: true,
            exact_available: false,
        }
    }
}

impl PlannerRuntimeContext {
    pub fn route_available(&self, route_kind: PlannerRouteKind) -> bool {
        match route_kind {
            PlannerRouteKind::Exact => self.exact_available,
            PlannerRouteKind::Symbol => self.symbol_available,
            PlannerRouteKind::Semantic => self.semantic_available,
            PlannerRouteKind::Impact => self.impact_available,
        }
    }
}

pub fn runtime_diagnostics(context: &PlannerRuntimeContext) -> Vec<PlannerDiagnostic> {
    let mut diagnostics = vec![scaffold_info(
        "planner_runtime_scaffold",
        "planner runtime glue is scaffolded and remains snapshot-scoped",
    )];
    if !context.exact_available {
        diagnostics.push(scaffold_warning(
            "exact_route_unavailable",
            "exact route remains unavailable in the current repository",
        ));
    }
    diagnostics
}
