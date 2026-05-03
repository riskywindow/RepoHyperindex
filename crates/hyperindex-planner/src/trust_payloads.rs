use hyperindex_protocol::planner::{
    PlannerAmbiguity, PlannerAmbiguityReason, PlannerAnchor, PlannerContextRef,
    PlannerEvidenceKind, PlannerQueryIr, PlannerResultGroup, PlannerTrustPayload,
    PlannerTrustTier,
};

use crate::route_registry::PlannerRoutePlan;

/// Context provided to the trust decorator for evidence-backed trust decisions.
#[derive(Debug)]
pub struct TrustDecoratorContext<'a> {
    pub route_plan: &'a PlannerRoutePlan,
    pub ir: &'a PlannerQueryIr,
}

#[derive(Debug, Default, Clone)]
pub struct TrustPayloadFactory;

impl TrustPayloadFactory {
    /// Decorates each result group with evidence-backed trust tiers,
    /// determinism flags, explanation details, and warnings.
    ///
    /// Every group receives a trust classification:
    /// - **direct**: a single deterministic route (exact or symbol with context) found this
    /// - **corroborated**: multiple independent routes agree on this result
    /// - **fallback-derived**: only semantic or impact routes matched; no structural confirmation
    /// - **low-signal**: the query lacked deterministic seeds; result quality is uncertain
    pub fn decorate(
        &self,
        mut groups: Vec<PlannerResultGroup>,
        ctx: &TrustDecoratorContext<'_>,
    ) -> DecorateOutput {
        let all_scores: Vec<u32> = groups.iter().filter_map(|g| g.score).collect();

        for (index, group) in groups.iter_mut().enumerate() {
            let next_score = all_scores.get(index + 1).copied();
            decorate_group(group, ctx, next_score);
        }

        let ambiguity = score_gap_ambiguity(&groups);

        DecorateOutput { groups, ambiguity }
    }
}

/// Output of trust decoration, including the decorated groups and any
/// score-gap ambiguity detected during decoration.
pub struct DecorateOutput {
    pub groups: Vec<PlannerResultGroup>,
    /// Ambiguity detected from score-gap analysis between the top result groups.
    /// Only populated when the top groups have nearly identical scores and
    /// different anchors, indicating the planner cannot confidently rank them.
    pub ambiguity: Option<PlannerAmbiguity>,
}

// ---------------------------------------------------------------------------
// Evidence analysis
// ---------------------------------------------------------------------------

struct EvidenceProfile {
    has_exact: bool,
    has_symbol: bool,
    has_semantic: bool,
    has_impact: bool,
    has_context_seed: bool,
}

impl EvidenceProfile {
    fn from_group(group: &PlannerResultGroup) -> Self {
        let mut profile = Self {
            has_exact: false,
            has_symbol: false,
            has_semantic: false,
            has_impact: false,
            has_context_seed: false,
        };
        for evidence in &group.evidence {
            match evidence.evidence_kind {
                PlannerEvidenceKind::ExactMatch => profile.has_exact = true,
                PlannerEvidenceKind::SymbolHit => profile.has_symbol = true,
                PlannerEvidenceKind::SemanticHit => profile.has_semantic = true,
                PlannerEvidenceKind::ImpactHit => profile.has_impact = true,
                PlannerEvidenceKind::ContextSeed => profile.has_context_seed = true,
                PlannerEvidenceKind::FilterMatch => {}
            }
        }
        profile
    }
}

// ---------------------------------------------------------------------------
// Trust tier computation
// ---------------------------------------------------------------------------

struct TierDecision {
    tier: PlannerTrustTier,
    deterministic: bool,
    template_id: String,
    reasons: Vec<String>,
}

