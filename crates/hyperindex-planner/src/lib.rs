pub mod cli_integration;
pub mod common;
pub mod daemon_integration;
pub mod exact_route;
pub mod intent_router;
pub mod planner_engine;
pub mod planner_model;
pub mod query_ir;
pub mod result_grouping;
pub mod route_registry;
pub mod score_fusion;
pub mod trust_payloads;

pub use daemon_integration::PlannerRuntimeContext;
pub use planner_model::{PlannerError, PlannerResult, PlannerWorkspace};

#[cfg(test)]
mod tests {
    use hyperindex_protocol::planner::{
        PlannerContextRef, PlannerExactMatchStyle, PlannerIntentSignal, PlannerMode,
        PlannerModeSelectionSource, PlannerQueryFilters, PlannerQueryParams, PlannerQueryStyle,
        PlannerRouteHints, PlannerRouteKind, PlannerRouteStatus, PlannerUserQuery,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::SymbolKind;
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use crate::{PlannerRuntimeContext, PlannerWorkspace};

    fn scaffold_snapshot() -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            repo_id: "repo-123".to_string(),
            repo_root: "/tmp/repo".to_string(),
            snapshot_id: "snap-123".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "deadbeef".to_string(),
                digest: "base".to_string(),
                file_count: 0,
                files: Vec::new(),
            },
            working_tree: WorkingTreeOverlay {
                digest: "working-tree".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    fn plan_query(
        mut params: PlannerQueryParams,
    ) -> hyperindex_protocol::planner::PlannerQueryResponse {
        params.repo_id = "repo-123".to_string();
        params.snapshot_id = "snap-123".to_string();

        PlannerWorkspace::default()
            .plan(
                &PlannerRuntimeContext::default(),
                &params,
                &scaffold_snapshot(),
            )
            .unwrap()
    }

    fn query_params(text: &str) -> PlannerQueryParams {
        PlannerQueryParams {
            repo_id: String::new(),
            snapshot_id: String::new(),
            query: PlannerUserQuery {
                text: text.to_string(),
            },
            mode_override: None,
            selected_context: None,
            target_context: None,
            filters: PlannerQueryFilters::default(),
            route_hints: PlannerRouteHints::default(),
            budgets: None,
            limit: 10,
            explain: false,
            include_trace: true,
        }
    }

    fn selected_routes(
        response: &hyperindex_protocol::planner::PlannerQueryResponse,
    ) -> Vec<PlannerRouteKind> {
        response.ir.planned_routes.clone()
    }

    #[test]
    fn identifier_queries_route_symbol_first() {
        let response = plan_query(query_params("SessionStore"));

        assert_eq!(response.mode.selected_mode, PlannerMode::Symbol);
        assert_eq!(response.ir.primary_style, PlannerQueryStyle::SymbolLookup);
        assert_eq!(
            response.ir.candidate_styles,
            vec![PlannerQueryStyle::SymbolLookup]
        );
        assert_eq!(
            response.ir.symbol_query.as_ref().unwrap().segments,
            vec!["SessionStore".to_string()]
        );
        assert_eq!(
            selected_routes(&response),
            vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic]
        );
    }

    #[test]
    fn regex_queries_route_exact_first() {
        let response = plan_query(query_params("/invalidate(Session|Token)/"));

        assert_eq!(response.mode.selected_mode, PlannerMode::Exact);
        assert_eq!(response.ir.primary_style, PlannerQueryStyle::ExactLookup);
        assert_eq!(
            response.ir.exact_query.as_ref().unwrap().match_style,
            PlannerExactMatchStyle::Regex
        );
        assert_eq!(
            selected_routes(&response),
            vec![
                PlannerRouteKind::Exact,
                PlannerRouteKind::Symbol,
                PlannerRouteKind::Semantic
            ]
        );
    }

    #[test]
    fn natural_language_queries_route_semantic_first() {
        let response = plan_query(query_params("where is the session middleware created"));

        assert_eq!(response.mode.selected_mode, PlannerMode::Semantic);
        assert_eq!(response.ir.primary_style, PlannerQueryStyle::SemanticLookup);
        assert_eq!(
            response.ir.candidate_styles,
            vec![PlannerQueryStyle::SemanticLookup]
        );
        assert_eq!(
            response.ir.semantic_query.as_ref().unwrap().tokens,
            vec![
                "where".to_string(),
                "is".to_string(),
                "the".to_string(),
                "session".to_string(),
                "middleware".to_string(),
                "created".to_string(),
            ]
        );
        assert_eq!(
            selected_routes(&response),
            vec![PlannerRouteKind::Semantic, PlannerRouteKind::Symbol]
        );
    }

    #[test]
    fn blast_radius_queries_plan_seed_resolution_then_impact() {
        let response = plan_query(query_params("what breaks if I rename SessionStore"));

        assert_eq!(response.mode.selected_mode, PlannerMode::Impact);
        assert_eq!(response.ir.primary_style, PlannerQueryStyle::ImpactAnalysis);
        assert_eq!(
            response.ir.impact_query.as_ref().unwrap().action_terms,
            vec!["breaks".to_string(), "rename".to_string()]
        );
        assert_eq!(
            response.ir.impact_query.as_ref().unwrap().subject_terms,
            vec!["SessionStore".to_string()]
        );
        assert_eq!(
            selected_routes(&response),
            vec![
                PlannerRouteKind::Symbol,
                PlannerRouteKind::Semantic,
                PlannerRouteKind::Impact
            ]
        );
    }

