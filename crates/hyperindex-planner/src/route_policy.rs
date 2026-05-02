use hyperindex_protocol::planner::{
    PlannerContextRef, PlannerExactMatchStyle, PlannerIntentSignal, PlannerQueryIr,
    PlannerQueryStyle, PlannerRouteKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannerRoutePolicyKind {
    SingleRoute,
    StagedFallback,
    MultiRouteCandidates,
    SeedThenImpact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerRoutePolicy {
    pub kind: PlannerRoutePolicyKind,
    pub selected_routes: Vec<PlannerRouteKind>,
    pub low_signal: bool,
    pub notes: Vec<String>,
}

pub fn plan_route_policy(ir: &PlannerQueryIr) -> PlannerRoutePolicy {
    if has_signal(ir, PlannerIntentSignal::ExplicitModeOverride) {
        return single_route_policy(
            ir.planned_routes.first().cloned().into_iter().collect(),
            "explicit mode override bypassed auto routing",
            false,
        );
    }

    if matches!(ir.primary_style, PlannerQueryStyle::ImpactAnalysis) {
        if impact_has_direct_seed(ir) {
            return single_route_policy(
                route_if_present(ir, PlannerRouteKind::Impact),
                "impact analysis can run directly from selected symbol/file context or a concrete file seed",
                false,
            );
        }

        if ir.symbol_query.is_some() {
            let mut routes = Vec::new();
            push_if_present(ir, &mut routes, PlannerRouteKind::Symbol);
            push_if_present(ir, &mut routes, PlannerRouteKind::Semantic);
            push_if_present(ir, &mut routes, PlannerRouteKind::Impact);
            return with_kind(
                PlannerRoutePolicyKind::SeedThenImpact,
                routes,
                false,
                "impact analysis needs deterministic seed resolution before the impact engine is consulted",
            );
        }

        let mut routes = Vec::new();
        push_if_present(ir, &mut routes, PlannerRouteKind::Semantic);
        if ir.symbol_query.is_some() {
            push_if_present(ir, &mut routes, PlannerRouteKind::Symbol);
        }
        if routes.is_empty() {
            push_if_present(ir, &mut routes, PlannerRouteKind::Impact);
        }
        return with_kind(
            PlannerRoutePolicyKind::MultiRouteCandidates,
            routes,
            true,
            "impact-like wording lacked a deterministic symbol or file seed, so the planner stayed on candidate-retrieval routes",
        );
    }

    if ir.candidate_styles.len() > 1 {
        return with_kind(
            PlannerRoutePolicyKind::MultiRouteCandidates,
            ir.planned_routes.clone(),
            is_low_signal_query(ir),
            "mixed route signals kept multiple route candidates live",
        );
    }

    with_kind(
        PlannerRoutePolicyKind::StagedFallback,
        ir.planned_routes.clone(),
        is_low_signal_query(ir),
        "one dominant route stayed primary while lower-priority fallbacks remained available",
    )
}

fn single_route_policy(
    routes: Vec<PlannerRouteKind>,
    note: &'static str,
    low_signal: bool,
) -> PlannerRoutePolicy {
    with_kind(
        PlannerRoutePolicyKind::SingleRoute,
        routes,
        low_signal,
        note,
    )
}

fn with_kind(
    preferred_kind: PlannerRoutePolicyKind,
    routes: Vec<PlannerRouteKind>,
    low_signal: bool,
    note: &'static str,
) -> PlannerRoutePolicy {
    let kind = if routes.len() <= 1 {
        PlannerRoutePolicyKind::SingleRoute
    } else {
        preferred_kind
    };

    PlannerRoutePolicy {
        kind,
        selected_routes: routes,
        low_signal,
        notes: vec![note.to_string()],
    }
}

fn impact_has_direct_seed(ir: &PlannerQueryIr) -> bool {
    has_direct_context_seed(ir.selected_context.as_ref())
        || has_direct_context_seed(ir.target_context.as_ref())
        || matches!(
            ir.exact_query.as_ref().map(|query| &query.match_style),
            Some(PlannerExactMatchStyle::Path)
        )
        || ir
            .impact_query
            .as_ref()
            .map(|query| looks_like_path(&query.subject_terms.join(" ")))
            .unwrap_or(false)
}

fn has_direct_context_seed(context: Option<&PlannerContextRef>) -> bool {
    matches!(
        context,
        Some(
            PlannerContextRef::Symbol { .. }
                | PlannerContextRef::Span { .. }
                | PlannerContextRef::File { .. }
                | PlannerContextRef::Impact { .. }
        )
    )
}

fn is_low_signal_query(ir: &PlannerQueryIr) -> bool {
    matches!(ir.primary_style, PlannerQueryStyle::SemanticLookup)
        && ir.selected_context.is_none()
        && ir.target_context.is_none()
        && ir.exact_query.is_none()
        && ir.symbol_query.is_none()
        && ir.impact_query.is_none()
        && ir
            .semantic_query
            .as_ref()
            .map(|query| query.tokens.len() <= 3)
            .unwrap_or(true)
}

fn has_signal(ir: &PlannerQueryIr, signal: PlannerIntentSignal) -> bool {
    ir.intent_signals.contains(&signal)
}

fn route_if_present(ir: &PlannerQueryIr, route_kind: PlannerRouteKind) -> Vec<PlannerRouteKind> {
    ir.planned_routes
        .iter()
        .find(|planned| **planned == route_kind)
        .cloned()
        .into_iter()
        .collect()
}

fn push_if_present(
    ir: &PlannerQueryIr,
    routes: &mut Vec<PlannerRouteKind>,
    route_kind: PlannerRouteKind,
) {
    if ir.planned_routes.contains(&route_kind) && !routes.contains(&route_kind) {
        routes.push(route_kind);
    }
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || value.rsplit_once('.').is_some()
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::planner::{
        PlannerContextRef, PlannerImpactQueryIntent, PlannerIntentSignal, PlannerMode,
        PlannerQueryFilters, PlannerQueryIr, PlannerQueryStyle, PlannerRouteHints,
        PlannerRouteKind, PlannerSemanticQueryIntent,
    };

    use super::{PlannerRoutePolicyKind, plan_route_policy};

    fn ir() -> PlannerQueryIr {
        PlannerQueryIr {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            surface_query: "where is invalidateSession used".to_string(),
            normalized_query: "where is invalidateSession used".to_string(),
            selected_mode: PlannerMode::Semantic,
            primary_style: PlannerQueryStyle::SemanticLookup,
            candidate_styles: vec![PlannerQueryStyle::SemanticLookup],
            planned_routes: vec![PlannerRouteKind::Semantic, PlannerRouteKind::Symbol],
            intent_signals: Vec::new(),
            limit: 10,
            selected_context: None,
            target_context: None,
            exact_query: None,
            symbol_query: None,
            semantic_query: Some(PlannerSemanticQueryIntent {
                normalized_text: "where is invalidateSession used".to_string(),
                tokens: vec![
                    "where".to_string(),
                    "is".to_string(),
                    "invalidatesession".to_string(),
                    "used".to_string(),
                ],
            }),
            impact_query: None,
            filters: PlannerQueryFilters::default(),
            route_hints: PlannerRouteHints::default(),
            budgets: crate::daemon_integration::PlannerRuntimeContext::default().budget_policy,
        }
    }

    #[test]
    fn explicit_override_collapses_to_one_route() {
        let mut query_ir = ir();
        query_ir.selected_mode = PlannerMode::Exact;
        query_ir.primary_style = PlannerQueryStyle::ExactLookup;
        query_ir.planned_routes = vec![
            PlannerRouteKind::Exact,
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Semantic,
        ];
        query_ir
            .intent_signals
            .push(PlannerIntentSignal::ExplicitModeOverride);

        let policy = plan_route_policy(&query_ir);

        assert_eq!(policy.kind, PlannerRoutePolicyKind::SingleRoute);
        assert_eq!(policy.selected_routes, vec![PlannerRouteKind::Exact]);
    }

    #[test]
    fn mixed_queries_keep_multi_route_candidates_live() {
        let mut query_ir = ir();
        query_ir.candidate_styles = vec![
            PlannerQueryStyle::SymbolLookup,
            PlannerQueryStyle::SemanticLookup,
        ];
        query_ir.primary_style = PlannerQueryStyle::SymbolLookup;
        query_ir.planned_routes = vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic];

        let policy = plan_route_policy(&query_ir);

        assert_eq!(policy.kind, PlannerRoutePolicyKind::MultiRouteCandidates);
        assert_eq!(
            policy.selected_routes,
            vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic]
        );
    }

    #[test]
    fn impact_queries_with_symbol_seed_stage_into_impact() {
        let mut query_ir = ir();
        query_ir.selected_mode = PlannerMode::Impact;
        query_ir.primary_style = PlannerQueryStyle::ImpactAnalysis;
        query_ir.candidate_styles = vec![PlannerQueryStyle::ImpactAnalysis];
        query_ir.planned_routes = vec![
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Semantic,
            PlannerRouteKind::Impact,
        ];
        query_ir.symbol_query = Some(hyperindex_protocol::planner::PlannerSymbolQueryIntent {
            normalized_symbol: "SessionStore".to_string(),
            segments: vec!["SessionStore".to_string()],
        });
        query_ir.impact_query = Some(PlannerImpactQueryIntent {
            normalized_text: "what breaks if I rename SessionStore".to_string(),
            action_terms: vec!["rename".to_string()],
            subject_terms: vec!["SessionStore".to_string()],
        });

        let policy = plan_route_policy(&query_ir);

        assert_eq!(policy.kind, PlannerRoutePolicyKind::SeedThenImpact);
        assert_eq!(
            policy.selected_routes,
            vec![
                PlannerRouteKind::Symbol,
                PlannerRouteKind::Semantic,
                PlannerRouteKind::Impact
            ]
        );
    }

    #[test]
    fn selected_file_context_goes_directly_to_impact() {
        let mut query_ir = ir();
        query_ir.selected_mode = PlannerMode::Impact;
        query_ir.primary_style = PlannerQueryStyle::ImpactAnalysis;
        query_ir.planned_routes = vec![
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Semantic,
            PlannerRouteKind::Impact,
        ];
        query_ir.selected_context = Some(PlannerContextRef::File {
            path: "packages/auth/src/session/service.ts".to_string(),
        });

        let policy = plan_route_policy(&query_ir);

        assert_eq!(policy.kind, PlannerRoutePolicyKind::SingleRoute);
        assert_eq!(policy.selected_routes, vec![PlannerRouteKind::Impact]);
        assert!(!policy.low_signal);
    }

    #[test]
    fn low_signal_impact_queries_stay_off_impact_without_seed() {
        let mut query_ir = ir();
        query_ir.selected_mode = PlannerMode::Impact;
        query_ir.primary_style = PlannerQueryStyle::ImpactAnalysis;
        query_ir.candidate_styles = vec![
            PlannerQueryStyle::ImpactAnalysis,
            PlannerQueryStyle::SemanticLookup,
        ];
        query_ir.planned_routes = vec![
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Semantic,
            PlannerRouteKind::Impact,
        ];
        query_ir.impact_query = Some(PlannerImpactQueryIntent {
            normalized_text: "where do we invalidate sessions".to_string(),
            action_terms: vec!["invalidate".to_string()],
            subject_terms: vec!["sessions".to_string()],
        });

        let policy = plan_route_policy(&query_ir);

        assert_eq!(policy.kind, PlannerRoutePolicyKind::SingleRoute);
        assert_eq!(policy.selected_routes, vec![PlannerRouteKind::Semantic]);
        assert!(policy.low_signal);
    }
}