fn compute_tier(
    profile: &EvidenceProfile,
    route_count: usize,
    low_signal: bool,
) -> TierDecision {
    // HIGH: exact + symbol corroboration
    if profile.has_exact && profile.has_symbol {
        return TierDecision {
            tier: PlannerTrustTier::High,
            deterministic: true,
            template_id: "planner.trust.corroborated".to_string(),
            reasons: vec![
                "exact and symbol routes both matched this result".to_string(),
            ],
        };
    }

    // HIGH: 3+ routes agree
    if route_count >= 3 {
        return TierDecision {
            tier: PlannerTrustTier::High,
            deterministic: profile.has_exact || profile.has_context_seed,
            template_id: "planner.trust.corroborated".to_string(),
            reasons: vec![format!(
                "{route_count} independent routes matched this result"
            )],
        };
    }

    // HIGH: exact match alone
    if profile.has_exact {
        return TierDecision {
            tier: PlannerTrustTier::High,
            deterministic: true,
            template_id: "planner.trust.direct".to_string(),
            reasons: vec!["exact match provides deterministic evidence".to_string()],
        };
    }

    // HIGH: context seed provides deterministic anchor
    if profile.has_context_seed {
        return TierDecision {
            tier: PlannerTrustTier::High,
            deterministic: true,
            template_id: "planner.trust.direct".to_string(),
            reasons: vec!["context seed provides deterministic evidence".to_string()],
        };
    }

    // MEDIUM: 2 routes agree
    if route_count >= 2 {
        return TierDecision {
            tier: PlannerTrustTier::Medium,
            deterministic: false,
            template_id: "planner.trust.corroborated".to_string(),
            reasons: vec![format!(
                "{route_count} independent routes matched this result"
            )],
        };
    }

    // MEDIUM: single symbol hit with good query signal
    if profile.has_symbol && !low_signal {
        return TierDecision {
            tier: PlannerTrustTier::Medium,
            deterministic: false,
            template_id: "planner.trust.direct".to_string(),
            reasons: vec!["symbol route matched structurally".to_string()],
        };
    }

    // LOW: symbol hit but query was low-signal
    if profile.has_symbol && low_signal {
        return TierDecision {
            tier: PlannerTrustTier::Low,
            deterministic: false,
            template_id: "planner.trust.low_signal".to_string(),
            reasons: vec!["symbol route matched but query was low-signal".to_string()],
        };
    }

    // LOW: semantic-only (no structural corroboration)
    if profile.has_semantic && !profile.has_symbol && !profile.has_exact {
        let mut reasons =
            vec!["only semantic similarity matched; no structural confirmation".to_string()];
        if profile.has_impact {
            reasons
                .push("impact evidence present but lacks structural anchor".to_string());
        }
        return TierDecision {
            tier: PlannerTrustTier::Low,
            deterministic: false,
            template_id: "planner.trust.fallback_derived".to_string(),
            reasons,
        };
    }

    // LOW: impact-only
    if profile.has_impact && !profile.has_symbol && !profile.has_exact && !profile.has_semantic {
        return TierDecision {
            tier: PlannerTrustTier::Low,
            deterministic: false,
            template_id: "planner.trust.fallback_derived".to_string(),
            reasons: vec![
                "only impact analysis matched; no structural or semantic confirmation".to_string(),
            ],
        };
    }

    // NEEDS_REVIEW: insufficient evidence to assess
    TierDecision {
        tier: PlannerTrustTier::NeedsReview,
        deterministic: false,
        template_id: "planner.trust.low_signal".to_string(),
        reasons: vec!["insufficient evidence to confidently assess this result".to_string()],
    }
}

// ---------------------------------------------------------------------------
// Group decoration
// ---------------------------------------------------------------------------

fn decorate_group(
    group: &mut PlannerResultGroup,
    ctx: &TrustDecoratorContext<'_>,
    next_score: Option<u32>,
) {
    let profile = EvidenceProfile::from_group(group);
    let route_count = group.routes.len();
    let evidence_count = group.evidence.len() as u32;
    let low_signal = ctx.route_plan.low_signal;

    let decision = compute_tier(&profile, route_count, low_signal);

    // --- Warnings ---
    let mut warnings = Vec::new();

    if let (Some(my_score), Some(next)) = (group.score, next_score) {
        if my_score > 0 && next > 0 {
            let gap = my_score.saturating_sub(next);
            if gap <= 3 {
                warnings.push(format!(
                    "score gap to next group is {gap}; results may be ambiguous"
                ));
            }
        }
    }

    if low_signal {
        warnings.push(
            "query was low-signal; add a quoted literal, symbol, or file context to improve confidence"
                .to_string(),
        );
    }

    if ctx.route_plan.partial_results {
        warnings.push(
            "some selected routes were not consulted; results may be incomplete".to_string(),
        );
    }

    if ctx.route_plan.budget_exhausted {
        warnings.push(
            "route budget was exhausted before all routes could be consulted".to_string(),
        );
    }

    // --- Explanation details ---
    let details = build_explanation_details(group, &profile, ctx);

    // --- Apply trust payload ---
    group.trust = PlannerTrustPayload {
        tier: decision.tier,
        deterministic: decision.deterministic,
        evidence_count,
        route_agreement_count: route_count as u32,
        template_id: decision.template_id,
        reasons: decision.reasons,
        warnings,
    };

    // --- Enhance explanation ---
    group.explanation.details = details;
    group.explanation.template_id =
        explanation_template_id(&group.trust.tier, route_count);
}

