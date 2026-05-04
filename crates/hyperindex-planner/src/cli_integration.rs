use hyperindex_protocol::planner::{
    PlannerCapabilitiesResponse, PlannerExplainResponse, PlannerQueryResponse,
    PlannerStatusResponse,
};

pub fn render_query_response(
    response: &PlannerQueryResponse,
    json_output: bool,
) -> Result<String, serde_json::Error> {
    if json_output {
        return serde_json::to_string_pretty(response);
    }

    let mut lines = vec![
        format!("planner query {}", response.snapshot_id),
        format!(
            "mode: {:?} ({:?})",
            response.mode.selected_mode, response.mode.source
        ),
        format!(
            "routes: {}/{} available",
            response.stats.routes_available, response.stats.routes_considered
        ),
        format!("groups_returned: {}", response.stats.groups_returned),
    ];

    for group in &response.groups {
        lines.push(format!(
            "  [{:?}] {} (score={}, trust={:?}, routes={:?})",
            group.group_id,
            group.label,
            group.score.unwrap_or(0),
            group.trust.tier,
            group.routes,
        ));
        lines.push(format!("    {}", group.explanation.summary));
        for evidence in &group.evidence {
            let path = evidence.path.as_deref().unwrap_or("-");
            lines.push(format!(
                "    evidence: {:?} {} (score={})",
                evidence.evidence_kind,
                path,
                evidence.score.unwrap_or(0),
            ));
        }
    }

    if !response.diagnostics.is_empty() {
        lines.push("diagnostics:".to_string());
        for diagnostic in &response.diagnostics {
            lines.push(format!("- {}: {}", diagnostic.code, diagnostic.message));
        }
    }

    if let Some(trace) = &response.trace {
        lines.push("trace:".to_string());
        for step in &trace.steps {
            lines.push(format!("  step: {} - {}", step.code, step.message));
        }
        for route in &trace.routes {
            lines.push(format!(
                "  route {:?}: {:?} (available={}, selected={}, candidates={}, elapsed={}ms)",
                route.route_kind,
                route.status,
                route.available,
                route.selected,
                route.candidate_count.unwrap_or(0),
                route.elapsed_ms.unwrap_or(0),
            ));
        }
    }

    if let Some(ambiguity) = &response.ambiguity {
        lines.push(format!("ambiguity: {:?}", ambiguity.reason));
        for detail in &ambiguity.details {
            lines.push(format!("  {detail}"));
        }
    }

    if let Some(no_answer) = &response.no_answer {
        lines.push(format!("no_answer: {:?}", no_answer.reason));
        for detail in &no_answer.details {
            lines.push(format!("  {detail}"));
        }
    }

    Ok(lines.join("\n"))
}

pub fn render_explain_response(
    response: &PlannerExplainResponse,
    json_output: bool,
) -> Result<String, serde_json::Error> {
    if json_output {
        return serde_json::to_string_pretty(response);
    }

    let mut lines = vec![
        format!("planner explain {}", response.snapshot_id),
        format!(
            "mode: {:?} ({:?})",
            response.mode.selected_mode, response.mode.source
        ),
        format!("query_ir:"),
        format!("  surface_query: \"{}\"", response.ir.surface_query),
        format!("  normalized_query: \"{}\"", response.ir.normalized_query),
        format!("  primary_style: {:?}", response.ir.primary_style),
        format!("  candidate_styles: {:?}", response.ir.candidate_styles),
        format!("  planned_routes: {:?}", response.ir.planned_routes),
        format!("  intent_signals: {:?}", response.ir.intent_signals),
    ];

    if !response.candidates.is_empty() {
        lines.push(format!("candidates: ({})", response.candidates.len()));
        for candidate in &response.candidates {
            lines.push(format!(
                "  [{:?}] {} (route={:?}, score={}, normalized={})",
                candidate.candidate_id,
                candidate.label,
                candidate.route_kind,
                candidate.route_score.unwrap_or(0),
                candidate.normalized_score.unwrap_or(0),
            ));
            for evidence in &candidate.evidence {
                let path = evidence.path.as_deref().unwrap_or("-");
                lines.push(format!(
                    "    evidence: {:?} {} (score={})",
                    evidence.evidence_kind,
                    path,
                    evidence.score.unwrap_or(0),
                ));
            }
        }
    }

    if !response.groups.is_empty() {
        lines.push(format!("groups: ({})", response.groups.len()));
        for group in &response.groups {
            lines.push(format!(
                "  [{:?}] {} (score={}, trust={:?}, routes={:?})",
                group.group_id,
                group.label,
                group.score.unwrap_or(0),
                group.trust.tier,
                group.routes,
            ));
            lines.push(format!("    {}", group.explanation.summary));
            if !group.trust.reasons.is_empty() {
                for reason in &group.trust.reasons {
                    lines.push(format!("    trust: {reason}"));
                }
            }
            if !group.trust.warnings.is_empty() {
                for warning in &group.trust.warnings {
                    lines.push(format!("    warning: {warning}"));
                }
            }
        }
    }

    if !response.diagnostics.is_empty() {
        lines.push("diagnostics:".to_string());
        for diagnostic in &response.diagnostics {
            lines.push(format!(
                "  [{:?}] {}: {}",
                diagnostic.severity, diagnostic.code, diagnostic.message
            ));
        }
    }

    if let Some(trace) = &response.trace {
        lines.push("trace:".to_string());
        lines.push(format!(
            "  planner_version: {}",
            trace.planner_version
        ));
        lines.push(format!("  selected_mode: {:?}", trace.selected_mode));
        for step in &trace.steps {
            lines.push(format!("  step: {} - {}", step.code, step.message));
        }
        for route in &trace.routes {
            lines.push(format!(
                "  route {:?}: {:?} (available={}, selected={}, candidates={}, elapsed={}ms)",
                route.route_kind,
                route.status,
                route.available,
                route.selected,
                route.candidate_count.unwrap_or(0),
                route.elapsed_ms.unwrap_or(0),
            ));
            if let Some(skip_reason) = &route.skip_reason {
                lines.push(format!("    skip_reason: {:?}", skip_reason));
            }
            for note in &route.notes {
                lines.push(format!("    note: {note}"));
            }
        }
    }

    if let Some(ambiguity) = &response.ambiguity {
        lines.push(format!("ambiguity: {:?}", ambiguity.reason));
        for detail in &ambiguity.details {
            lines.push(format!("  {detail}"));
        }
    }

    if let Some(no_answer) = &response.no_answer {
        lines.push(format!("no_answer: {:?}", no_answer.reason));
        for detail in &no_answer.details {
            lines.push(format!("  {detail}"));
        }
    }

    lines.push(format!(
        "stats: candidates_considered={}, groups_returned={}, elapsed={}ms",
        response.stats.candidates_considered,
        response.stats.groups_returned,
        response.stats.elapsed_ms,
    ));

    Ok(lines.join("\n"))
}

