use hyperindex_protocol::planner::{
    PlannerMode, PlannerQueryIr, PlannerRouteKind, PlannerRouteSkipReason, PlannerRouteStatus,
    PlannerRouteTrace,
};

use crate::common::budget_for_route;
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
        let route_order = route_order(&ir.selected_mode, &ir.route_hints.preferred_routes);
        let selected_route = route_order
            .iter()
            .find(|route_kind| !ir.route_hints.disabled_routes.contains(route_kind))
            .cloned();

        let traces = route_order
            .into_iter()
            .map(|route_kind| {
                let budget = Some(budget_for_route(&ir.budgets, route_kind.clone()));
                let disabled_by_hint = ir.route_hints.disabled_routes.contains(&route_kind);
                let available = if matches!(route_kind, PlannerRouteKind::Exact) {
                    context.route_available(route_kind.clone()) && self.exact_route.available()
                } else {
                    context.route_available(route_kind.clone())
                };
                let selected = selected_route.as_ref() == Some(&route_kind);

                if disabled_by_hint {
                    return PlannerRouteTrace {
                        route_kind,
                        available: false,
                        selected: false,
                        status: PlannerRouteStatus::Skipped,
                        skip_reason: Some(PlannerRouteSkipReason::FilteredByRouteHint),
                        budget,
                        candidate_count: None,
                        group_count: None,
                        elapsed_ms: None,
                        notes: vec!["route disabled by planner route hints".to_string()],
                    };
                }

                if available {
                    PlannerRouteTrace {
                        route_kind,
                        available,
                        selected,
                        status: PlannerRouteStatus::Deferred,
                        skip_reason: Some(PlannerRouteSkipReason::ExecutionDeferred),
                        budget,
                        candidate_count: None,
                        group_count: None,
                        elapsed_ms: None,
                        notes: vec![
                            "route registered in the Phase 7 public contract".to_string(),
                            "live route execution remains deferred".to_string(),
                        ],
                    }
                } else {
                    let (skip_reason, note) = if matches!(route_kind, PlannerRouteKind::Exact) {
                        (
                            if context.exact_enabled {
                                PlannerRouteSkipReason::ExactEngineUnavailable
                            } else {
                                PlannerRouteSkipReason::CapabilityDisabled
                            },
                            if context.exact_enabled {
                                self.exact_route.unavailable_reason().to_string()
                            } else {
                                "exact route disabled by runtime configuration".to_string()
                            },
                        )
                    } else {
                        (
                            PlannerRouteSkipReason::CapabilityDisabled,
                            "route disabled by runtime context".to_string(),
                        )
                    };

                    PlannerRouteTrace {
                        route_kind,
                        available,
                        selected,
                        status: PlannerRouteStatus::Skipped,
                        skip_reason: Some(skip_reason),
                        budget,
                        candidate_count: None,
                        group_count: None,
                        elapsed_ms: None,
                        notes: vec![note],
                    }
                }
            })
            .collect();

        PlannerRoutePlan { traces }
    }
}

fn route_order(
    selected_mode: &PlannerMode,
    preferred_routes: &[PlannerRouteKind],
) -> Vec<PlannerRouteKind> {
    let base = match selected_mode {
        PlannerMode::Auto => vec![
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Semantic,
            PlannerRouteKind::Impact,
            PlannerRouteKind::Exact,
        ],
        PlannerMode::Exact => vec![
            PlannerRouteKind::Exact,
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Semantic,
        ],
        PlannerMode::Symbol => vec![
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Exact,
            PlannerRouteKind::Semantic,
        ],
        PlannerMode::Semantic => vec![PlannerRouteKind::Semantic, PlannerRouteKind::Symbol],
        PlannerMode::Impact => vec![PlannerRouteKind::Impact, PlannerRouteKind::Symbol],
    };

    let mut ordered = Vec::new();
    if let Some(primary) = base.first().cloned() {
        ordered.push(primary);
    }
    for route_kind in preferred_routes {
        if !ordered.contains(route_kind) {
            ordered.push(route_kind.clone());
        }
    }
    for route_kind in base {
        if !ordered.contains(&route_kind) {
            ordered.push(route_kind);
        }
    }
    ordered
}
