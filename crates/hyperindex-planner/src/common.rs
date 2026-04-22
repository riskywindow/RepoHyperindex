use hyperindex_protocol::planner::{
    PlannerDiagnostic, PlannerDiagnosticSeverity, PlannerRouteBudget, PlannerRouteKind,
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

pub fn default_budget(route_kind: PlannerRouteKind) -> PlannerRouteBudget {
    match route_kind {
        PlannerRouteKind::Exact => PlannerRouteBudget {
            route_kind,
            max_candidates: 25,
            max_groups: 10,
        },
        PlannerRouteKind::Symbol => PlannerRouteBudget {
            route_kind,
            max_candidates: 20,
            max_groups: 10,
        },
        PlannerRouteKind::Semantic => PlannerRouteBudget {
            route_kind,
            max_candidates: 16,
            max_groups: 8,
        },
        PlannerRouteKind::Impact => PlannerRouteBudget {
            route_kind,
            max_candidates: 8,
            max_groups: 4,
        },
    }
}