pub fn render_status_response(
    response: &PlannerStatusResponse,
    json_output: bool,
) -> Result<String, serde_json::Error> {
    if json_output {
        return serde_json::to_string_pretty(response);
    }

    let mut lines = vec![
        format!("planner status {}", response.snapshot_id),
        format!("state: {:?}", response.state),
    ];

    for route in &response.capabilities.routes {
        let status = if route.available {
            "ready"
        } else if route.enabled {
            "enabled (not ready)"
        } else {
            "disabled"
        };
        lines.push(format!("  {:?}: {}", route.route_kind, status));
        if let Some(reason) = &route.reason {
            lines.push(format!("    {reason}"));
        }
    }

    if !response.diagnostics.is_empty() {
        lines.push("diagnostics:".to_string());
        for diagnostic in &response.diagnostics {
            lines.push(format!(
                "  [{:?}] {}: {}",
                diagnostic.severity, diagnostic.code, diagnostic.message
            ));
        }
    }

    Ok(lines.join("\n"))
}

pub fn render_capabilities_response(
    response: &PlannerCapabilitiesResponse,
    json_output: bool,
) -> Result<String, serde_json::Error> {
    if json_output {
        return serde_json::to_string_pretty(response);
    }

    let mut lines = vec![
        format!("planner capabilities {}", response.snapshot_id),
        format!("default_mode: {:?}", response.default_mode),
        format!(
            "limits: default={}, max={}",
            response.default_limit, response.max_limit
        ),
        format!(
            "budgets: timeout={}ms, max_groups={}",
            response.budgets.total_timeout_ms, response.budgets.max_groups
        ),
    ];

    lines.push("capabilities:".to_string());
    lines.push(format!(
        "  status={}, query={}, explain={}, trace={}",
        response.capabilities.status,
        response.capabilities.query,
        response.capabilities.explain,
        response.capabilities.trace,
    ));
    lines.push(format!(
        "  modes: {:?}",
        response.capabilities.modes
    ));

    lines.push("routes:".to_string());
    for route in &response.capabilities.routes {
        let status = if route.available {
            "ready"
        } else if route.enabled {
            "enabled (not ready)"
        } else {
            "disabled"
        };
        lines.push(format!("  {:?}: {}", route.route_kind, status));
        if let Some(reason) = &route.reason {
            lines.push(format!("    {reason}"));
        }
    }

    let f = &response.capabilities.filters;
    lines.push("filters:".to_string());
    lines.push(format!(
        "  path_globs={}, languages={}, extensions={}, symbol_kinds={}, packages={}, workspaces={}",
        f.path_globs, f.languages, f.extensions, f.symbol_kinds, f.package_names, f.workspace_roots,
    ));

    if !response.budgets.route_budgets.is_empty() {
        lines.push("route_budgets:".to_string());
        for budget in &response.budgets.route_budgets {
            lines.push(format!(
                "  {:?}: max_candidates={}, max_groups={}, timeout={}ms",
                budget.route_kind,
                budget.max_candidates,
                budget.max_groups,
                budget.timeout_ms,
            ));
        }
    }

    if !response.diagnostics.is_empty() {
        lines.push("diagnostics:".to_string());
        for diagnostic in &response.diagnostics {
            lines.push(format!(
                "  [{:?}] {}: {}",
                diagnostic.severity, diagnostic.code, diagnostic.message
            ));
        }
    }

    Ok(lines.join("\n"))
}
