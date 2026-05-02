use std::cmp::Reverse;

use hyperindex_protocol::planner::{
    PlannerContextRef, PlannerExactMatchStyle, PlannerExactQueryIntent, PlannerImpactQueryIntent,
    PlannerIntentSignal, PlannerMode, PlannerModeDecision, PlannerModeSelectionSource,
    PlannerQueryParams, PlannerQueryStyle, PlannerRouteKind, PlannerSemanticQueryIntent,
    PlannerSymbolQueryIntent,
};

use crate::common::normalize_query;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedIntent {
    pub mode: PlannerModeDecision,
    pub primary_style: PlannerQueryStyle,
    pub candidate_styles: Vec<PlannerQueryStyle>,
    pub planned_routes: Vec<PlannerRouteKind>,
    pub intent_signals: Vec<PlannerIntentSignal>,
    pub exact_query: Option<PlannerExactQueryIntent>,
    pub symbol_query: Option<PlannerSymbolQueryIntent>,
    pub semantic_query: Option<PlannerSemanticQueryIntent>,
    pub impact_query: Option<PlannerImpactQueryIntent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StyleScore {
    style: PlannerQueryStyle,
    score: u32,
    reasons: Vec<String>,
}

impl StyleScore {
    fn new(style: PlannerQueryStyle) -> Self {
        Self {
            style,
            score: 0,
            reasons: Vec::new(),
        }
    }

    fn add(&mut self, score: u32, reason: impl Into<String>) {
        self.score += score;
        self.reasons.push(reason.into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueryFeatures {
    normalized: String,
    raw_tokens: Vec<String>,
    lowered_tokens: Vec<String>,
    signals: Vec<PlannerIntentSignal>,
    exact_query: Option<PlannerExactQueryIntent>,
    symbol_query: Option<PlannerSymbolQueryIntent>,
    semantic_query: Option<PlannerSemanticQueryIntent>,
    impact_query: Option<PlannerImpactQueryIntent>,
    identifier_like: bool,
    qualified_symbol_like: bool,
    path_like: bool,
    natural_language_question: bool,
    impact_phrase: bool,
    action_term_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextBias {
    signal: PlannerIntentSignal,
    symbol_score: u32,
    semantic_score: u32,
    impact_score: u32,
    reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExactSeedPolicy {
    WhenExactLike,
    Always,
}

#[derive(Debug, Default, Clone)]
pub struct IntentRouter;

impl IntentRouter {
    pub fn classify(
        &self,
        params: &PlannerQueryParams,
        default_mode: PlannerMode,
    ) -> ClassifiedIntent {
        let features = extract_features(&params.query.text);
        let normalized_default_mode = normalize_default_mode(default_mode);

        if let Some(mode_override) = params.mode_override.clone() {
            if !matches!(mode_override, PlannerMode::Auto) {
                return classify_from_override(params, features, mode_override);
            }
        }

        let mut exact = StyleScore::new(PlannerQueryStyle::ExactLookup);
        let mut symbol = StyleScore::new(PlannerQueryStyle::SymbolLookup);
        let mut semantic = StyleScore::new(PlannerQueryStyle::SemanticLookup);
        let mut impact = StyleScore::new(PlannerQueryStyle::ImpactAnalysis);

        if let Some(exact_query) = &features.exact_query {
            let reason = match exact_query.match_style {
                PlannerExactMatchStyle::Regex => "regex-like delimiters imply exact-style lookup",
                PlannerExactMatchStyle::Literal => "quoted literals imply exact-style lookup",
                PlannerExactMatchStyle::Path => "path-like text implies exact-style lookup",
                PlannerExactMatchStyle::Glob => "glob syntax implies exact-style lookup",
            };
            exact.add(6, reason);
        }

        if features.qualified_symbol_like {
            symbol.add(6, "qualified symbol syntax implies symbol-style lookup");
        } else if features.identifier_like {
            symbol.add(4, "identifier-like text implies symbol-style lookup");
        }
        if contains_symbol_keyword(&features.lowered_tokens) {
            symbol.add(2, "symbol-oriented wording strengthens symbol-style lookup");
        }
        if features.path_like {
            symbol.add(1, "path-like text can still seed symbol lookup");
        }

        if features.natural_language_question {
            semantic.add(
                4,
                "natural-language question wording implies semantic-style lookup",
            );
        }
        if features.lowered_tokens.len() >= 3 {
            semantic.add(2, "multi-token freeform text keeps semantic lookup viable");
        }
        if !features.identifier_like && !features.qualified_symbol_like && !features.path_like {
            semantic.add(
                1,
                "no dominant identifier or path signal keeps semantic lookup as a fallback",
            );
        }

        if features.impact_phrase {
            impact.add(6, "blast-radius wording implies impact-style lookup");
        }
        if features.action_term_count > 0 {
            impact.add(6, "action-oriented wording implies impact-style lookup");
        }
        if features.natural_language_question && features.action_term_count > 0 {
            impact.add(
                1,
                "question-form action queries can require impact analysis plus lookup fallbacks",
            );
        }

        if let Some(context_bias) = selected_context_bias(params.selected_context.as_ref()) {
            symbol.add(context_bias.symbol_score, context_bias.reason);
            semantic.add(context_bias.semantic_score, context_bias.reason);
            impact.add(context_bias.impact_score, context_bias.reason);
        }

        if params.target_context.is_some() {
            impact.add(
                1,
                "explicit target context biases routing toward impact analysis",
            );
        }
        if !filters_are_empty(params) {
            symbol.add(
                1,
                "explicit filters narrow the lookup scope deterministically",
            );
        }

        let mut ranked_styles = vec![exact, symbol, semantic, impact];
        ranked_styles.sort_by_key(|entry| (Reverse(entry.score), style_priority(&entry.style)));

        let primary_score = ranked_styles
            .first()
            .map(|entry| entry.score)
            .unwrap_or_default();
        let primary_style = if primary_score == 0 {
            mode_to_style(&normalized_default_mode)
        } else {
            ranked_styles[0].style.clone()
        };

        let mut candidate_styles = if primary_score == 0 {
            vec![primary_style.clone()]
        } else {
            ranked_styles
                .iter()
                .filter(|entry| entry.score > 0 && entry.score + 1 >= primary_score)
                .map(|entry| entry.style.clone())
                .collect::<Vec<_>>()
        };
        if candidate_styles.is_empty() {
            candidate_styles.push(primary_style.clone());
        }

        let reason_source = ranked_styles
            .iter()
            .find(|entry| entry.style == primary_style)
            .map(|entry| entry.reasons.clone())
            .unwrap_or_else(|| {
                vec!["query fell back to the configured planner default mode".to_string()]
            });

        let mut reasons = reason_source;
        if candidate_styles.len() > 1 {
            reasons.push(format!(
                "mixed route signals kept {}",
                candidate_styles
                    .iter()
                    .map(query_style_label)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        let selected_mode = style_to_mode(&primary_style);
        let planned_routes = planned_routes_for_styles(
            &candidate_styles,
            features.exact_query.is_some(),
            &params.route_hints.preferred_routes,
            &params.route_hints.disabled_routes,
            if params.route_hints.require_exact_seed {
                ExactSeedPolicy::Always
            } else {
                ExactSeedPolicy::WhenExactLike
            },
        );

        let mut intent_signals = features.signals;
        if let Some(context_bias) = selected_context_bias(params.selected_context.as_ref()) {
            push_signal(&mut intent_signals, context_bias.signal);
        }
        if params.target_context.is_some() {
            push_signal(
                &mut intent_signals,
                PlannerIntentSignal::TargetContextProvided,
            );
        }
        if !filters_are_empty(params) {
            push_signal(
                &mut intent_signals,
                PlannerIntentSignal::FilterScopeProvided,
            );
        }

        ClassifiedIntent {
            mode: PlannerModeDecision {
                requested_mode: params.mode_override.clone(),
                selected_mode,
                source: PlannerModeSelectionSource::Heuristic,
                reasons,
            },
            primary_style,
            candidate_styles,
            planned_routes,
            intent_signals,
            exact_query: features.exact_query,
            symbol_query: features.symbol_query,
            semantic_query: features.semantic_query,
            impact_query: features.impact_query,
        }
    }
}

fn classify_from_override(
    params: &PlannerQueryParams,
    mut features: QueryFeatures,
    mode_override: PlannerMode,
) -> ClassifiedIntent {
    let primary_style = mode_to_style(&mode_override);
    let planned_routes = planned_routes_for_override(&mode_override, &params.route_hints);

    push_signal(
        &mut features.signals,
        PlannerIntentSignal::ExplicitModeOverride,
    );
    if let Some(context_bias) = selected_context_bias(params.selected_context.as_ref()) {
        push_signal(&mut features.signals, context_bias.signal);
    }
    if params.target_context.is_some() {
        push_signal(
            &mut features.signals,
            PlannerIntentSignal::TargetContextProvided,
        );
    }
    if !filters_are_empty(params) {
        push_signal(
            &mut features.signals,
            PlannerIntentSignal::FilterScopeProvided,
        );
    }

    ensure_primary_shape(&mut features, &primary_style);

    ClassifiedIntent {
        mode: PlannerModeDecision {
            requested_mode: Some(mode_override.clone()),
            selected_mode: mode_override,
            source: PlannerModeSelectionSource::ExplicitOverride,
            reasons: vec!["planner mode selected from explicit override".to_string()],
        },
        primary_style: primary_style.clone(),
        candidate_styles: vec![primary_style],
        planned_routes,
        intent_signals: features.signals,
        exact_query: features.exact_query,
        symbol_query: features.symbol_query,
        semantic_query: features.semantic_query,
        impact_query: features.impact_query,
    }
}

fn extract_features(raw_query: &str) -> QueryFeatures {
    let normalized = normalize_query(raw_query);
    let raw_tokens = unique_preserve_order(
        normalized
            .split_whitespace()
            .map(clean_token)
            .filter(|token| !token.is_empty()),
    );
    let lowered = normalized.to_ascii_lowercase();
    let lowered_tokens =
        unique_preserve_order(raw_tokens.iter().map(|token| token.to_ascii_lowercase()));

    let regex_like = is_regex_like(&normalized);
    let quoted_literal = is_quoted_literal(&normalized);
    let glob_like = is_glob_like(&normalized);
    let exact_path_seed = if regex_like || quoted_literal || glob_like {
        None
    } else if raw_tokens.len() == 1 && is_path_like(&normalized) {
        Some(normalized.clone())
    } else {
        raw_tokens.iter().find(|token| is_path_like(token)).cloned()
    };
    let path_like = exact_path_seed.is_some();
    let qualified_symbol_seed = raw_tokens
        .iter()
        .find(|token| is_qualified_symbol_like(token))
        .cloned();
    let identifier_seed = if raw_tokens.len() == 1 && is_identifier_segment(&normalized) {
        Some(normalized.clone())
    } else {
        raw_tokens
            .iter()
            .find(|token| is_identifier_signal_token(token))
            .cloned()
    };
    let qualified_symbol_like = qualified_symbol_seed.is_some();
    let identifier_like = identifier_seed.is_some();
    let natural_language_question = is_natural_language_question(&lowered_tokens, &lowered);
    let impact_phrase = contains_impact_phrase(&lowered);
    let action_terms = extract_action_terms(&raw_tokens);
    let subject_terms = extract_subject_terms(&raw_tokens);

    let exact_query = build_exact_query(
        &normalized,
        exact_path_seed.as_deref(),
        regex_like,
        quoted_literal,
        glob_like,
    );
    let symbol_query = build_symbol_query(
        qualified_symbol_seed
            .as_deref()
            .or(identifier_seed.as_deref()),
    );
    let semantic_query =
        build_semantic_query(&normalized, &lowered_tokens, natural_language_question);
    let impact_query =
        build_impact_query(&normalized, &action_terms, &subject_terms, impact_phrase);

    let mut signals = Vec::new();
    if regex_like {
        push_signal(&mut signals, PlannerIntentSignal::RegexLike);
    }
    if quoted_literal {
        push_signal(&mut signals, PlannerIntentSignal::QuotedLiteral);
    }
    if path_like {
        push_signal(&mut signals, PlannerIntentSignal::PathLike);
    }
    if glob_like {
        push_signal(&mut signals, PlannerIntentSignal::GlobLike);
    }
    if identifier_like {
        push_signal(&mut signals, PlannerIntentSignal::IdentifierLike);
    }
    if qualified_symbol_like {
        push_signal(&mut signals, PlannerIntentSignal::QualifiedSymbolLike);
    }
    if natural_language_question {
        push_signal(&mut signals, PlannerIntentSignal::NaturalLanguageQuestion);
    }
    if lowered_tokens.len() >= 3 {
        push_signal(&mut signals, PlannerIntentSignal::MultiTokenQuery);
    }
    if impact_phrase || !action_terms.is_empty() {
        push_signal(&mut signals, PlannerIntentSignal::ImpactPhrase);
    }

    QueryFeatures {
        normalized,
        raw_tokens,
        lowered_tokens,
        signals,
        exact_query,
        symbol_query,
        semantic_query,
        impact_query,
        identifier_like,
        qualified_symbol_like,
        path_like,
        natural_language_question,
        impact_phrase,
        action_term_count: action_terms.len(),
    }
}

fn build_exact_query(
    normalized: &str,
    path_seed: Option<&str>,
    regex_like: bool,
    quoted_literal: bool,
    glob_like: bool,
) -> Option<PlannerExactQueryIntent> {
    if regex_like {
        return Some(PlannerExactQueryIntent {
            normalized_term: normalized
                .trim_start_matches('/')
                .trim_end_matches('/')
                .to_string(),
            match_style: PlannerExactMatchStyle::Regex,
        });
    }
    if quoted_literal {
        return Some(PlannerExactQueryIntent {
            normalized_term: normalized.trim_matches('"').trim_matches('\'').to_string(),
            match_style: PlannerExactMatchStyle::Literal,
        });
    }
    if glob_like {
        return Some(PlannerExactQueryIntent {
            normalized_term: normalized.to_string(),
            match_style: PlannerExactMatchStyle::Glob,
        });
    }
    if let Some(path_seed) = path_seed {
        return Some(PlannerExactQueryIntent {
            normalized_term: path_seed.to_string(),
            match_style: PlannerExactMatchStyle::Path,
        });
    }
    None
}

fn build_symbol_query(symbol_seed: Option<&str>) -> Option<PlannerSymbolQueryIntent> {
    let symbol_seed = symbol_seed?;
    let segments = split_symbol_segments(symbol_seed);
    Some(PlannerSymbolQueryIntent {
        normalized_symbol: symbol_seed.to_string(),
        segments: if segments.is_empty() {
            vec![symbol_seed.to_string()]
        } else {
            segments
        },
    })
}

fn build_semantic_query(
    normalized: &str,
    lowered_tokens: &[String],
    natural_language_question: bool,
) -> Option<PlannerSemanticQueryIntent> {
    if !natural_language_question && lowered_tokens.len() < 3 {
        return None;
    }

    Some(PlannerSemanticQueryIntent {
        normalized_text: normalized.to_string(),
        tokens: lowered_tokens.to_vec(),
    })
}

fn build_impact_query(
    normalized: &str,
    action_terms: &[String],
    subject_terms: &[String],
    impact_phrase: bool,
) -> Option<PlannerImpactQueryIntent> {
    if !impact_phrase && action_terms.is_empty() {
        return None;
    }

    Some(PlannerImpactQueryIntent {
        normalized_text: normalized.to_string(),
        action_terms: action_terms.to_vec(),
        subject_terms: subject_terms.to_vec(),
    })
}

fn ensure_primary_shape(features: &mut QueryFeatures, primary_style: &PlannerQueryStyle) {
    match primary_style {
        PlannerQueryStyle::ExactLookup => {
            if features.exact_query.is_none() {
                features.exact_query = Some(PlannerExactQueryIntent {
                    normalized_term: features.normalized.clone(),
                    match_style: PlannerExactMatchStyle::Literal,
                });
            }
        }
        PlannerQueryStyle::SymbolLookup => {
            if features.symbol_query.is_none() {
                features.symbol_query = Some(PlannerSymbolQueryIntent {
                    normalized_symbol: features.normalized.clone(),
                    segments: vec![features.normalized.clone()],
                });
            }
        }
        PlannerQueryStyle::SemanticLookup => {
            if features.semantic_query.is_none() {
                features.semantic_query = Some(PlannerSemanticQueryIntent {
                    normalized_text: features.normalized.clone(),
                    tokens: features.lowered_tokens.clone(),
                });
            }
        }
        PlannerQueryStyle::ImpactAnalysis => {
            if features.impact_query.is_none() {
                features.impact_query = Some(PlannerImpactQueryIntent {
                    normalized_text: features.normalized.clone(),
                    action_terms: extract_action_terms(&features.raw_tokens),
                    subject_terms: extract_subject_terms(&features.raw_tokens),
                });
            }
        }
    }
}

fn planned_routes_for_styles(
    styles: &[PlannerQueryStyle],
    has_exact_shape: bool,
    preferred_routes: &[PlannerRouteKind],
    disabled_routes: &[PlannerRouteKind],
    exact_seed_policy: ExactSeedPolicy,
) -> Vec<PlannerRouteKind> {
    let mut routes = Vec::new();
    let include_exact = match exact_seed_policy {
        ExactSeedPolicy::WhenExactLike => has_exact_shape,
        ExactSeedPolicy::Always => true,
    };

    for style in styles {
        let candidates = match style {
            PlannerQueryStyle::ExactLookup => {
                vec![
                    PlannerRouteKind::Exact,
                    PlannerRouteKind::Symbol,
                    PlannerRouteKind::Semantic,
                ]
            }
            PlannerQueryStyle::SymbolLookup => {
                let mut route_kinds = vec![PlannerRouteKind::Symbol];
                if include_exact {
                    route_kinds.push(PlannerRouteKind::Exact);
                }
                route_kinds.push(PlannerRouteKind::Semantic);
                route_kinds
            }
            PlannerQueryStyle::SemanticLookup => {
                let mut route_kinds = vec![PlannerRouteKind::Semantic, PlannerRouteKind::Symbol];
                if include_exact {
                    route_kinds.push(PlannerRouteKind::Exact);
                }
                route_kinds
            }
            PlannerQueryStyle::ImpactAnalysis => {
                vec![
                    PlannerRouteKind::Symbol,
                    PlannerRouteKind::Semantic,
                    PlannerRouteKind::Impact,
                ]
            }
        };
        for route_kind in candidates {
            push_route(&mut routes, route_kind);
        }
    }

    if include_exact && !styles.contains(&PlannerQueryStyle::ExactLookup) {
        if !routes.contains(&PlannerRouteKind::Exact) {
            push_route(&mut routes, PlannerRouteKind::Exact);
        }
    }

    let primary_route = routes.first().cloned();
    let mut reordered = Vec::new();
    if let Some(primary_route) = primary_route {
        reordered.push(primary_route);
    }
    for route_kind in preferred_routes {
        if Some(route_kind) != reordered.first() {
            push_route(&mut reordered, route_kind.clone());
        }
    }
    for route_kind in routes {
        push_route(&mut reordered, route_kind);
    }

    reordered
        .into_iter()
        .filter(|route_kind| !disabled_routes.contains(route_kind))
        .collect()
}

fn planned_routes_for_override(
    mode_override: &PlannerMode,
    route_hints: &hyperindex_protocol::planner::PlannerRouteHints,
) -> Vec<PlannerRouteKind> {
    let route_kind = match mode_override {
        PlannerMode::Auto => return Vec::new(),
        PlannerMode::Exact => PlannerRouteKind::Exact,
        PlannerMode::Symbol => PlannerRouteKind::Symbol,
        PlannerMode::Semantic => PlannerRouteKind::Semantic,
        PlannerMode::Impact => PlannerRouteKind::Impact,
    };

    (!route_hints.disabled_routes.contains(&route_kind))
        .then_some(route_kind)
        .into_iter()
        .collect()
}

fn filters_are_empty(params: &PlannerQueryParams) -> bool {
    params.filters.path_globs.is_empty()
        && params.filters.package_names.is_empty()
        && params.filters.package_roots.is_empty()
        && params.filters.workspace_roots.is_empty()
        && params.filters.languages.is_empty()
        && params.filters.extensions.is_empty()
        && params.filters.symbol_kinds.is_empty()
}

fn selected_context_bias(context: Option<&PlannerContextRef>) -> Option<ContextBias> {
    match context {
        Some(PlannerContextRef::Symbol { .. }) => Some(ContextBias {
            signal: PlannerIntentSignal::SelectedSymbolContext,
            symbol_score: 2,
            semantic_score: 0,
            impact_score: 1,
            reason: "selected symbol context biases routing toward grounded symbol or impact plans",
        }),
        Some(PlannerContextRef::File { .. }) => Some(ContextBias {
            signal: PlannerIntentSignal::SelectedFileContext,
            symbol_score: 1,
            semantic_score: 1,
            impact_score: 1,
            reason: "selected file context narrows the planner search space",
        }),
        Some(PlannerContextRef::Span { .. }) => Some(ContextBias {
            signal: PlannerIntentSignal::SelectedSpanContext,
            symbol_score: 1,
            semantic_score: 1,
            impact_score: 0,
            reason: "selected span context narrows the planner search space",
        }),
        Some(PlannerContextRef::Package { .. }) => Some(ContextBias {
            signal: PlannerIntentSignal::SelectedPackageContext,
            symbol_score: 0,
            semantic_score: 1,
            impact_score: 0,
            reason: "selected package context keeps semantic lookup scoped",
        }),
        Some(PlannerContextRef::Workspace { .. }) => Some(ContextBias {
            signal: PlannerIntentSignal::SelectedWorkspaceContext,
            symbol_score: 0,
            semantic_score: 1,
            impact_score: 0,
            reason: "selected workspace context keeps semantic lookup scoped",
        }),
        Some(PlannerContextRef::Impact { .. }) => Some(ContextBias {
            signal: PlannerIntentSignal::SelectedImpactContext,
            symbol_score: 0,
            semantic_score: 0,
            impact_score: 3,
            reason: "selected impact context strongly biases routing toward impact analysis",
        }),
        None => None,
    }
}

fn normalize_default_mode(default_mode: PlannerMode) -> PlannerMode {
    match default_mode {
        PlannerMode::Auto => PlannerMode::Semantic,
        mode => mode,
    }
}

fn mode_to_style(mode: &PlannerMode) -> PlannerQueryStyle {
    match mode {
        PlannerMode::Auto => PlannerQueryStyle::SemanticLookup,
        PlannerMode::Exact => PlannerQueryStyle::ExactLookup,
        PlannerMode::Symbol => PlannerQueryStyle::SymbolLookup,
        PlannerMode::Semantic => PlannerQueryStyle::SemanticLookup,
        PlannerMode::Impact => PlannerQueryStyle::ImpactAnalysis,
    }
}

fn style_to_mode(style: &PlannerQueryStyle) -> PlannerMode {
    match style {
        PlannerQueryStyle::ExactLookup => PlannerMode::Exact,
        PlannerQueryStyle::SymbolLookup => PlannerMode::Symbol,
        PlannerQueryStyle::SemanticLookup => PlannerMode::Semantic,
        PlannerQueryStyle::ImpactAnalysis => PlannerMode::Impact,
    }
}

fn style_priority(style: &PlannerQueryStyle) -> u8 {
    match style {
        PlannerQueryStyle::ImpactAnalysis => 0,
        PlannerQueryStyle::ExactLookup => 1,
        PlannerQueryStyle::SymbolLookup => 2,
        PlannerQueryStyle::SemanticLookup => 3,
    }
}

fn query_style_label(style: &PlannerQueryStyle) -> &'static str {
    match style {
        PlannerQueryStyle::ExactLookup => "exact_lookup",
        PlannerQueryStyle::SymbolLookup => "symbol_lookup",
        PlannerQueryStyle::SemanticLookup => "semantic_lookup",
        PlannerQueryStyle::ImpactAnalysis => "impact_analysis",
    }
}

fn contains_symbol_keyword(tokens: &[String]) -> bool {
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "definition"
                | "definitions"
                | "reference"
                | "references"
                | "caller"
                | "callers"
                | "method"
                | "class"
                | "symbol"
                | "member"
                | "namespace"
        )
    })
}

fn contains_impact_phrase(lowered: &str) -> bool {
    [
        "what breaks",
        "blast radius",
        "who depends on",
        "what depends on",
        "if i rename",
        "if i delete",
        "if i change",
        "affected by",
        "impact of",
    ]
    .iter()
    .any(|phrase| lowered.contains(phrase))
}

fn is_regex_like(normalized: &str) -> bool {
    normalized.len() > 2
        && normalized.starts_with('/')
        && normalized.ends_with('/')
        && normalized[1..normalized.len() - 1].chars().any(|ch| {
            matches!(
                ch,
                '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
            )
        })
}

fn is_quoted_literal(normalized: &str) -> bool {
    (normalized.starts_with('"') && normalized.ends_with('"') && normalized.len() > 1)
        || (normalized.starts_with('\'') && normalized.ends_with('\'') && normalized.len() > 1)
}

fn is_glob_like(normalized: &str) -> bool {
    !normalized.contains(' ')
        && !is_regex_like(normalized)
        && (normalized.contains('*') || normalized.contains('?'))
}

fn is_path_like(normalized: &str) -> bool {
    normalized.contains('/') || normalized.contains('\\') || has_known_extension(normalized)
}

fn has_known_extension(normalized: &str) -> bool {
    if normalized.contains(' ') {
        return false;
    }
    let Some((base, extension)) = normalized.rsplit_once('.') else {
        return false;
    };
    !base.is_empty()
        && matches!(
            extension.to_ascii_lowercase().as_str(),
            "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "json" | "toml" | "md" | "rs"
        )
}

fn is_qualified_symbol_like(normalized: &str) -> bool {
    if normalized.contains(' ') {
        return false;
    }
    if normalized.contains("::") || normalized.contains('#') {
        return split_symbol_segments(normalized)
            .into_iter()
            .all(|segment| is_identifier_segment(&segment));
    }
    if normalized.contains('.') && !has_known_extension(normalized) {
        let segments = split_symbol_segments(normalized);
        return segments.len() > 1
            && segments
                .into_iter()
                .all(|segment| is_identifier_segment(&segment));
    }
    false
}

fn split_symbol_segments(normalized: &str) -> Vec<String> {
    normalized
        .replace("::", ".")
        .replace('#', ".")
        .split('.')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn is_identifier_segment(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || matches!(first, '_' | '$')) {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$'))
}

fn is_identifier_signal_token(value: &str) -> bool {
    is_identifier_segment(value)
        && (value.chars().skip(1).any(|ch| ch.is_ascii_uppercase())
            || value.starts_with('_')
            || value.contains('_')
            || value.contains('$')
            || value
                .chars()
                .next()
                .map(|ch| ch.is_ascii_uppercase())
                .unwrap_or(false))
}

fn is_natural_language_question(tokens: &[String], lowered: &str) -> bool {
    let starts_with_question = tokens
        .first()
        .map(|token| {
            matches!(
                token.as_str(),
                "where" | "how" | "what" | "which" | "who" | "why" | "when"
            )
        })
        .unwrap_or(false);

    starts_with_question
        || lowered.contains("which file")
        || lowered.contains("implementation")
        || lowered.contains("what breaks")
}

fn extract_action_terms(tokens: &[String]) -> Vec<String> {
    unique_preserve_order(tokens.iter().filter_map(|token| {
        let lowered = token.to_ascii_lowercase();
        matches!(
            lowered.as_str(),
            "invalidate"
                | "rename"
                | "delete"
                | "remove"
                | "change"
                | "changes"
                | "modify"
                | "break"
                | "breaks"
                | "affect"
                | "affects"
        )
        .then_some(token.clone())
    }))
}

fn extract_subject_terms(tokens: &[String]) -> Vec<String> {
    unique_preserve_order(tokens.iter().filter_map(|token| {
        let lowered = token.to_ascii_lowercase();
        (!matches!(
            lowered.as_str(),
            "a" | "an"
                | "the"
                | "and"
                | "or"
                | "to"
                | "of"
                | "in"
                | "on"
                | "for"
                | "with"
                | "if"
                | "i"
                | "we"
                | "do"
                | "does"
                | "what"
                | "where"
                | "how"
                | "which"
                | "who"
                | "why"
                | "when"
                | "this"
                | "that"
                | "it"
                | "break"
                | "breaks"
                | "invalidate"
                | "rename"
                | "delete"
                | "remove"
                | "change"
                | "changes"
                | "modify"
                | "affect"
                | "affects"
        ))
        .then_some(token.clone())
    }))
}

fn clean_token(token: &str) -> String {
    token
        .trim_matches(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$')))
        .to_string()
}

fn push_signal(signals: &mut Vec<PlannerIntentSignal>, signal: PlannerIntentSignal) {
    if !signals.contains(&signal) {
        signals.push(signal);
    }
}

fn push_route(routes: &mut Vec<PlannerRouteKind>, route_kind: PlannerRouteKind) {
    if !routes.contains(&route_kind) {
        routes.push(route_kind);
    }
}

fn unique_preserve_order<I>(items: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut values = Vec::new();
    for item in items {
        if !values.contains(&item) {
            values.push(item);
        }
    }
    values
}
