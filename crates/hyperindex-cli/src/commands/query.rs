use std::path::Path;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_planner::cli_integration::render_query_response;
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::planner::{
    PlannerMode, PlannerQueryFilters, PlannerQueryParams, PlannerRouteHints, PlannerUserQuery,
};

use crate::client::DaemonClient;

pub fn query(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    query: &str,
    mode_override: Option<&str>,
    limit: u32,
    path_globs: Vec<String>,
    include_trace: bool,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::PlannerQuery(PlannerQueryParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        query: PlannerUserQuery {
            text: query.to_string(),
        },
        mode_override: parse_mode_override(mode_override)?,
        selected_context: None,
        target_context: None,
        filters: PlannerQueryFilters {
            path_globs,
            package_names: Vec::new(),
            package_roots: Vec::new(),
            workspace_roots: Vec::new(),
            languages: Vec::new(),
            extensions: Vec::new(),
            symbol_kinds: Vec::new(),
        },
        route_hints: PlannerRouteHints::default(),
        budgets: None,
        limit,
        explain: false,
        include_trace,
    }))? {
        SuccessPayload::PlannerQuery(response) => {
            render_query_response(&response, json_output).map_err(render_error)
        }
        other => Err(unexpected_response("planner_query", other)),
    }
}

fn parse_mode_override(raw: Option<&str>) -> HyperindexResult<Option<PlannerMode>> {
    match raw {
        None => Ok(None),
        Some("auto") => Ok(Some(PlannerMode::Auto)),
        Some("exact") => Ok(Some(PlannerMode::Exact)),
        Some("symbol") => Ok(Some(PlannerMode::Symbol)),
        Some("semantic") => Ok(Some(PlannerMode::Semantic)),
        Some("impact") => Ok(Some(PlannerMode::Impact)),
        Some(other) => Err(HyperindexError::Message(format!(
            "unsupported planner mode override {other}; expected auto, exact, symbol, semantic, or impact"
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
        PlannerBudgetPolicy, PlannerImpactQueryIntent, PlannerIntentSignal, PlannerMode,
        PlannerModeDecision, PlannerModeSelectionSource, PlannerNoAnswer, PlannerNoAnswerReason,
        PlannerQueryFilters, PlannerQueryIr, PlannerQueryResponse, PlannerQueryStats,
        PlannerQueryStyle, PlannerRouteBudget, PlannerRouteKind, PlannerSemanticQueryIntent,
    };

    use super::{parse_mode_override, render_query_response};

    #[test]
    fn parse_mode_override_accepts_supported_values() {
        assert_eq!(
            parse_mode_override(Some("symbol")).unwrap(),
            Some(PlannerMode::Symbol)
        );
        assert!(parse_mode_override(Some("unknown")).is_err());
    }

    #[test]
    fn render_query_response_covers_human_and_json_modes() {
        let response = PlannerQueryResponse {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            mode: PlannerModeDecision {
                requested_mode: Some(PlannerMode::Auto),
                selected_mode: PlannerMode::Semantic,
                source: PlannerModeSelectionSource::Heuristic,
                reasons: vec!["scaffold".to_string()],
            },
            ir: PlannerQueryIr {
                repo_id: "repo-123".to_string(),
                snapshot_id: "snap-123".to_string(),
                surface_query: "where do we invalidate sessions?".to_string(),
                normalized_query: "where do we invalidate sessions?".to_string(),
                selected_mode: PlannerMode::Semantic,
                primary_style: PlannerQueryStyle::SemanticLookup,
                candidate_styles: vec![
                    PlannerQueryStyle::SemanticLookup,
                    PlannerQueryStyle::ImpactAnalysis,
                ],
                planned_routes: vec![
                    PlannerRouteKind::Semantic,
                    PlannerRouteKind::Symbol,
                    PlannerRouteKind::Impact,
                ],
                intent_signals: vec![
                    PlannerIntentSignal::NaturalLanguageQuestion,
                    PlannerIntentSignal::MultiTokenQuery,
                    PlannerIntentSignal::ImpactPhrase,
                ],
                limit: 10,
                selected_context: None,
                target_context: None,
                exact_query: None,
                symbol_query: None,
                semantic_query: Some(PlannerSemanticQueryIntent {
                    normalized_text: "where do we invalidate sessions?".to_string(),
                    tokens: vec![
                        "where".to_string(),
                        "do".to_string(),
                        "we".to_string(),
                        "invalidate".to_string(),
                        "sessions".to_string(),
                    ],
                }),
                impact_query: Some(PlannerImpactQueryIntent {
                    normalized_text: "where do we invalidate sessions?".to_string(),
                    action_terms: vec!["invalidate".to_string()],
                    subject_terms: vec!["sessions".to_string()],
                }),
                filters: PlannerQueryFilters {
                    path_globs: vec!["packages/**".to_string()],
                    package_names: Vec::new(),
                    package_roots: Vec::new(),
                    workspace_roots: Vec::new(),
                    languages: Vec::new(),
                    extensions: Vec::new(),
                    symbol_kinds: Vec::new(),
                },
                route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                budgets: PlannerBudgetPolicy {
                    total_timeout_ms: 1_500,
                    max_groups: 10,
                    route_budgets: vec![PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Semantic,
                        max_candidates: 16,
                        max_groups: 8,
                        timeout_ms: 400,
                    }],
                },
            },
            groups: Vec::new(),
            diagnostics: Vec::new(),
            trace: None,
            no_answer: Some(PlannerNoAnswer {
                reason: PlannerNoAnswerReason::ExecutionDeferred,
                details: vec!["contract only".to_string()],
            }),
            ambiguity: None,
            stats: PlannerQueryStats {
                limit_requested: 10,
                routes_considered: 4,
                routes_available: 3,
                candidates_considered: 0,
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