    #[test]
    fn mixed_queries_keep_multiple_candidate_styles() {
        let response = plan_query(query_params("where is SessionStore.invalidateSession used"));

        assert_eq!(response.mode.selected_mode, PlannerMode::Symbol);
        assert_eq!(
            response.ir.candidate_styles,
            vec![
                PlannerQueryStyle::SymbolLookup,
                PlannerQueryStyle::SemanticLookup
            ]
        );
        assert_eq!(
            selected_routes(&response),
            vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic]
        );
    }

    #[test]
    fn explicit_mode_override_overrides_heuristics() {
        let mut params = query_params("where do we invalidate sessions?");
        params.mode_override = Some(PlannerMode::Exact);
        params.include_trace = false;

        let response = plan_query(params);

        assert_eq!(response.mode.selected_mode, PlannerMode::Exact);
        assert_eq!(
            response.mode.source,
            PlannerModeSelectionSource::ExplicitOverride
        );
        assert_eq!(
            response.ir.candidate_styles,
            vec![PlannerQueryStyle::ExactLookup]
        );
        assert!(
            response
                .ir
                .intent_signals
                .contains(&PlannerIntentSignal::ExplicitModeOverride)
        );
        assert!(response.trace.is_none());
    }

    #[test]
    fn selected_file_context_biases_impact_style_queries() {
        let mut params = query_params("what breaks if I change this file");
        params.selected_context = Some(PlannerContextRef::File {
            path: "packages/auth/src/session/service.ts".to_string(),
        });

        let response = plan_query(params);

        assert_eq!(response.mode.selected_mode, PlannerMode::Impact);
        assert!(
            response
                .ir
                .intent_signals
                .contains(&PlannerIntentSignal::SelectedFileContext)
        );
        assert_eq!(
            selected_routes(&response),
            vec![
                PlannerRouteKind::Symbol,
                PlannerRouteKind::Semantic,
                PlannerRouteKind::Impact
            ]
        );
    }

    #[test]
    fn filters_and_hints_are_normalized_in_ir() {
        let mut params = query_params("packages/auth/src/session/service.ts");
        params.filters = PlannerQueryFilters {
            path_globs: vec![
                " apps/** ".to_string(),
                "packages/**".to_string(),
                "apps/**".to_string(),
            ],
            package_names: vec![
                "@hyperindex/auth".to_string(),
                "@hyperindex/auth".to_string(),
            ],
            package_roots: vec!["packages/auth".to_string(), "packages/auth".to_string()],
            workspace_roots: vec![".".to_string(), ".".to_string()],
            languages: Vec::new(),
            extensions: vec![".TS".to_string(), "ts".to_string(), "tsx".to_string()],
            symbol_kinds: vec![SymbolKind::Method, SymbolKind::Function, SymbolKind::Method],
        };
        params.route_hints = PlannerRouteHints {
            preferred_routes: vec![
                PlannerRouteKind::Semantic,
                PlannerRouteKind::Symbol,
                PlannerRouteKind::Semantic,
            ],
            disabled_routes: vec![PlannerRouteKind::Exact, PlannerRouteKind::Exact],
            require_exact_seed: true,
        };

        let response = plan_query(params);

        assert_eq!(
            response.ir.filters.path_globs,
            vec!["apps/**".to_string(), "packages/**".to_string()]
        );
        assert_eq!(
            response.ir.filters.extensions,
            vec!["ts".to_string(), "tsx".to_string()]
        );
        assert_eq!(
            response.ir.filters.symbol_kinds,
            vec![SymbolKind::Function, SymbolKind::Method]
        );
        assert_eq!(
            response.ir.route_hints.preferred_routes,
            vec![PlannerRouteKind::Semantic, PlannerRouteKind::Symbol]
        );
        assert_eq!(
            response.ir.route_hints.disabled_routes,
            vec![PlannerRouteKind::Exact]
        );
        assert_eq!(
            selected_routes(&response),
            vec![PlannerRouteKind::Semantic, PlannerRouteKind::Symbol]
        );
        assert!(
            response
                .trace
                .as_ref()
                .unwrap()
                .routes
                .iter()
                .any(|route| route.route_kind == PlannerRouteKind::Exact
                    && route.status == PlannerRouteStatus::Skipped)
        );
    }

    #[test]
    fn hero_query_keeps_impact_primary_with_semantic_fallback() {
        let response = plan_query(query_params("where do we invalidate sessions?"));

        assert_eq!(response.mode.selected_mode, PlannerMode::Impact);
        assert_eq!(response.ir.primary_style, PlannerQueryStyle::ImpactAnalysis);
        assert_eq!(
            response.ir.candidate_styles,
            vec![
                PlannerQueryStyle::ImpactAnalysis,
                PlannerQueryStyle::SemanticLookup
            ]
        );
        assert_eq!(
            response.ir.impact_query.as_ref().unwrap().action_terms,
            vec!["invalidate".to_string()]
        );
        assert_eq!(
            response.ir.impact_query.as_ref().unwrap().subject_terms,
            vec!["sessions".to_string()]
        );
        assert_eq!(
            selected_routes(&response),
            vec![
                PlannerRouteKind::Symbol,
                PlannerRouteKind::Semantic,
                PlannerRouteKind::Impact
            ]
        );
    }
}
