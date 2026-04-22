use hyperindex_protocol::planner::{
    PlannerBudgetPolicy, PlannerDiagnostic, PlannerDiagnosticSeverity, PlannerRouteBudget,
    PlannerRouteKind,
};

pub const PLANNER_PHASE: &str = "phase7";
pub const PLANNER_LAYOUT_VERSION: &str = "phase7-planner-scaffold-v1";

pub fn normalize_query(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn planner_version() -> String {
    PLANNER_LAYOUT_VERSION.to_string()
}

pub fn scaffold_info(code: impl Into<String>, message: impl Into<String>) -> PlannerDiagnostic {
    PlannerDiagnostic {
        severity: PlannerDiagnosticSeverity::Info,
        code: code.into(),
        message: message.into(),
    }
}

pub fn scaffold_warning(code: impl Into<String>, message: impl Into<String>) -> PlannerDiagnostic {
    PlannerDiagnostic {
        severity: PlannerDiagnosticSeverity::Warning,
        code: code.into(),
        message: message.into(),
    }
}

pub fn budget_for_route(
    policy: &PlannerBudgetPolicy,
    route_kind: PlannerRouteKind,
) -> PlannerRouteBudget {
    policy
        .route_budgets
        .iter()
        .find(|budget| budget.route_kind == route_kind)
        .cloned()
        .unwrap_or_else(|| PlannerRouteBudget {
            route_kind,
            max_candidates: 10,
            max_groups: policy.max_groups,
            timeout_ms: policy.total_timeout_ms,
        })
}
