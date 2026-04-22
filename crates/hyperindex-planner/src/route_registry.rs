use hyperindex_protocol::planner::{
    PlannerIntentKind, PlannerQueryIr, PlannerRouteKind, PlannerRouteStatus, PlannerRouteTrace,
    PlannerSkipReason,
};

use crate::common::default_budget;
use crate::daemon_integration::PlannerRuntimeContext;
use crate::exact_route::UnavailableExactRouteProvider;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerRoutePlan {
    pub traces: Vec<PlannerRouteTrace>,
}

impl PlannerRoutePlan {
    pub fn routes_considered(&self) -> u32 {
        self.traces.len() as u32
    }

    pub fn routes_available(&self) -> u32 {
        self.traces.iter().filter(|trace| trace.available).count() as u32
    }
}

#[derive(Debug, Clone)]
pub struct PlannerRouteRegistry {
    exact_route: UnavailableExactRouteProvider,
}

impl Default for PlannerRouteRegistry {
    fn default() -> Self {
        Self {
            exact_route: UnavailableExactRouteProvider,
        }
    }
}

impl PlannerRouteRegistry {
    pub fn plan(&self, context: &PlannerRuntimeContext, ir: &PlannerQueryIr) -> PlannerRoutePlan {
        let route_order = route_order(&ir.intent);
        let traces = route_order
            .into_iter()
            .map(|route_kind| {
                let available = if matches!(route_kind, PlannerRouteKind::Exact) {
                    self.exact_route.available()
                } else {
                    context.route_available(route_kind.clone())
                };

                if available {
                    PlannerRouteTrace {
                        route_kind: route_kind.clone(),
                        available,
                        status: PlannerRouteStatus::Planned,
                        skip_reason: None,
                        budget: Some(default_budget(route_kind)),
                        notes: vec![
                            "route registered in the Phase 7 scaffold".to_string(),
                            "execution is deferred until engine integration lands".to_string(),
                        ],
                    }
                } else {
                    let skip_reason = if matches!(route_kind, PlannerRouteKind::Exact) {
                        Some(PlannerSkipReason::ExactEngineUnavailable)
                    } else {
                        Some(PlannerSkipReason::CapabilityDisabled)
                    };
                    let note = if matches!(route_kind, PlannerRouteKind::Exact) {
                        self.exact_route.unavailable_reason().to_string()
                    } else {
                        "route disabled by runtime context".to_string()
                    };
                    PlannerRouteTrace {
                        route_kind: route_kind.clone(),
                        available,
                        status: PlannerRouteStatus::Skipped,
                        skip_reason,
                        budget: Some(default_budget(route_kind)),
                        notes: vec![note],
                    }
                }
            })
            .collect();

        PlannerRoutePlan { traces }
    }
}

fn route_order(intent: &PlannerIntentKind) -> Vec<PlannerRouteKind> {
    match intent {
        PlannerIntentKind::Lookup => vec![
            PlannerRouteKind::Exact,
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Semantic,
        ],
        PlannerIntentKind::Semantic => {
            vec![PlannerRouteKind::Semantic, PlannerRouteKind::Symbol]
        }
        PlannerIntentKind::Impact => {
            vec![PlannerRouteKind::Impact, PlannerRouteKind::Symbol]
        }
        PlannerIntentKind::Hybrid => vec![
            PlannerRouteKind::Semantic,
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Impact,
            PlannerRouteKind::Exact,
        ],
    }
}
