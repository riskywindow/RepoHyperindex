use hyperindex_protocol::planner::{
    PlannerBudgetPolicy, PlannerCapabilities, PlannerDiagnostic, PlannerFilterCapabilities,
    PlannerMode, PlannerQueryState, PlannerRouteCapability, PlannerRouteKind,
};

use crate::common::{scaffold_info, scaffold_warning};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerRuntimeContext {
    pub planner_enabled: bool,
    pub exact_enabled: bool,
    pub symbol_available: bool,
    pub semantic_available: bool,
    pub impact_available: bool,
    pub exact_available: bool,
    pub default_mode: PlannerMode,
    pub default_limit: u32,
    pub max_limit: u32,
    pub budget_policy: PlannerBudgetPolicy,
}

impl Default for PlannerRuntimeContext {
    fn default() -> Self {
        Self {
            planner_enabled: true,
            exact_enabled: true,
            symbol_available: true,
            semantic_available: true,
            impact_available: true,
            exact_available: false,
            default_mode: PlannerMode::Auto,
            default_limit: 10,
            max_limit: 50,
            budget_policy: PlannerBudgetPolicy {
                total_timeout_ms: 1_500,
                max_groups: 10,
                route_budgets: vec![
                    hyperindex_protocol::planner::PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Exact,
                        max_candidates: 25,
                        max_groups: 10,
                        timeout_ms: 150,
                    },
                    hyperindex_protocol::planner::PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Symbol,
                        max_candidates: 20,
                        max_groups: 10,
                        timeout_ms: 250,
                    },
                    hyperindex_protocol::planner::PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Semantic,
                        max_candidates: 16,
                        max_groups: 8,
                        timeout_ms: 400,
                    },
                    hyperindex_protocol::planner::PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Impact,
                        max_candidates: 8,
                        max_groups: 4,
                        timeout_ms: 600,
                    },
                ],
            },
        }
    }
}

impl PlannerRuntimeContext {
    pub fn route_enabled(&self, route_kind: PlannerRouteKind) -> bool {
        match route_kind {
            PlannerRouteKind::Exact => self.exact_enabled,
            PlannerRouteKind::Symbol => self.symbol_available,
            PlannerRouteKind::Semantic => self.semantic_available,
            PlannerRouteKind::Impact => self.impact_available,
        }
    }

    pub fn route_available(&self, route_kind: PlannerRouteKind) -> bool {
        match route_kind {
            PlannerRouteKind::Exact => self.exact_enabled && self.exact_available,
            PlannerRouteKind::Symbol => self.symbol_available,
            PlannerRouteKind::Semantic => self.semantic_available,
            PlannerRouteKind::Impact => self.impact_available,
        }
    }

    pub fn query_state(&self) -> PlannerQueryState {
        if !self.planner_enabled {
            PlannerQueryState::Disabled
        } else if self.symbol_available || self.semantic_available || self.impact_available {
            PlannerQueryState::Ready
        } else {
            PlannerQueryState::Degraded
        }
    }

    pub fn capabilities(&self) -> PlannerCapabilities {
        PlannerCapabilities {
            status: true,
            query: self.planner_enabled,
            explain: self.planner_enabled,
            trace: self.planner_enabled,
            explicit_mode_override: true,
            modes: vec![
                PlannerMode::Auto,
                PlannerMode::Exact,
                PlannerMode::Symbol,
                PlannerMode::Semantic,
                PlannerMode::Impact,
            ],
            filters: PlannerFilterCapabilities {
                path_globs: true,
                package_names: true,
                package_roots: true,
                workspace_roots: true,
                languages: true,
                extensions: true,
                symbol_kinds: true,
            },
            routes: vec![
                self.route_capability(PlannerRouteKind::Exact),
                self.route_capability(PlannerRouteKind::Symbol),
                self.route_capability(PlannerRouteKind::Semantic),
                self.route_capability(PlannerRouteKind::Impact),
            ],
        }
    }

    fn route_capability(&self, route_kind: PlannerRouteKind) -> PlannerRouteCapability {
        let available = self.route_available(route_kind.clone());
        let reason = match &route_kind {
            PlannerRouteKind::Exact if !self.exact_available => {
                Some("exact route boundary exists but no exact engine ships yet".to_string())
            }
            PlannerRouteKind::Symbol if !self.symbol_available => {
                Some("symbol route is disabled by runtime configuration".to_string())
            }
            PlannerRouteKind::Semantic if !self.semantic_available => {
                Some("semantic route is disabled by runtime configuration".to_string())
            }
            PlannerRouteKind::Impact if !self.impact_available => {
                Some("impact route is disabled by runtime configuration".to_string())
            }
            _ => None,
        };

        PlannerRouteCapability {
            route_kind: route_kind.clone(),
            enabled: self.route_enabled(route_kind),
            available,
            reason,
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
    if !context.planner_enabled {
        diagnostics.push(scaffold_warning(
            "planner_disabled",
            "planner configuration is disabled; query and explain responses will remain empty",
        ));
    }
    diagnostics
}
