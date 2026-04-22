use std::path::Path;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_planner::cli_integration::render_query_response;
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::planner::{PlannerIntentKind, PlannerQueryParams, PlannerQueryText};

use crate::client::DaemonClient;

pub fn query(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    query: &str,
    intent_hint: Option<&str>,
    limit: u32,
    path_globs: Vec<String>,
    include_trace: bool,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::PlannerQuery(PlannerQueryParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        query: PlannerQueryText {
            text: query.to_string(),
        },
        intent_hint: parse_intent_hint(intent_hint)?,
        path_globs,
        limit,
        include_trace,
    }))? {
        SuccessPayload::PlannerQuery(response) => {
            render_query_response(&response, json_output).map_err(render_error)
        }
        other => Err(unexpected_response("planner_query", other)),
    }
}

fn parse_intent_hint(raw: Option<&str>) -> HyperindexResult<Option<PlannerIntentKind>> {
    match raw {
        None => Ok(None),
        Some("lookup") => Ok(Some(PlannerIntentKind::Lookup)),
        Some("semantic") => Ok(Some(PlannerIntentKind::Semantic)),
        Some("impact") => Ok(Some(PlannerIntentKind::Impact)),
        Some("hybrid") => Ok(Some(PlannerIntentKind::Hybrid)),
        Some(other) => Err(HyperindexError::Message(format!(
            "unsupported planner intent hint {other}; expected lookup, semantic, impact, or hybrid"
        ))),
    }
}

fn render_error(error: serde_json::Error) -> HyperindexError {
    HyperindexError::Message(format!("failed to render planner response: {error}"))
}

fn unexpected_response(method: &str, payload: SuccessPayload) -> HyperindexError {
    HyperindexError::Message(format!(
        "expected {method} response but received {:?}",
        payload
    ))
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::planner::{
        PlannerIntentDecision, PlannerIntentKind, PlannerIntentSource, PlannerQueryIr,
        PlannerQueryResponse, PlannerQueryStats,
    };

    use super::{parse_intent_hint, render_query_response};

    #[test]
    fn parse_intent_hint_accepts_supported_values() {
        assert_eq!(
            parse_intent_hint(Some("hybrid")).unwrap(),
            Some(PlannerIntentKind::Hybrid)
        );
        assert!(parse_intent_hint(Some("unknown")).is_err());
    }

    #[test]
    fn render_query_response_covers_human_and_json_modes() {
        let response = PlannerQueryResponse {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            intent: PlannerIntentDecision {
                selected_intent: PlannerIntentKind::Hybrid,
                source: PlannerIntentSource::Heuristic,
                reasons: vec!["scaffold".to_string()],
            },
            ir: PlannerQueryIr {
                repo_id: "repo-123".to_string(),
                snapshot_id: "snap-123".to_string(),
                surface_query: "where do we invalidate sessions?".to_string(),
                normalized_query: "where do we invalidate sessions?".to_string(),
                intent: PlannerIntentKind::Hybrid,
                limit: 10,
                path_globs: vec!["packages/**".to_string()],
            },
            groups: Vec::new(),
            diagnostics: Vec::new(),
            trace: None,
            stats: PlannerQueryStats {
                limit_requested: 10,
                routes_considered: 4,
                routes_available: 3,
                groups_returned: 0,
                elapsed_ms: 0,
            },
        };

        let human = render_query_response(&response, false).unwrap();
        assert!(human.contains("planner query snap-123"));
        let json = render_query_response(&response, true).unwrap();
        assert!(json.contains("\"snapshot_id\": \"snap-123\""));
    }
}