// ---------------------------------------------------------------------------
// Explanation details (template-based, no generative text)
// ---------------------------------------------------------------------------

fn build_explanation_details(
    group: &PlannerResultGroup,
    profile: &EvidenceProfile,
    ctx: &TrustDecoratorContext<'_>,
) -> Vec<String> {
    let mut details = Vec::new();

    // Route contributions
    let route_names: Vec<String> = group.routes.iter().map(|r| format!("{r:?}")).collect();
    details.push(format!("matched by route(s): {}", route_names.join(", ")));

    // Evidence breakdown
    let mut evidence_kinds = Vec::new();
    if profile.has_exact {
        evidence_kinds.push("ExactMatch");
    }
    if profile.has_symbol {
        evidence_kinds.push("SymbolHit");
    }
    if profile.has_semantic {
        evidence_kinds.push("SemanticHit");
    }
    if profile.has_impact {
        evidence_kinds.push("ImpactHit");
    }
    if profile.has_context_seed {
        evidence_kinds.push("ContextSeed");
    }
    if !evidence_kinds.is_empty() {
        details.push(format!("evidence kinds: {}", evidence_kinds.join(", ")));
    }

    // Fused score
    if let Some(score) = group.score {
        details.push(format!("fused score: {score}"));
    }

    // Route policy
    details.push(format!(
        "route policy: {:?}",
        ctx.route_plan.route_policy
    ));

    details
}

