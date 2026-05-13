use std::path::Path;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_planner::cli_integration::{
    render_capabilities_response, render_explain_response, render_query_response,
    render_status_response,
};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::planner::{
    PlannerCapabilitiesParams, PlannerContextRef, PlannerExplainParams, PlannerMode,
    PlannerQueryFilters, PlannerQueryParams, PlannerRouteHints, PlannerStatusParams,
    PlannerUserQuery,
};
use hyperindex_protocol::symbols::SymbolId;

use crate::client::DaemonClient;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryContextInput {
    pub file: Option<String>,
    pub symbol_id: Option<String>,
    pub symbol_path: Option<String>,
    pub symbol_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedQueryCommandInput {
    pub repo_id: String,
    pub snapshot_id: String,
    pub query: String,
    pub mode_override: Option<String>,
    pub limit: u32,
    pub path_globs: Vec<String>,
    pub selected_context: QueryContextInput,
    pub target_context: QueryContextInput,
    pub include_trace: bool,
}

pub fn query(
    config_path: Option<&Path>,
    input: &UnifiedQueryCommandInput,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::PlannerQuery(build_query_params(input, false)?))? {
        SuccessPayload::PlannerQuery(response) => {
            render_query_response(&response, json_output).map_err(render_error)
        }
        other => Err(unexpected_response("planner_query", other)),
    }
}

pub fn explain(
    config_path: Option<&Path>,
    input: &UnifiedQueryCommandInput,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::PlannerExplain(PlannerExplainParams {
        query: build_query_params(input, true)?,
    }))? {
        SuccessPayload::PlannerExplain(response) => {
            render_explain_response(&response, json_output).map_err(render_error)
        }
        other => Err(unexpected_response("planner_explain", other)),
    }
}

pub fn status(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::PlannerStatus(PlannerStatusParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
    }))? {
        SuccessPayload::PlannerStatus(response) => {
            render_status_response(&response, json_output).map_err(render_error)
        }
        other => Err(unexpected_response("planner_status", other)),
    }
}

pub fn capabilities(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::PlannerCapabilities(
        PlannerCapabilitiesParams {
            repo_id: repo_id.to_string(),
            snapshot_id: snapshot_id.to_string(),
        },
    ))? {
        SuccessPayload::PlannerCapabilities(response) => {
            render_capabilities_response(&response, json_output).map_err(render_error)
        }
        other => Err(unexpected_response("planner_capabilities", other)),
    }
}

fn build_query_params(
    input: &UnifiedQueryCommandInput,
    explain: bool,
) -> HyperindexResult<PlannerQueryParams> {
    Ok(PlannerQueryParams {
        repo_id: input.repo_id.clone(),
        snapshot_id: input.snapshot_id.clone(),
        query: PlannerUserQuery {
            text: input.query.clone(),
        },
        mode_override: parse_mode_override(input.mode_override.as_deref())?,
        selected_context: query_context("selected", &input.selected_context)?,
        target_context: query_context("target", &input.target_context)?,
        filters: PlannerQueryFilters {
            path_globs: input.path_globs.clone(),
            package_names: Vec::new(),
            package_roots: Vec::new(),
            workspace_roots: Vec::new(),
            languages: Vec::new(),
            extensions: Vec::new(),
            symbol_kinds: Vec::new(),
        },
        route_hints: PlannerRouteHints::default(),
        budgets: None,
        limit: input.limit,
        explain,
        include_trace: explain || input.include_trace,
    })
}

fn query_context(
    label: &str,
    input: &QueryContextInput,
) -> HyperindexResult<Option<PlannerContextRef>> {
    if let Some(path) = input.file.as_deref() {
        if input.symbol_id.is_some() || input.symbol_path.is_some() || input.symbol_name.is_some() {
            return Err(HyperindexError::Message(format!(
                "{label} planner context must be either --{label}-file or the pair --{label}-symbol-id with --{label}-symbol-path"
            )));
        }
        return Ok(Some(PlannerContextRef::File {
            path: path.to_string(),
        }));
    }

    match (input.symbol_id.as_deref(), input.symbol_path.as_deref()) {
        (None, None) => {
            if input.symbol_name.is_some() {
                return Err(HyperindexError::Message(format!(
                    "{label} planner context requires --{label}-symbol-id and --{label}-symbol-path when --{label}-symbol-name is set"
                )));
            }
            Ok(None)
        }
        (Some(symbol_id), Some(path)) => Ok(Some(PlannerContextRef::Symbol {
            symbol_id: SymbolId(symbol_id.to_string()),
            path: path.to_string(),
            span: None,
            display_name: input.symbol_name.clone(),
        })),
        _ => Err(HyperindexError::Message(format!(
            "{label} planner context must be either --{label}-file or the pair --{label}-symbol-id with --{label}-symbol-path"
        ))),
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
            "unsupported planner mode {other}; expected auto, exact, symbol, semantic, or impact"
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

    use super::{
        QueryContextInput, UnifiedQueryCommandInput, build_query_params, parse_mode_override,
        query_context,
    };
    use hyperindex_planner::cli_integration::render_query_response;

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

    #[test]
    fn query_context_supports_file_and_symbol_inputs() {
        let file = query_context(
            "selected",
            &QueryContextInput {
                file: Some("packages/auth/src/session/service.ts".to_string()),
                ..QueryContextInput::default()
            },
        )
        .unwrap();
        assert!(matches!(
            file,
            Some(hyperindex_protocol::planner::PlannerContextRef::File { .. })
        ));

        let symbol = query_context(
            "selected",
            &QueryContextInput {
                symbol_id: Some("sym-123".to_string()),
                symbol_path: Some("packages/auth/src/session/service.ts".to_string()),
                symbol_name: Some("invalidateSession".to_string()),
                ..QueryContextInput::default()
            },
        )
        .unwrap();
        assert!(matches!(
            symbol,
            Some(hyperindex_protocol::planner::PlannerContextRef::Symbol { .. })
        ));
    }

    #[test]
    fn query_context_rejects_partial_symbol_inputs() {
        let error = query_context(
            "target",
            &QueryContextInput {
                symbol_id: Some("sym-123".to_string()),
                ..QueryContextInput::default()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("--target-symbol-id"));
    }

    #[test]
    fn build_query_params_carries_context_and_trace_flags() {
        let params = build_query_params(
            &UnifiedQueryCommandInput {
                repo_id: "repo-123".to_string(),
                snapshot_id: "snap-123".to_string(),
                query: "what breaks if I change this".to_string(),
                mode_override: Some("impact".to_string()),
                limit: 5,
                path_globs: vec!["packages/**".to_string()],
                selected_context: QueryContextInput {
                    symbol_id: Some("sym-123".to_string()),
                    symbol_path: Some("packages/auth/src/session/service.ts".to_string()),
                    symbol_name: Some("invalidateSession".to_string()),
                    ..QueryContextInput::default()
                },
                target_context: QueryContextInput::default(),
                include_trace: false,
            },
            true,
        )
        .unwrap();

        assert_eq!(params.mode_override, Some(PlannerMode::Impact));
        assert!(params.explain);
        assert!(params.include_trace);
        assert!(matches!(
            params.selected_context,
            Some(hyperindex_protocol::planner::PlannerContextRef::Symbol { .. })
        ));
    }
}
