use hyperindex_protocol::planner::PlannerQueryResponse;

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

    if !response.diagnostics.is_empty() {
        lines.push("diagnostics:".to_string());
        for diagnostic in &response.diagnostics {
            lines.push(format!("- {}: {}", diagnostic.code, diagnostic.message));
        }
    }

    if let Some(trace) = &response.trace {
        lines.push("trace:".to_string());
        for route in &trace.routes {
            lines.push(format!(
                "- {:?}: {:?} (available={}, selected={})",
                route.route_kind, route.status, route.available, route.selected
            ));
        }
    }

    if let Some(no_answer) = &response.no_answer {
        lines.push(format!("no_answer: {:?}", no_answer.reason));
    }

    Ok(lines.join("\n"))
}