fn explanation_template_id(tier: &PlannerTrustTier, route_count: usize) -> String {
    match (tier, route_count) {
        (PlannerTrustTier::High, n) if n >= 2 => {
            "planner.explain.multi_route_agreement".to_string()
        }
        (PlannerTrustTier::High, _) => "planner.explain.single_route_direct".to_string(),
        (PlannerTrustTier::Medium, n) if n >= 2 => {
            "planner.explain.multi_route_agreement".to_string()
        }
        (PlannerTrustTier::Medium, _) => "planner.explain.single_route_structural".to_string(),
        (PlannerTrustTier::Low, _) => "planner.explain.fallback_derived".to_string(),
        (PlannerTrustTier::NeedsReview, _) => "planner.explain.low_signal".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Score-gap ambiguity detection
// ---------------------------------------------------------------------------

fn score_gap_ambiguity(groups: &[PlannerResultGroup]) -> Option<PlannerAmbiguity> {
    if groups.len() < 2 {
        return None;
    }
    let (Some(first_score), Some(second_score)) = (groups[0].score, groups[1].score) else {
        return None;
    };
    if first_score == 0 || second_score == 0 {
        return None;
    }
    let gap = first_score.saturating_sub(second_score);
    if gap > 3 {
        return None;
    }
    if !are_different_anchors(&groups[0], &groups[1]) {
        return None;
    }

    let candidate_contexts = groups
        .iter()
        .take(2)
        .filter_map(|g| anchor_to_context(g.anchor.as_ref()?))
        .collect::<Vec<_>>();

    Some(PlannerAmbiguity {
        reason: PlannerAmbiguityReason::MultipleAnchorsRemain,
        details: vec![format!(
            "top 2 result groups have a score gap of {gap}; consider refining the query to disambiguate"
        )],
        candidate_contexts,
    })
}

fn are_different_anchors(a: &PlannerResultGroup, b: &PlannerResultGroup) -> bool {
    match (&a.anchor, &b.anchor) {
        (Some(aa), Some(bb)) => !anchors_eq(aa, bb),
        _ => true,
    }
}

fn anchors_eq(a: &PlannerAnchor, b: &PlannerAnchor) -> bool {
    match (a, b) {
        (
            PlannerAnchor::Symbol { symbol_id: a, .. },
            PlannerAnchor::Symbol { symbol_id: b, .. },
        ) => a == b,
        (PlannerAnchor::File { path: a }, PlannerAnchor::File { path: b }) => a == b,
        _ => false,
    }
}

fn anchor_to_context(anchor: &PlannerAnchor) -> Option<PlannerContextRef> {
    match anchor {
        PlannerAnchor::Symbol {
            symbol_id,
            path,
            span,
        } => Some(PlannerContextRef::Symbol {
            symbol_id: symbol_id.clone(),
            path: path.clone(),
            span: span.clone(),
            display_name: None,
        }),
        PlannerAnchor::File { path } => Some(PlannerContextRef::File { path: path.clone() }),
        PlannerAnchor::Span { path, span } => Some(PlannerContextRef::Span {
            path: path.clone(),
            span: span.clone(),
        }),
        PlannerAnchor::Impact { entity } => Some(PlannerContextRef::Impact {
            entity: entity.clone(),
        }),
        PlannerAnchor::Package { .. } | PlannerAnchor::Workspace { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// Diagnostic helpers consumed by planner_engine
// ---------------------------------------------------------------------------

/// Returns true when every group in a non-empty result set has Low or
/// NeedsReview trust, indicating the planner has low overall confidence.
pub fn all_groups_low_confidence(groups: &[PlannerResultGroup]) -> bool {
    !groups.is_empty()
        && groups.iter().all(|g| {
            matches!(
                g.trust.tier,
                PlannerTrustTier::Low | PlannerTrustTier::NeedsReview
            )
        })
}

/// Builds a human-readable summary of trust tier distribution across groups.
pub fn trust_tier_summary(groups: &[PlannerResultGroup]) -> String {
    let mut high = 0_u32;
    let mut medium = 0_u32;
    let mut low = 0_u32;
    let mut needs_review = 0_u32;
    for group in groups {
        match group.trust.tier {
            PlannerTrustTier::High => high += 1,
            PlannerTrustTier::Medium => medium += 1,
            PlannerTrustTier::Low => low += 1,
            PlannerTrustTier::NeedsReview => needs_review += 1,
        }
    }
    format!("High={high} Medium={medium} Low={low} NeedsReview={needs_review}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use hyperindex_protocol::planner::{
        PlannerAnchor, PlannerEvidenceItem, PlannerEvidenceKind, PlannerExplanationPayload,
        PlannerResultGroup, PlannerRouteKind, PlannerTrustPayload, PlannerTrustTier,
    };
    use hyperindex_protocol::symbols::SymbolId;

    use crate::route_policy::PlannerRoutePolicyKind;
    use crate::route_registry::PlannerRoutePlan;

    use super::*;

    // -- helpers --

    fn stub_route_plan(low_signal: bool, partial_results: bool, budget_exhausted: bool) -> PlannerRoutePlan {
        PlannerRoutePlan {
            traces: Vec::new(),
            capabilities: Vec::new(),
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            ambiguity: None,
            raw_candidate_count: 0,
            route_policy: PlannerRoutePolicyKind::MultiRouteCandidates,
            low_signal,
            partial_results,
            budget_exhausted,
            early_stopped: false,
        }
    }

    fn stub_ir() -> PlannerQueryIr {
        PlannerQueryIr {
            repo_id: "repo".to_string(),
            snapshot_id: "snap".to_string(),
            surface_query: "test".to_string(),
            normalized_query: "test".to_string(),
            selected_mode: hyperindex_protocol::planner::PlannerMode::Auto,
            primary_style: hyperindex_protocol::planner::PlannerQueryStyle::SymbolLookup,
            candidate_styles: vec![hyperindex_protocol::planner::PlannerQueryStyle::SymbolLookup],
            planned_routes: vec![PlannerRouteKind::Symbol],
            intent_signals: Vec::new(),
            limit: 10,
            selected_context: None,
            target_context: None,
            exact_query: None,
            symbol_query: None,
            semantic_query: None,
            impact_query: None,
            filters: hyperindex_protocol::planner::PlannerQueryFilters::default(),
            route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
            budgets: crate::daemon_integration::PlannerRuntimeContext::default().budget_policy,
        }
    }

    fn evidence(kind: PlannerEvidenceKind, route: PlannerRouteKind) -> PlannerEvidenceItem {
        PlannerEvidenceItem {
            evidence_kind: kind,
            route_kind: route,
            label: "test evidence".to_string(),
            path: Some("src/main.rs".to_string()),
            span: None,
            symbol_id: None,
            impact_entity: None,
            snippet: None,
            score: Some(90),
            notes: Vec::new(),
        }
    }

    fn group_with(
        id: &str,
        score: u32,
        routes: Vec<PlannerRouteKind>,
        evidence_items: Vec<PlannerEvidenceItem>,
        anchor: Option<PlannerAnchor>,
    ) -> PlannerResultGroup {
        PlannerResultGroup {
            group_id: id.to_string(),
            label: id.to_string(),
            anchor,
            routes,
            trust: PlannerTrustPayload {
                tier: PlannerTrustTier::NeedsReview,
                deterministic: false,
                evidence_count: evidence_items.len() as u32,
                route_agreement_count: 0,
                template_id: "planner.trust.structural_default".to_string(),
                reasons: vec!["pending decoration".to_string()],
                warnings: Vec::new(),
            },
            explanation: PlannerExplanationPayload {
                template_id: "planner.group.fused".to_string(),
                summary: "test group".to_string(),
                details: Vec::new(),
            },
            evidence: evidence_items,
            score: Some(score),
        }
    }

    // -- trust tier tests --

    #[test]
    fn high_trust_exact_and_symbol_corroboration() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            100,
            vec![PlannerRouteKind::Exact, PlannerRouteKind::Symbol],
            vec![
                evidence(PlannerEvidenceKind::ExactMatch, PlannerRouteKind::Exact),
                evidence(PlannerEvidenceKind::SymbolHit, PlannerRouteKind::Symbol),
            ],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::High);
        assert!(g.trust.deterministic);
        assert_eq!(g.trust.template_id, "planner.trust.corroborated");
        assert!(g.trust.reasons[0].contains("exact and symbol"));
    }

    #[test]
    fn high_trust_three_route_agreement() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            104,
            vec![
                PlannerRouteKind::Symbol,
                PlannerRouteKind::Semantic,
                PlannerRouteKind::Impact,
            ],
            vec![
                evidence(PlannerEvidenceKind::SymbolHit, PlannerRouteKind::Symbol),
                evidence(PlannerEvidenceKind::SemanticHit, PlannerRouteKind::Semantic),
                evidence(PlannerEvidenceKind::ImpactHit, PlannerRouteKind::Impact),
            ],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::High);
        // Not deterministic because no exact/context_seed in evidence
        assert!(!g.trust.deterministic);
        assert_eq!(g.trust.template_id, "planner.trust.corroborated");
        assert!(g.trust.reasons[0].contains("3 independent routes"));
    }

    #[test]
    fn high_trust_exact_match_alone() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            100,
            vec![PlannerRouteKind::Exact],
            vec![evidence(
                PlannerEvidenceKind::ExactMatch,
                PlannerRouteKind::Exact,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::High);
        assert!(g.trust.deterministic);
        assert_eq!(g.trust.template_id, "planner.trust.direct");
    }

    #[test]
    fn high_trust_context_seed() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            95,
            vec![PlannerRouteKind::Symbol],
            vec![evidence(
                PlannerEvidenceKind::ContextSeed,
                PlannerRouteKind::Symbol,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::High);
        assert!(g.trust.deterministic);
        assert_eq!(g.trust.template_id, "planner.trust.direct");
    }

    #[test]
    fn medium_trust_two_route_agreement() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            95,
            vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic],
            vec![
                evidence(PlannerEvidenceKind::SymbolHit, PlannerRouteKind::Symbol),
                evidence(PlannerEvidenceKind::SemanticHit, PlannerRouteKind::Semantic),
            ],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::Medium);
        assert!(!g.trust.deterministic);
        assert_eq!(g.trust.template_id, "planner.trust.corroborated");
    }

    #[test]
    fn medium_trust_single_symbol_good_signal() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            90,
            vec![PlannerRouteKind::Symbol],
            vec![evidence(
                PlannerEvidenceKind::SymbolHit,
                PlannerRouteKind::Symbol,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::Medium);
        assert!(!g.trust.deterministic);
        assert_eq!(g.trust.template_id, "planner.trust.direct");
    }

    #[test]
    fn low_trust_symbol_with_low_signal() {
        let plan = stub_route_plan(true, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            90,
            vec![PlannerRouteKind::Symbol],
            vec![evidence(
                PlannerEvidenceKind::SymbolHit,
                PlannerRouteKind::Symbol,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::Low);
        assert_eq!(g.trust.template_id, "planner.trust.low_signal");
    }

    #[test]
    fn low_trust_semantic_only() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            70,
            vec![PlannerRouteKind::Semantic],
            vec![evidence(
                PlannerEvidenceKind::SemanticHit,
                PlannerRouteKind::Semantic,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::Low);
        assert!(!g.trust.deterministic);
        assert_eq!(g.trust.template_id, "planner.trust.fallback_derived");
        assert!(g.trust.reasons[0].contains("semantic similarity"));
    }

    #[test]
    fn low_trust_impact_only() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            60,
            vec![PlannerRouteKind::Impact],
            vec![evidence(
                PlannerEvidenceKind::ImpactHit,
                PlannerRouteKind::Impact,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::Low);
        assert_eq!(g.trust.template_id, "planner.trust.fallback_derived");
    }

    #[test]
    fn needs_review_no_evidence() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            50,
            vec![PlannerRouteKind::Symbol],
            vec![evidence(
                PlannerEvidenceKind::FilterMatch,
                PlannerRouteKind::Symbol,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let g = &output.groups[0];
        assert_eq!(g.trust.tier, PlannerTrustTier::NeedsReview);
        assert_eq!(g.trust.template_id, "planner.trust.low_signal");
    }

    // -- warning tests --

    #[test]
    fn warning_close_score_gap() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![
            group_with(
                "g1",
                100,
                vec![PlannerRouteKind::Symbol],
                vec![evidence(
                    PlannerEvidenceKind::SymbolHit,
                    PlannerRouteKind::Symbol,
                )],
                Some(PlannerAnchor::Symbol {
                    symbol_id: SymbolId("sym.A".to_string()),
                    path: "a.rs".to_string(),
                    span: None,
                }),
            ),
            group_with(
                "g2",
                98,
                vec![PlannerRouteKind::Symbol],
                vec![evidence(
                    PlannerEvidenceKind::SymbolHit,
                    PlannerRouteKind::Symbol,
                )],
                Some(PlannerAnchor::Symbol {
                    symbol_id: SymbolId("sym.B".to_string()),
                    path: "b.rs".to_string(),
                    span: None,
                }),
            ),
        ];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert!(output.groups[0]
            .trust
            .warnings
            .iter()
            .any(|w| w.contains("score gap")));
    }

    #[test]
    fn warning_low_signal_query() {
        let plan = stub_route_plan(true, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            90,
            vec![PlannerRouteKind::Symbol],
            vec![evidence(
                PlannerEvidenceKind::SymbolHit,
                PlannerRouteKind::Symbol,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert!(output.groups[0]
            .trust
            .warnings
            .iter()
            .any(|w| w.contains("low-signal")));
    }

    #[test]
    fn warning_partial_results() {
        let plan = stub_route_plan(false, true, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            90,
            vec![PlannerRouteKind::Symbol],
            vec![evidence(
                PlannerEvidenceKind::SymbolHit,
                PlannerRouteKind::Symbol,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert!(output.groups[0]
            .trust
            .warnings
            .iter()
            .any(|w| w.contains("not consulted")));
    }

    #[test]
    fn warning_budget_exhausted() {
        let plan = stub_route_plan(false, false, true);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            90,
            vec![PlannerRouteKind::Symbol],
            vec![evidence(
                PlannerEvidenceKind::SymbolHit,
                PlannerRouteKind::Symbol,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert!(output.groups[0]
            .trust
            .warnings
            .iter()
            .any(|w| w.contains("budget")));
    }

    // -- explanation detail tests --

    #[test]
    fn explanation_details_are_evidence_backed() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            95,
            vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic],
            vec![
                evidence(PlannerEvidenceKind::SymbolHit, PlannerRouteKind::Symbol),
                evidence(PlannerEvidenceKind::SemanticHit, PlannerRouteKind::Semantic),
            ],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let details = &output.groups[0].explanation.details;
        assert!(details.iter().any(|d| d.contains("Symbol") && d.contains("Semantic")));
        assert!(details.iter().any(|d| d.contains("SymbolHit")));
        assert!(details.iter().any(|d| d.contains("fused score: 95")));
        assert!(details.iter().any(|d| d.contains("route policy")));
    }

    #[test]
    fn explanation_template_id_reflects_tier_and_route_count() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };

        // High + multi-route
        let groups = vec![group_with(
            "g1",
            104,
            vec![
                PlannerRouteKind::Exact,
                PlannerRouteKind::Symbol,
                PlannerRouteKind::Semantic,
            ],
            vec![
                evidence(PlannerEvidenceKind::ExactMatch, PlannerRouteKind::Exact),
                evidence(PlannerEvidenceKind::SymbolHit, PlannerRouteKind::Symbol),
                evidence(PlannerEvidenceKind::SemanticHit, PlannerRouteKind::Semantic),
            ],
            None,
        )];
        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert_eq!(
            output.groups[0].explanation.template_id,
            "planner.explain.multi_route_agreement"
        );

        // Medium + single route
        let groups = vec![group_with(
            "g2",
            90,
            vec![PlannerRouteKind::Symbol],
            vec![evidence(
                PlannerEvidenceKind::SymbolHit,
                PlannerRouteKind::Symbol,
            )],
            None,
        )];
        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert_eq!(
            output.groups[0].explanation.template_id,
            "planner.explain.single_route_structural"
        );

        // Low
        let groups = vec![group_with(
            "g3",
            70,
            vec![PlannerRouteKind::Semantic],
            vec![evidence(
                PlannerEvidenceKind::SemanticHit,
                PlannerRouteKind::Semantic,
            )],
            None,
        )];
        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert_eq!(
            output.groups[0].explanation.template_id,
            "planner.explain.fallback_derived"
        );
    }

    // -- score-gap ambiguity tests --

    #[test]
    fn ambiguity_detected_when_top_groups_have_close_scores() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![
            group_with(
                "g1",
                100,
                vec![PlannerRouteKind::Symbol],
                vec![evidence(
                    PlannerEvidenceKind::SymbolHit,
                    PlannerRouteKind::Symbol,
                )],
                Some(PlannerAnchor::Symbol {
                    symbol_id: SymbolId("sym.A".to_string()),
                    path: "a.rs".to_string(),
                    span: None,
                }),
            ),
            group_with(
                "g2",
                99,
                vec![PlannerRouteKind::Symbol],
                vec![evidence(
                    PlannerEvidenceKind::SymbolHit,
                    PlannerRouteKind::Symbol,
                )],
                Some(PlannerAnchor::Symbol {
                    symbol_id: SymbolId("sym.B".to_string()),
                    path: "b.rs".to_string(),
                    span: None,
                }),
            ),
        ];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert!(output.ambiguity.is_some());
        let amb = output.ambiguity.unwrap();
        assert_eq!(amb.reason, PlannerAmbiguityReason::MultipleAnchorsRemain);
        assert!(!amb.candidate_contexts.is_empty());
    }

    #[test]
    fn no_ambiguity_when_score_gap_is_large() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![
            group_with(
                "g1",
                100,
                vec![PlannerRouteKind::Symbol],
                vec![evidence(
                    PlannerEvidenceKind::SymbolHit,
                    PlannerRouteKind::Symbol,
                )],
                Some(PlannerAnchor::Symbol {
                    symbol_id: SymbolId("sym.A".to_string()),
                    path: "a.rs".to_string(),
                    span: None,
                }),
            ),
            group_with(
                "g2",
                80,
                vec![PlannerRouteKind::Semantic],
                vec![evidence(
                    PlannerEvidenceKind::SemanticHit,
                    PlannerRouteKind::Semantic,
                )],
                Some(PlannerAnchor::File {
                    path: "b.rs".to_string(),
                }),
            ),
        ];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert!(output.ambiguity.is_none());
    }

    #[test]
    fn no_ambiguity_when_single_group() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![group_with(
            "g1",
            100,
            vec![PlannerRouteKind::Symbol],
            vec![evidence(
                PlannerEvidenceKind::SymbolHit,
                PlannerRouteKind::Symbol,
            )],
            None,
        )];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert!(output.ambiguity.is_none());
    }

    // -- determinism test --

    #[test]
    fn decoration_is_deterministic() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let make_groups = || {
            vec![
                group_with(
                    "g1",
                    100,
                    vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic],
                    vec![
                        evidence(PlannerEvidenceKind::SymbolHit, PlannerRouteKind::Symbol),
                        evidence(
                            PlannerEvidenceKind::SemanticHit,
                            PlannerRouteKind::Semantic,
                        ),
                    ],
                    Some(PlannerAnchor::Symbol {
                        symbol_id: SymbolId("sym.A".to_string()),
                        path: "a.rs".to_string(),
                        span: None,
                    }),
                ),
                group_with(
                    "g2",
                    80,
                    vec![PlannerRouteKind::Semantic],
                    vec![evidence(
                        PlannerEvidenceKind::SemanticHit,
                        PlannerRouteKind::Semantic,
                    )],
                    Some(PlannerAnchor::File {
                        path: "b.rs".to_string(),
                    }),
                ),
            ]
        };

        let first = TrustPayloadFactory.decorate(make_groups(), &ctx);
        for _ in 0..10 {
            let result = TrustPayloadFactory.decorate(make_groups(), &ctx);
            assert_eq!(first.groups.len(), result.groups.len());
            for (a, b) in first.groups.iter().zip(result.groups.iter()) {
                assert_eq!(a.trust.tier, b.trust.tier);
                assert_eq!(a.trust.deterministic, b.trust.deterministic);
                assert_eq!(a.trust.template_id, b.trust.template_id);
                assert_eq!(a.trust.reasons, b.trust.reasons);
                assert_eq!(a.trust.warnings, b.trust.warnings);
                assert_eq!(a.explanation.template_id, b.explanation.template_id);
                assert_eq!(a.explanation.details, b.explanation.details);
            }
        }
    }

    // -- all_groups_low_confidence helper --

    #[test]
    fn all_groups_low_confidence_detects_weak_results() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![
            group_with(
                "g1",
                70,
                vec![PlannerRouteKind::Semantic],
                vec![evidence(
                    PlannerEvidenceKind::SemanticHit,
                    PlannerRouteKind::Semantic,
                )],
                None,
            ),
            group_with(
                "g2",
                60,
                vec![PlannerRouteKind::Impact],
                vec![evidence(
                    PlannerEvidenceKind::ImpactHit,
                    PlannerRouteKind::Impact,
                )],
                None,
            ),
        ];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert!(all_groups_low_confidence(&output.groups));
    }

    #[test]
    fn all_groups_low_confidence_false_when_high_present() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![
            group_with(
                "g1",
                100,
                vec![PlannerRouteKind::Exact],
                vec![evidence(
                    PlannerEvidenceKind::ExactMatch,
                    PlannerRouteKind::Exact,
                )],
                None,
            ),
            group_with(
                "g2",
                60,
                vec![PlannerRouteKind::Semantic],
                vec![evidence(
                    PlannerEvidenceKind::SemanticHit,
                    PlannerRouteKind::Semantic,
                )],
                None,
            ),
        ];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        assert!(!all_groups_low_confidence(&output.groups));
    }

    // -- empty input --

    #[test]
    fn empty_groups_produce_empty_output() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let output = TrustPayloadFactory.decorate(Vec::new(), &ctx);
        assert!(output.groups.is_empty());
        assert!(output.ambiguity.is_none());
    }

    // -- trust_tier_summary --

    #[test]
    fn trust_tier_summary_counts_correctly() {
        let plan = stub_route_plan(false, false, false);
        let ir = stub_ir();
        let ctx = TrustDecoratorContext {
            route_plan: &plan,
            ir: &ir,
        };
        let groups = vec![
            group_with(
                "g1",
                100,
                vec![PlannerRouteKind::Exact],
                vec![evidence(
                    PlannerEvidenceKind::ExactMatch,
                    PlannerRouteKind::Exact,
                )],
                None,
            ),
            group_with(
                "g2",
                90,
                vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic],
                vec![
                    evidence(PlannerEvidenceKind::SymbolHit, PlannerRouteKind::Symbol),
                    evidence(PlannerEvidenceKind::SemanticHit, PlannerRouteKind::Semantic),
                ],
                None,
            ),
            group_with(
                "g3",
                60,
                vec![PlannerRouteKind::Semantic],
                vec![evidence(
                    PlannerEvidenceKind::SemanticHit,
                    PlannerRouteKind::Semantic,
                )],
                None,
            ),
        ];

        let output = TrustPayloadFactory.decorate(groups, &ctx);
        let summary = trust_tier_summary(&output.groups);
        assert!(summary.contains("High=1"));
        assert!(summary.contains("Medium=1"));
        assert!(summary.contains("Low=1"));
    }
}
