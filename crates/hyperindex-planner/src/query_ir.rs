use hyperindex_protocol::planner::{
    PlannerBudgetPolicy, PlannerQueryFilters, PlannerQueryIr, PlannerQueryParams, PlannerRouteHints,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;

use crate::common::normalize_query;
use crate::daemon_integration::PlannerRuntimeContext;
use crate::intent_router::ClassifiedIntent;
use crate::planner_model::{PlannerError, PlannerResult};

#[derive(Debug, Default, Clone)]
pub struct QueryIrBuilder;

impl QueryIrBuilder {
    pub fn build(
        &self,
        context: &PlannerRuntimeContext,
        params: &PlannerQueryParams,
        snapshot: &ComposedSnapshot,
        classified: &ClassifiedIntent,
    ) -> PlannerResult<PlannerQueryIr> {
        let normalized_query = normalize_query(&params.query.text);
        if normalized_query.is_empty() {
            return Err(PlannerError::InvalidQuery(
                "planner query text must not be empty".to_string(),
            ));
        }

        let budgets = merge_budget_policy(&context.budget_policy, params.budgets.as_ref());
        let limit = params.limit.max(1).min(context.max_limit);
        let filters = normalize_filters(&params.filters);
        let route_hints = normalize_route_hints(&params.route_hints);

        Ok(PlannerQueryIr {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            surface_query: params.query.text.clone(),
            normalized_query,
            selected_mode: classified.mode.selected_mode.clone(),
            primary_style: classified.primary_style.clone(),
            candidate_styles: classified.candidate_styles.clone(),
            planned_routes: classified.planned_routes.clone(),
            intent_signals: classified.intent_signals.clone(),
            limit,
            selected_context: params.selected_context.clone(),
            target_context: params.target_context.clone(),
            exact_query: classified.exact_query.clone(),
            symbol_query: classified.symbol_query.clone(),
            semantic_query: classified.semantic_query.clone(),
            impact_query: classified.impact_query.clone(),
            filters,
            route_hints,
            budgets,
        })
    }
}

fn merge_budget_policy(
    defaults: &PlannerBudgetPolicy,
    overrides: Option<&hyperindex_protocol::planner::PlannerBudgetHints>,
) -> PlannerBudgetPolicy {
    let Some(overrides) = overrides else {
        return defaults.clone();
    };

    let mut policy = defaults.clone();
    if let Some(total_timeout_ms) = overrides.total_timeout_ms {
        policy.total_timeout_ms = total_timeout_ms;
    }
    if let Some(max_groups) = overrides.max_groups {
        policy.max_groups = max_groups;
    }
    if !overrides.route_budgets.is_empty() {
        policy.route_budgets = overrides.route_budgets.clone();
    }
    policy
}

fn normalize_filters(filters: &PlannerQueryFilters) -> PlannerQueryFilters {
    PlannerQueryFilters {
        path_globs: normalize_string_list(&filters.path_globs, false, false),
        package_names: normalize_string_list(&filters.package_names, false, false),
        package_roots: normalize_string_list(&filters.package_roots, false, false),
        workspace_roots: normalize_string_list(&filters.workspace_roots, false, false),
        languages: normalize_enum_list(&filters.languages, language_key),
        extensions: normalize_string_list(&filters.extensions, true, true),
        symbol_kinds: normalize_enum_list(&filters.symbol_kinds, symbol_kind_key),
    }
}

fn normalize_route_hints(route_hints: &PlannerRouteHints) -> PlannerRouteHints {
    PlannerRouteHints {
        preferred_routes: dedupe_preserve_order(&route_hints.preferred_routes),
        disabled_routes: dedupe_preserve_order(&route_hints.disabled_routes),
        require_exact_seed: route_hints.require_exact_seed,
    }
}

fn normalize_string_list(
    values: &[String],
    lowercase: bool,
    strip_dot_prefix: bool,
) -> Vec<String> {
    let mut normalized = values
        .iter()
        .map(|value| normalize_query(value))
        .filter(|value| !value.is_empty())
        .map(|mut value| {
            if strip_dot_prefix {
                value = value.trim_start_matches('.').to_string();
            }
            if lowercase {
                value = value.to_ascii_lowercase();
            }
            value
        })
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_enum_list<T: Clone>(values: &[T], key: fn(&T) -> &'static str) -> Vec<T> {
    let mut normalized = values.to_vec();
    normalized.sort_by_key(key);
    normalized.dedup_by(|left, right| key(left) == key(right));
    normalized
}

fn language_key(value: &hyperindex_protocol::symbols::LanguageId) -> &'static str {
    use hyperindex_protocol::symbols::LanguageId;

    match value {
        LanguageId::Typescript => "typescript",
        LanguageId::Tsx => "tsx",
        LanguageId::Javascript => "javascript",
        LanguageId::Jsx => "jsx",
        LanguageId::Mts => "mts",
        LanguageId::Cts => "cts",
    }
}

fn symbol_kind_key(value: &hyperindex_protocol::symbols::SymbolKind) -> &'static str {
    use hyperindex_protocol::symbols::SymbolKind;

    match value {
        SymbolKind::File => "file",
        SymbolKind::Module => "module",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "interface",
        SymbolKind::TypeAlias => "type_alias",
        SymbolKind::Enum => "enum",
        SymbolKind::EnumMember => "enum_member",
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Constructor => "constructor",
        SymbolKind::Property => "property",
        SymbolKind::Field => "field",
        SymbolKind::Variable => "variable",
        SymbolKind::Constant => "constant",
        SymbolKind::Parameter => "parameter",
        SymbolKind::ImportBinding => "import_binding",
    }
}

fn dedupe_preserve_order<T: Clone + PartialEq>(values: &[T]) -> Vec<T> {
    let mut normalized = Vec::new();
    for value in values {
        if !normalized.contains(value) {
            normalized.push(value.clone());
        }
    }
    normalized
}
