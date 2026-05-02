use std::sync::Arc;

use hyperindex_protocol::planner::{
    PlannerAmbiguity, PlannerAmbiguityReason, PlannerAnchor, PlannerContextRef, PlannerDiagnostic,
    PlannerQueryIr, PlannerRouteKind, PlannerRouteSkipReason, PlannerRouteStatus,
    PlannerRouteTrace,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;

use crate::common::{budget_for_route, scaffold_info, scaffold_warning};
use crate::daemon_integration::PlannerRuntimeContext;
use crate::exact_route::UnavailableExactRouteProvider;
use crate::route_adapters::{
    NormalizedPlannerCandidate, PlannerRouteAdapter, PlannerRouteCapabilityReport,
    PlannerRouteExecutionState, PlannerRouteReadiness, PlannerRouteRequest,
    full_filter_capabilities,
};
use crate::route_policy::{PlannerRoutePolicyKind, plan_route_policy};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerRoutePlan {
    pub traces: Vec<PlannerRouteTrace>,
    pub capabilities: Vec<PlannerRouteCapabilityReport>,
    pub candidates: Vec<NormalizedPlannerCandidate>,
    pub diagnostics: Vec<PlannerDiagnostic>,
    pub ambiguity: Option<PlannerAmbiguity>,
    pub raw_candidate_count: u32,
    pub route_policy: PlannerRoutePolicyKind,
    pub low_signal: bool,
    pub partial_results: bool,
    pub budget_exhausted: bool,
    pub early_stopped: bool,
}

impl PlannerRoutePlan {
    pub fn routes_considered(&self) -> u32 {
        self.traces.len() as u32
    }

    pub fn routes_available(&self) -> u32 {
        self.traces.iter().filter(|trace| trace.available).count() as u32
    }

    pub fn selected_routes_available(&self) -> u32 {
        self.traces
            .iter()
            .filter(|trace| trace.selected && trace.available)
            .count() as u32
    }

    pub fn has_deferred_routes(&self) -> bool {
        self.traces
            .iter()
            .any(|trace| trace.status == PlannerRouteStatus::Deferred && trace.selected)
    }
}

#[derive(Debug, Clone)]
pub struct PlannerRouteRegistry {
    adapters: Vec<Arc<dyn PlannerRouteAdapter>>,
}

impl Default for PlannerRouteRegistry {
    fn default() -> Self {
        Self::new(vec![
            Arc::new(UnavailableExactRouteProvider),
            Arc::new(CapabilityOnlyRouteAdapter::new(PlannerRouteKind::Symbol)),
            Arc::new(CapabilityOnlyRouteAdapter::new(PlannerRouteKind::Semantic)),
            Arc::new(CapabilityOnlyRouteAdapter::new(PlannerRouteKind::Impact)),
        ])
    }
}

impl PlannerRouteRegistry {
    pub fn new(adapters: Vec<Arc<dyn PlannerRouteAdapter>>) -> Self {
        Self { adapters }
    }

    pub fn capabilities(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
    ) -> Vec<PlannerRouteCapabilityReport> {
        all_routes()
            .into_iter()
            .map(|route_kind| self.capability_for(context, snapshot, route_kind))
            .collect()
    }

    pub fn plan(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
        ir: &PlannerQueryIr,
    ) -> PlannerRoutePlan {
        let capabilities = self.capabilities(context, snapshot);
        let route_policy = plan_route_policy(ir);
        let budget_selection = select_budgeted_routes(ir, &route_policy.selected_routes);
        let mut traces = initialize_traces(ir, &capabilities, &route_policy, &budget_selection);
        let mut diagnostics = capabilities
            .iter()
            .flat_map(|report| report.diagnostics.clone())
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        let mut ambiguity = None;
        let mut raw_candidate_count = 0_u32;
        if route_policy.low_signal {
            diagnostics.push(scaffold_warning(
                "planner_low_signal_query",
                "planner query is low-signal without a deterministic exact, symbol, or file seed",
            ));
        }
        if budget_selection.budget_exhausted {
            diagnostics.push(scaffold_warning(
                "planner_budget_exhausted",
                "planner route selection hit the configured total timeout budget before every selected route could be executed",
            ));
        }

        let early_stopped = match route_policy.kind {
            PlannerRoutePolicyKind::SingleRoute
            | PlannerRoutePolicyKind::StagedFallback
            | PlannerRoutePolicyKind::MultiRouteCandidates => self.execute_standard_policy(
                context,
                snapshot,
                ir,
                &capabilities,
                &budget_selection.executable_routes,
                route_policy.kind.clone(),
                &mut traces,
                &mut diagnostics,
                &mut candidates,
                &mut ambiguity,
                &mut raw_candidate_count,
            ),
            PlannerRoutePolicyKind::SeedThenImpact => self.execute_seed_then_impact_policy(
                context,
                snapshot,
                ir,
                &capabilities,
                &budget_selection.executable_routes,
                &mut traces,
                &mut diagnostics,
                &mut candidates,
                &mut ambiguity,
                &mut raw_candidate_count,
            ),
        };

        let partial_results = candidates_exist_with_missing_selected_routes(&traces, &candidates);
        if partial_results {
            diagnostics.push(scaffold_warning(
                "planner_partial_results",
                format!(
                    "planner returned partial route-level results after skipping {} selected route(s)",
                    traces
                        .iter()
                        .filter(|trace| trace.selected && trace.status != PlannerRouteStatus::Executed)
                        .count()
                ),
            ));
        }

        PlannerRoutePlan {
            traces,
            capabilities,
            candidates,
            diagnostics,
            ambiguity,
            raw_candidate_count,
            route_policy: route_policy.kind,
            low_signal: route_policy.low_signal,
            partial_results,
            budget_exhausted: budget_selection.budget_exhausted,
            early_stopped,
        }
    }

    fn capability_for(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
        route_kind: PlannerRouteKind,
    ) -> PlannerRouteCapabilityReport {
        self.adapter_for(&route_kind)
            .map(|adapter| adapter.capability(context, snapshot))
            .unwrap_or_else(|| missing_route_capability(context, route_kind))
    }

    fn adapter_for(&self, route_kind: &PlannerRouteKind) -> Option<&Arc<dyn PlannerRouteAdapter>> {
        self.adapters
            .iter()
            .find(|adapter| adapter.kind() == *route_kind)
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_standard_policy(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
        ir: &PlannerQueryIr,
        capabilities: &[PlannerRouteCapabilityReport],
        executable_routes: &[PlannerRouteKind],
        policy_kind: PlannerRoutePolicyKind,
        traces: &mut [PlannerRouteTrace],
        diagnostics: &mut Vec<PlannerDiagnostic>,
        candidates: &mut Vec<NormalizedPlannerCandidate>,
        ambiguity: &mut Option<PlannerAmbiguity>,
        raw_candidate_count: &mut u32,
    ) -> bool {
        for (index, route_kind) in executable_routes.iter().enumerate() {
            let capability = capability_for_route(capabilities, route_kind);
            let budget = budget_for_route(&ir.budgets, route_kind.clone());
            let attempt = self.attempt_route(context, snapshot, ir, capability, budget);

            *raw_candidate_count += attempt.raw_candidate_count;
            diagnostics.extend(attempt.diagnostics.clone());
            if ambiguity.is_none() {
                *ambiguity = attempt.ambiguity.clone();
            }
            candidates.extend(attempt.candidates.clone());
            apply_attempt_to_trace(traces, route_kind, capability, attempt.clone());

            let should_stop = match policy_kind {
                PlannerRoutePolicyKind::SingleRoute => true,
                PlannerRoutePolicyKind::StagedFallback => {
                    !attempt.candidates.is_empty() || attempt.ambiguity.is_some()
                }
                PlannerRoutePolicyKind::MultiRouteCandidates => false,
                PlannerRoutePolicyKind::SeedThenImpact => false,
            };

            if should_stop {
                let skipped_message = match policy_kind {
                    PlannerRoutePolicyKind::SingleRoute => {
                        "single-route policy bypassed lower-priority routes".to_string()
                    }
                    PlannerRoutePolicyKind::StagedFallback => {
                        format!("{route_kind:?} satisfied the staged fallback policy")
                    }
                    PlannerRoutePolicyKind::MultiRouteCandidates
                    | PlannerRoutePolicyKind::SeedThenImpact => String::new(),
                };
                mark_remaining_routes_skipped(
                    traces,
                    &executable_routes[index + 1..],
                    skipped_message,
                );
                return index + 1 < executable_routes.len();
            }
        }

        false
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_seed_then_impact_policy(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
        ir: &PlannerQueryIr,
        capabilities: &[PlannerRouteCapabilityReport],
        executable_routes: &[PlannerRouteKind],
        traces: &mut [PlannerRouteTrace],
        diagnostics: &mut Vec<PlannerDiagnostic>,
        candidates: &mut Vec<NormalizedPlannerCandidate>,
        ambiguity: &mut Option<PlannerAmbiguity>,
        raw_candidate_count: &mut u32,
    ) -> bool {
        let seed_routes = executable_routes
            .iter()
            .filter(|route_kind| **route_kind != PlannerRouteKind::Impact)
            .cloned()
            .collect::<Vec<_>>();
        let impact_selected = executable_routes.contains(&PlannerRouteKind::Impact);
        let mut derived_context = ir
            .selected_context
            .clone()
            .or_else(|| ir.target_context.clone());

        for (index, route_kind) in seed_routes.iter().enumerate() {
            let capability = capability_for_route(capabilities, route_kind);
            let budget = budget_for_route(&ir.budgets, route_kind.clone());
            let attempt = self.attempt_route(context, snapshot, ir, capability, budget);

            *raw_candidate_count += attempt.raw_candidate_count;
            diagnostics.extend(attempt.diagnostics.clone());
            candidates.extend(attempt.candidates.clone());
            apply_attempt_to_trace(traces, route_kind, capability, attempt.clone());

            if let Some(route_ambiguity) = attempt.ambiguity.clone() {
                if ambiguity.is_none() {
                    *ambiguity = Some(route_ambiguity);
                }
                mark_remaining_routes_skipped(
                    traces,
                    &seed_routes[index + 1..],
                    "impact seed resolution stopped after an ambiguous seed route".to_string(),
                );
                if impact_selected {
                    mark_remaining_routes_skipped(
                        traces,
                        &[PlannerRouteKind::Impact],
                        "impact route withheld because seed resolution remained ambiguous"
                            .to_string(),
                    );
                }
                return true;
            }

            match seed_context_from_candidates(&attempt.candidates) {
                SeedResolution::Unique(context_ref) => {
                    derived_context = Some(context_ref);
                    mark_remaining_routes_skipped(
                        traces,
                        &seed_routes[index + 1..],
                        format!("{route_kind:?} resolved a deterministic impact seed"),
                    );
                    break;
                }
                SeedResolution::Ambiguous(candidate_contexts) => {
                    if ambiguity.is_none() {
                        *ambiguity = Some(PlannerAmbiguity {
                            reason: PlannerAmbiguityReason::MultipleAnchorsRemain,
                            details: vec![format!(
                                "{route_kind:?} produced multiple symbol or file anchors, so impact analysis stopped before guessing"
                            )],
                            candidate_contexts,
                        });
                    }
                    mark_remaining_routes_skipped(
                        traces,
                        &seed_routes[index + 1..],
                        "impact seed resolution stopped after multiple anchors remained"
                            .to_string(),
                    );
                    if impact_selected {
                        mark_remaining_routes_skipped(
                            traces,
                            &[PlannerRouteKind::Impact],
                            "impact route withheld because seed resolution remained ambiguous"
                                .to_string(),
                        );
                    }
                    return true;
                }
                SeedResolution::None => {}
            }
        }

        if !impact_selected {
            return derived_context.is_some();
        }

        let Some(seed_context) = derived_context else {
            diagnostics.push(scaffold_warning(
                "planner_impact_seed_missing",
                "impact route was not executed because no deterministic symbol or file context was resolved from the selected seed routes",
            ));
            mark_remaining_routes_skipped(
                traces,
                &[PlannerRouteKind::Impact],
                "impact route withheld until a symbol or file context is selected".to_string(),
            );
            return false;
        };

        let capability = capability_for_route(capabilities, &PlannerRouteKind::Impact);
        let budget = budget_for_route(&ir.budgets, PlannerRouteKind::Impact);
        let mut derived_ir = ir.clone();
        if derived_ir.selected_context.is_none() && derived_ir.target_context.is_none() {
            derived_ir.selected_context = Some(seed_context);
        }
        let attempt = self.attempt_route(context, snapshot, &derived_ir, capability, budget);
        *raw_candidate_count += attempt.raw_candidate_count;
        diagnostics.extend(attempt.diagnostics.clone());
        if ambiguity.is_none() {
            *ambiguity = attempt.ambiguity.clone();
        }
        candidates.extend(attempt.candidates.clone());
        apply_attempt_to_trace(traces, &PlannerRouteKind::Impact, capability, attempt);

        true
    }

    fn attempt_route(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
        ir: &PlannerQueryIr,
        capability: &PlannerRouteCapabilityReport,
        budget: hyperindex_protocol::planner::PlannerRouteBudget,
    ) -> RouteAttempt {
        if !capability.available {
            return RouteAttempt {
                status: PlannerRouteStatus::Skipped,
                skip_reason: Some(
                    if matches!(capability.route_kind, PlannerRouteKind::Exact) {
                        PlannerRouteSkipReason::ExactEngineUnavailable
                    } else {
                        PlannerRouteSkipReason::CapabilityDisabled
                    },
                ),
                candidates: Vec::new(),
                diagnostics: Vec::new(),
                notes: capability.notes.clone(),
                elapsed_ms: None,
                raw_candidate_count: 0,
                ambiguity: None,
            };
        }

        let unsupported_filters = capability.unsupported_filters(&ir.filters);
        if !unsupported_filters.is_empty() {
            let filters = unsupported_filters.join(", ");
            return RouteAttempt {
                status: PlannerRouteStatus::Skipped,
                skip_reason: Some(PlannerRouteSkipReason::CapabilityDisabled),
                candidates: Vec::new(),
                diagnostics: vec![scaffold_warning(
                    format!("{}_unsupported_filters", route_code(&capability.route_kind)),
                    format!(
                        "{:?} route skipped because the current adapter does not support requested filters: {filters}",
                        capability.route_kind
                    ),
                )],
                notes: vec![format!("unsupported requested filters: {filters}")],
                elapsed_ms: None,
                raw_candidate_count: 0,
                ambiguity: None,
            };
        }

        let request = PlannerRouteRequest {
            runtime: context,
            snapshot,
            ir,
            budget,
        };
        let execution = self
            .adapter_for(&capability.route_kind)
            .map(|adapter| adapter.execute(&request))
            .unwrap_or_else(|| missing_route_execution(capability.route_kind.clone()));

        let raw_candidate_count = execution.candidates.len() as u32;
        let filtered = execution
            .candidates
            .into_iter()
            .filter(|candidate| candidate.matches_filters(&ir.filters, snapshot))
            .collect::<Vec<_>>();
        let filtered_out = raw_candidate_count.saturating_sub(filtered.len() as u32);
        let mut notes = capability.notes.clone();
        notes.extend(execution.notes.clone());
        if filtered_out > 0 {
            notes.push(format!(
                "{filtered_out} candidate(s) filtered after normalization"
            ));
        }

        let (status, skip_reason, elapsed_ms) = match execution.state {
            PlannerRouteExecutionState::Deferred => (
                PlannerRouteStatus::Deferred,
                Some(PlannerRouteSkipReason::ExecutionDeferred),
                Some(execution.elapsed_ms),
            ),
            PlannerRouteExecutionState::Executed => (
                PlannerRouteStatus::Executed,
                None,
                Some(execution.elapsed_ms),
            ),
        };

        RouteAttempt {
            status,
            skip_reason,
            candidates: filtered,
            diagnostics: execution.diagnostics,
            notes,
            elapsed_ms,
            raw_candidate_count,
            ambiguity: execution.ambiguity,
        }
    }
}

#[derive(Debug, Clone)]
struct RouteAttempt {
    status: PlannerRouteStatus,
    skip_reason: Option<PlannerRouteSkipReason>,
    candidates: Vec<NormalizedPlannerCandidate>,
    diagnostics: Vec<PlannerDiagnostic>,
    notes: Vec<String>,
    elapsed_ms: Option<u64>,
    raw_candidate_count: u32,
    ambiguity: Option<PlannerAmbiguity>,
}

#[derive(Debug, Clone, Default)]
struct BudgetSelection {
    executable_routes: Vec<PlannerRouteKind>,
    exhausted_routes: Vec<PlannerRouteKind>,
    zero_budget_routes: Vec<PlannerRouteKind>,
    budget_exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SeedResolution {
    None,
    Unique(PlannerContextRef),
    Ambiguous(Vec<PlannerContextRef>),
}

fn initialize_traces(
    ir: &PlannerQueryIr,
    capabilities: &[PlannerRouteCapabilityReport],
    route_policy: &crate::route_policy::PlannerRoutePolicy,
    budget_selection: &BudgetSelection,
) -> Vec<PlannerRouteTrace> {
    all_routes()
        .into_iter()
        .map(|route_kind| {
            let capability = capability_for_route(capabilities, &route_kind);
            let budget = budget_for_route(&ir.budgets, route_kind.clone());
            let selected = route_policy.selected_routes.contains(&route_kind);
            let disabled_by_hint = ir.route_hints.disabled_routes.contains(&route_kind);

            if disabled_by_hint {
                return PlannerRouteTrace {
                    route_kind,
                    available: false,
                    selected: false,
                    status: PlannerRouteStatus::Skipped,
                    skip_reason: Some(PlannerRouteSkipReason::FilteredByRouteHint),
                    budget: Some(budget),
                    candidate_count: None,
                    group_count: None,
                    elapsed_ms: None,
                    notes: vec!["route disabled by planner route hints".to_string()],
                };
            }

            let mut trace = PlannerRouteTrace {
                route_kind: route_kind.clone(),
                available: capability.available,
                selected,
                status: if selected {
                    PlannerRouteStatus::Planned
                } else {
                    PlannerRouteStatus::Skipped
                },
                skip_reason: if selected {
                    None
                } else {
                    Some(PlannerRouteSkipReason::FilteredByMode)
                },
                budget: Some(budget),
                candidate_count: None,
                group_count: None,
                elapsed_ms: None,
                notes: if selected {
                    route_policy.notes.clone()
                } else {
                    vec![format!(
                        "route omitted by {:?} planner policy",
                        route_policy.kind
                    )]
                },
            };

            if budget_selection.zero_budget_routes.contains(&route_kind) {
                trace.status = PlannerRouteStatus::Skipped;
                trace.skip_reason = Some(PlannerRouteSkipReason::ExecutionDeferred);
                trace.notes = vec![
                    "route budget disables execution because timeout or max_candidates resolved to zero"
                        .to_string(),
                ];
            } else if budget_selection.exhausted_routes.contains(&route_kind) {
                trace.status = PlannerRouteStatus::Skipped;
                trace.skip_reason = Some(PlannerRouteSkipReason::ExecutionDeferred);
                trace.notes = vec![
                    "route omitted because the planner total timeout budget was exhausted first"
                        .to_string(),
                ];
            }

            trace
        })
        .collect()
}

fn capability_for_route<'a>(
    capabilities: &'a [PlannerRouteCapabilityReport],
    route_kind: &PlannerRouteKind,
) -> &'a PlannerRouteCapabilityReport {
    capabilities
        .iter()
        .find(|capability| capability.route_kind == *route_kind)
        .expect("every public route must have a capability report")
}

fn select_budgeted_routes(
    ir: &PlannerQueryIr,
    selected_routes: &[PlannerRouteKind],
) -> BudgetSelection {
    let mut selection = BudgetSelection::default();
    let mut reserved_timeout_ms = 0_u64;
    let mut deferred_first_route = None;

    for route_kind in selected_routes {
        let budget = budget_for_route(&ir.budgets, route_kind.clone());
        if budget.timeout_ms == 0 || budget.max_candidates == 0 {
            selection.zero_budget_routes.push(route_kind.clone());
            selection.budget_exhausted = true;
            continue;
        }

        if !selection.executable_routes.is_empty()
            && reserved_timeout_ms.saturating_add(budget.timeout_ms) > ir.budgets.total_timeout_ms
        {
            selection.exhausted_routes.push(route_kind.clone());
            selection.budget_exhausted = true;
            continue;
        }

        if selection.executable_routes.is_empty() && budget.timeout_ms > ir.budgets.total_timeout_ms
        {
            deferred_first_route = Some(route_kind.clone());
            selection.budget_exhausted = true;
            continue;
        }

        reserved_timeout_ms = reserved_timeout_ms.saturating_add(budget.timeout_ms);
        selection.executable_routes.push(route_kind.clone());
    }

    if selection.executable_routes.is_empty() {
        if let Some(route_kind) = deferred_first_route {
            selection.executable_routes.push(route_kind);
        }
    }

    selection
}

fn apply_attempt_to_trace(
    traces: &mut [PlannerRouteTrace],
    route_kind: &PlannerRouteKind,
    capability: &PlannerRouteCapabilityReport,
    attempt: RouteAttempt,
) {
    let trace = trace_mut(traces, route_kind);
    trace.available = capability.available;
    trace.status = attempt.status;
    trace.skip_reason = attempt.skip_reason;
    trace.candidate_count =
        (trace.status == PlannerRouteStatus::Executed).then_some(attempt.candidates.len() as u32);
    trace.elapsed_ms = attempt.elapsed_ms;
    trace.notes = attempt.notes;
}

fn trace_mut<'a>(
    traces: &'a mut [PlannerRouteTrace],
    route_kind: &PlannerRouteKind,
) -> &'a mut PlannerRouteTrace {
    traces
        .iter_mut()
        .find(|trace| trace.route_kind == *route_kind)
        .expect("every public route must have a trace slot")
}

fn mark_remaining_routes_skipped(
    traces: &mut [PlannerRouteTrace],
    remaining_routes: &[PlannerRouteKind],
    message: String,
) {
    for route_kind in remaining_routes {
        let trace = trace_mut(traces, route_kind);
        if trace.status == PlannerRouteStatus::Planned {
            trace.status = PlannerRouteStatus::Skipped;
            trace.skip_reason = Some(PlannerRouteSkipReason::ExecutionDeferred);
            trace.notes = vec![message.clone()];
        }
    }
}

fn seed_context_from_candidates(candidates: &[NormalizedPlannerCandidate]) -> SeedResolution {
    let mut contexts = Vec::new();
    for candidate in candidates {
        let Some(context) = candidate_context(candidate) else {
            continue;
        };
        if !contexts.contains(&context) {
            contexts.push(context);
        }
    }

    match contexts.len() {
        0 => SeedResolution::None,
        1 => SeedResolution::Unique(contexts.remove(0)),
        _ => SeedResolution::Ambiguous(contexts),
    }
}

fn candidate_context(candidate: &NormalizedPlannerCandidate) -> Option<PlannerContextRef> {
    match &candidate.anchor {
        PlannerAnchor::Symbol {
            symbol_id,
            path,
            span,
        } => Some(PlannerContextRef::Symbol {
            symbol_id: symbol_id.clone(),
            path: path.clone(),
            span: span.clone(),
            display_name: Some(candidate.label.clone()),
        }),
        PlannerAnchor::Span { path, span } => Some(PlannerContextRef::Span {
            path: path.clone(),
            span: span.clone(),
        }),
        PlannerAnchor::File { path } => Some(PlannerContextRef::File { path: path.clone() }),
        PlannerAnchor::Impact { entity } => Some(PlannerContextRef::Impact {
            entity: entity.clone(),
        }),
        PlannerAnchor::Package { .. } | PlannerAnchor::Workspace { .. } => None,
    }
}

fn candidates_exist_with_missing_selected_routes(
    traces: &[PlannerRouteTrace],
    candidates: &[NormalizedPlannerCandidate],
) -> bool {
    !candidates.is_empty()
        && traces.iter().any(|trace| {
            trace.selected
                && matches!(
                    trace.status,
                    PlannerRouteStatus::Skipped | PlannerRouteStatus::Deferred
                )
                && !trace.notes.iter().any(|note| {
                    note.contains("single-route policy bypassed")
                        || note.contains("satisfied the staged fallback policy")
                        || note.contains("resolved a deterministic impact seed")
                })
        })
}

fn all_routes() -> Vec<PlannerRouteKind> {
    vec![
        PlannerRouteKind::Exact,
        PlannerRouteKind::Symbol,
        PlannerRouteKind::Semantic,
        PlannerRouteKind::Impact,
    ]
}

fn route_code(route_kind: &PlannerRouteKind) -> &'static str {
    match route_kind {
        PlannerRouteKind::Exact => "exact_route",
        PlannerRouteKind::Symbol => "symbol_route",
        PlannerRouteKind::Semantic => "semantic_route",
        PlannerRouteKind::Impact => "impact_route",
    }
}

fn missing_route_capability(
    context: &PlannerRuntimeContext,
    route_kind: PlannerRouteKind,
) -> PlannerRouteCapabilityReport {
    let enabled = context.route_enabled(route_kind.clone());
    PlannerRouteCapabilityReport {
        route_kind,
        enabled,
        available: false,
        readiness: if enabled {
            PlannerRouteReadiness::Unavailable
        } else {
            PlannerRouteReadiness::Disabled
        },
        reason: Some("route adapter is not registered".to_string()),
        supported_filters: full_filter_capabilities(),
        constraints: crate::route_adapters::PlannerRouteConstraints::default(),
        diagnostics: vec![scaffold_warning(
            "planner_route_adapter_missing",
            "route adapter is not registered in the planner registry",
        )],
        notes: vec!["route adapter is not registered".to_string()],
    }
}

fn missing_route_execution(
    route_kind: PlannerRouteKind,
) -> crate::route_adapters::PlannerRouteExecution {
    crate::route_adapters::PlannerRouteExecution {
        state: PlannerRouteExecutionState::Executed,
        candidates: Vec::new(),
        diagnostics: vec![scaffold_warning(
            format!("{}_adapter_missing", route_code(&route_kind)),
            "route execution skipped because no adapter is registered",
        )],
        notes: vec!["route adapter is not registered".to_string()],
        elapsed_ms: 0,
        ambiguity: None,
    }
}

#[derive(Debug, Clone)]
struct CapabilityOnlyRouteAdapter {
    route_kind: PlannerRouteKind,
}

impl CapabilityOnlyRouteAdapter {
    fn new(route_kind: PlannerRouteKind) -> Self {
        Self { route_kind }
    }
}

impl PlannerRouteAdapter for CapabilityOnlyRouteAdapter {
    fn kind(&self) -> PlannerRouteKind {
        self.route_kind.clone()
    }

    fn capability(
        &self,
        context: &PlannerRuntimeContext,
        _snapshot: &ComposedSnapshot,
    ) -> PlannerRouteCapabilityReport {
        let enabled = context.route_enabled(self.route_kind.clone());
        let available = context.route_available(self.route_kind.clone());
        PlannerRouteCapabilityReport {
            route_kind: self.route_kind.clone(),
            enabled,
            available,
            readiness: if !enabled {
                PlannerRouteReadiness::Disabled
            } else if available {
                PlannerRouteReadiness::Ready
            } else {
                PlannerRouteReadiness::Unavailable
            },
            reason: if available {
                None
            } else {
                Some("route disabled by runtime context".to_string())
            },
            supported_filters: full_filter_capabilities(),
            constraints: crate::route_adapters::PlannerRouteConstraints {
                emits_engine_local_scores: true,
                returns_file_provenance: true,
                returns_symbol_provenance: true,
                returns_span_provenance: true,
                planner_applies_filters_post_retrieval: true,
                ..crate::route_adapters::PlannerRouteConstraints::default()
            },
            diagnostics: vec![scaffold_info(
                format!("{}_capability_only", route_code(&self.route_kind)),
                "route capability is present but no live executor is configured in this workspace",
            )],
            notes: vec!["live route execution remains deferred".to_string()],
        }
    }

    fn execute(
        &self,
        _request: &PlannerRouteRequest<'_>,
    ) -> crate::route_adapters::PlannerRouteExecution {
        crate::route_adapters::PlannerRouteExecution {
            state: PlannerRouteExecutionState::Deferred,
            candidates: Vec::new(),
            diagnostics: vec![scaffold_info(
                format!("{}_execution_deferred", route_code(&self.route_kind)),
                "route execution remains deferred in this planner workspace",
            )],
            notes: vec!["route execution remains deferred".to_string()],
            elapsed_ms: 0,
            ambiguity: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hyperindex_protocol::planner::{
        PlannerAnchor, PlannerEvidenceItem, PlannerEvidenceKind, PlannerImpactQueryIntent,
        PlannerMode, PlannerQueryFilters, PlannerQueryIr, PlannerQueryStyle, PlannerRouteHints,
        PlannerRouteKind, PlannerRouteSkipReason, PlannerRouteStatus, PlannerSymbolQueryIntent,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::{ByteRange, LinePosition, SourceSpan, SymbolId, SymbolKind};
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use crate::daemon_integration::PlannerRuntimeContext;
    use crate::route_adapters::{
        NormalizedPlannerCandidate, PlannerRouteAdapter, PlannerRouteCapabilityReport,
        PlannerRouteConstraints, PlannerRouteExecution, PlannerRouteExecutionState,
        PlannerRouteReadiness, PlannerRouteRequest, full_filter_capabilities,
    };

    use super::PlannerRouteRegistry;

    #[derive(Debug)]
    struct FakeAdapter {
        route_kind: PlannerRouteKind,
        capability: PlannerRouteCapabilityReport,
        execution: PlannerRouteExecution,
    }

    impl PlannerRouteAdapter for FakeAdapter {
        fn kind(&self) -> PlannerRouteKind {
            self.route_kind.clone()
        }

        fn capability(
            &self,
            _context: &PlannerRuntimeContext,
            _snapshot: &ComposedSnapshot,
        ) -> PlannerRouteCapabilityReport {
            self.capability.clone()
        }

        fn execute(&self, _request: &PlannerRouteRequest<'_>) -> PlannerRouteExecution {
            self.execution.clone()
        }
    }

    fn snapshot() -> ComposedSnapshot {
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
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    fn span() -> SourceSpan {
        SourceSpan {
            start: LinePosition { line: 1, column: 0 },
            end: LinePosition {
                line: 1,
                column: 12,
            },
            bytes: ByteRange { start: 0, end: 12 },
        }
    }

    fn ir() -> PlannerQueryIr {
        PlannerQueryIr {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            surface_query: "invalidateSession".to_string(),
            normalized_query: "invalidateSession".to_string(),
            selected_mode: PlannerMode::Auto,
            primary_style: PlannerQueryStyle::SymbolLookup,
            candidate_styles: vec![
                PlannerQueryStyle::SymbolLookup,
                PlannerQueryStyle::SemanticLookup,
            ],
            planned_routes: vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic],
            intent_signals: Vec::new(),
            limit: 10,
            selected_context: None,
            target_context: None,
            exact_query: None,
            symbol_query: None,
            semantic_query: None,
            impact_query: None,
            filters: PlannerQueryFilters::default(),
            route_hints: PlannerRouteHints::default(),
            budgets: PlannerRuntimeContext::default().budget_policy,
        }
    }

    fn exact_ir() -> PlannerQueryIr {
        let mut query_ir = ir();
        query_ir.selected_mode = PlannerMode::Exact;
        query_ir.primary_style = PlannerQueryStyle::ExactLookup;
        query_ir.candidate_styles = vec![PlannerQueryStyle::ExactLookup];
        query_ir.planned_routes = vec![
            PlannerRouteKind::Exact,
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Semantic,
        ];
        query_ir
    }

    fn impact_ir() -> PlannerQueryIr {
        let mut query_ir = ir();
        query_ir.selected_mode = PlannerMode::Impact;
        query_ir.primary_style = PlannerQueryStyle::ImpactAnalysis;
        query_ir.candidate_styles = vec![PlannerQueryStyle::ImpactAnalysis];
        query_ir.planned_routes = vec![
            PlannerRouteKind::Symbol,
            PlannerRouteKind::Semantic,
            PlannerRouteKind::Impact,
        ];
        query_ir.symbol_query = Some(PlannerSymbolQueryIntent {
            normalized_symbol: "SessionStore".to_string(),
            segments: vec!["SessionStore".to_string()],
        });
        query_ir.impact_query = Some(PlannerImpactQueryIntent {
            normalized_text: "what breaks if I rename SessionStore".to_string(),
            action_terms: vec!["rename".to_string()],
            subject_terms: vec!["SessionStore".to_string()],
        });
        query_ir
    }

    fn candidate(route_kind: PlannerRouteKind, path: &str) -> NormalizedPlannerCandidate {
        NormalizedPlannerCandidate {
            candidate_id: format!("{route_kind:?}:{path}"),
            route_kind: route_kind.clone(),
            engine_type: route_kind.clone(),
            label: "invalidateSession".to_string(),
            anchor: PlannerAnchor::Symbol {
                symbol_id: SymbolId("sym.invalidateSession".to_string()),
                path: path.to_string(),
                span: Some(span()),
            },
            rank: Some(1),
            engine_score: Some(99),
            normalized_score: None,
            primary_path: Some(path.to_string()),
            primary_symbol_id: Some(SymbolId("sym.invalidateSession".to_string())),
            primary_span: Some(span()),
            language: Some(hyperindex_protocol::symbols::LanguageId::Typescript),
            extension: Some("ts".to_string()),
            symbol_kind: Some(SymbolKind::Function),
            package_name: None,
            package_root: None,
            workspace_root: Some("/tmp/repo".to_string()),
            evidence: vec![PlannerEvidenceItem {
                evidence_kind: match route_kind {
                    PlannerRouteKind::Semantic => PlannerEvidenceKind::SemanticHit,
                    _ => PlannerEvidenceKind::SymbolHit,
                },
                route_kind,
                label: "matched".to_string(),
                path: Some(path.to_string()),
                span: Some(span()),
                symbol_id: Some(SymbolId("sym.invalidateSession".to_string())),
                impact_entity: None,
                snippet: None,
                score: Some(99),
                notes: Vec::new(),
            }],
            engine_diagnostics: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn registry_executes_multiple_route_adapters_without_special_casing() {
        let registry = PlannerRouteRegistry::new(vec![
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Symbol,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Symbol,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(
                        PlannerRouteKind::Symbol,
                        "packages/auth/src/session/service.ts",
                    )],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 5,
                    ambiguity: None,
                },
            }),
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Semantic,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Semantic,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(PlannerRouteKind::Semantic, "src/session.ts")],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 7,
                    ambiguity: None,
                },
            }),
        ]);

        let plan = registry.plan(&PlannerRuntimeContext::default(), &snapshot(), &ir());

        assert_eq!(plan.candidates.len(), 2);
        assert_eq!(
            plan.route_policy,
            crate::route_policy::PlannerRoutePolicyKind::MultiRouteCandidates
        );
        assert_eq!(
            plan.candidates
                .iter()
                .map(|candidate| candidate.route_kind.clone())
                .collect::<Vec<_>>(),
            vec![PlannerRouteKind::Symbol, PlannerRouteKind::Semantic]
        );
        assert_eq!(
            plan.traces
                .iter()
                .filter(|trace| trace.status == PlannerRouteStatus::Executed)
                .count(),
            2
        );
    }

    #[test]
    fn registry_reports_unsupported_filters_and_unavailable_routes_explicitly() {
        let registry = PlannerRouteRegistry::new(vec![Arc::new(FakeAdapter {
            route_kind: PlannerRouteKind::Symbol,
            capability: PlannerRouteCapabilityReport {
                route_kind: PlannerRouteKind::Symbol,
                enabled: true,
                available: true,
                readiness: PlannerRouteReadiness::Ready,
                reason: None,
                supported_filters: hyperindex_protocol::planner::PlannerFilterCapabilities {
                    path_globs: true,
                    package_names: false,
                    package_roots: false,
                    workspace_roots: true,
                    languages: true,
                    extensions: true,
                    symbol_kinds: true,
                },
                constraints: PlannerRouteConstraints::default(),
                diagnostics: Vec::new(),
                notes: Vec::new(),
            },
            execution: PlannerRouteExecution {
                state: PlannerRouteExecutionState::Executed,
                candidates: vec![candidate(
                    PlannerRouteKind::Symbol,
                    "packages/auth/src/session/service.ts",
                )],
                diagnostics: Vec::new(),
                notes: Vec::new(),
                elapsed_ms: 5,
                ambiguity: None,
            },
        })]);
        let mut ir = ir();
        ir.filters.package_names = vec!["@hyperindex/auth".to_string()];

        let plan = registry.plan(&PlannerRuntimeContext::default(), &snapshot(), &ir);

        assert!(plan.candidates.is_empty());
        assert!(
            plan.diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "symbol_route_unsupported_filters" })
        );
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Symbol
                && trace.status == PlannerRouteStatus::Skipped
        }));
    }

    #[test]
    fn registry_stops_after_symbol_success_in_staged_fallback_plan() {
        let mut query_ir = ir();
        query_ir.candidate_styles = vec![PlannerQueryStyle::SymbolLookup];
        let registry = PlannerRouteRegistry::new(vec![
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Symbol,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Symbol,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(
                        PlannerRouteKind::Symbol,
                        "packages/auth/src/session/service.ts",
                    )],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 5,
                    ambiguity: None,
                },
            }),
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Semantic,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Semantic,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(PlannerRouteKind::Semantic, "src/session.ts")],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 7,
                    ambiguity: None,
                },
            }),
        ]);

        let plan = registry.plan(&PlannerRuntimeContext::default(), &snapshot(), &query_ir);

        assert_eq!(
            plan.route_policy,
            crate::route_policy::PlannerRoutePolicyKind::StagedFallback
        );
        assert!(plan.early_stopped);
        assert_eq!(plan.candidates.len(), 1);
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Symbol
                && trace.status == PlannerRouteStatus::Executed
        }));
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Semantic
                && trace.status == PlannerRouteStatus::Skipped
                && trace
                    .notes
                    .iter()
                    .any(|note| note.contains("staged fallback policy"))
        }));
    }

    #[test]
    fn registry_falls_back_when_primary_exact_route_is_unavailable() {
        let registry = PlannerRouteRegistry::new(vec![
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Exact,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Exact,
                    enabled: true,
                    available: false,
                    readiness: PlannerRouteReadiness::Unavailable,
                    reason: Some("exact engine does not ship".to_string()),
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: vec!["exact engine does not ship".to_string()],
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: Vec::new(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 0,
                    ambiguity: None,
                },
            }),
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Symbol,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Symbol,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(
                        PlannerRouteKind::Symbol,
                        "packages/auth/src/session/service.ts",
                    )],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 5,
                    ambiguity: None,
                },
            }),
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Semantic,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Semantic,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(PlannerRouteKind::Semantic, "src/session.ts")],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 7,
                    ambiguity: None,
                },
            }),
        ]);

        let plan = registry.plan(&PlannerRuntimeContext::default(), &snapshot(), &exact_ir());

        assert_eq!(plan.candidates.len(), 1);
        assert!(plan.partial_results);
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Exact
                && trace.skip_reason == Some(PlannerRouteSkipReason::ExactEngineUnavailable)
        }));
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Symbol
                && trace.status == PlannerRouteStatus::Executed
        }));
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Semantic
                && trace.status == PlannerRouteStatus::Skipped
        }));
    }

    #[test]
    fn registry_seeds_impact_from_symbol_result_before_running_impact() {
        let registry = PlannerRouteRegistry::new(vec![
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Symbol,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Symbol,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(
                        PlannerRouteKind::Symbol,
                        "packages/auth/src/session/service.ts",
                    )],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 5,
                    ambiguity: None,
                },
            }),
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Semantic,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Semantic,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(PlannerRouteKind::Semantic, "src/session.ts")],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 7,
                    ambiguity: None,
                },
            }),
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Impact,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Impact,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![NormalizedPlannerCandidate {
                        candidate_id: "impact:sym.invalidateSession".to_string(),
                        route_kind: PlannerRouteKind::Impact,
                        engine_type: PlannerRouteKind::Impact,
                        label: "invalidateSession".to_string(),
                        anchor: PlannerAnchor::File {
                            path: "packages/auth/src/session/service.ts".to_string(),
                        },
                        rank: Some(1),
                        engine_score: Some(95),
                        normalized_score: None,
                        primary_path: Some("packages/auth/src/session/service.ts".to_string()),
                        primary_symbol_id: None,
                        primary_span: None,
                        language: Some(hyperindex_protocol::symbols::LanguageId::Typescript),
                        extension: Some("ts".to_string()),
                        symbol_kind: None,
                        package_name: None,
                        package_root: None,
                        workspace_root: Some("/tmp/repo".to_string()),
                        evidence: Vec::new(),
                        engine_diagnostics: Vec::new(),
                        notes: Vec::new(),
                    }],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 9,
                    ambiguity: None,
                },
            }),
        ]);

        let plan = registry.plan(&PlannerRuntimeContext::default(), &snapshot(), &impact_ir());

        assert_eq!(
            plan.route_policy,
            crate::route_policy::PlannerRoutePolicyKind::SeedThenImpact
        );
        assert_eq!(
            plan.candidates
                .iter()
                .map(|candidate| candidate.route_kind.clone())
                .collect::<Vec<_>>(),
            vec![PlannerRouteKind::Symbol, PlannerRouteKind::Impact]
        );
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Semantic
                && trace.status == PlannerRouteStatus::Skipped
                && trace
                    .notes
                    .iter()
                    .any(|note| note.contains("deterministic impact seed"))
        }));
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Impact
                && trace.status == PlannerRouteStatus::Executed
        }));
    }

    #[test]
    fn registry_prunes_lower_priority_routes_when_timeout_budget_is_exhausted() {
        let mut query_ir = ir();
        query_ir.budgets.total_timeout_ms = 250;

        let registry = PlannerRouteRegistry::new(vec![
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Symbol,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Symbol,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(
                        PlannerRouteKind::Symbol,
                        "packages/auth/src/session/service.ts",
                    )],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 5,
                    ambiguity: None,
                },
            }),
            Arc::new(FakeAdapter {
                route_kind: PlannerRouteKind::Semantic,
                capability: PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Semantic,
                    enabled: true,
                    available: true,
                    readiness: PlannerRouteReadiness::Ready,
                    reason: None,
                    supported_filters: full_filter_capabilities(),
                    constraints: PlannerRouteConstraints::default(),
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                },
                execution: PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: vec![candidate(PlannerRouteKind::Semantic, "src/session.ts")],
                    diagnostics: Vec::new(),
                    notes: Vec::new(),
                    elapsed_ms: 7,
                    ambiguity: None,
                },
            }),
        ]);

        let plan = registry.plan(&PlannerRuntimeContext::default(), &snapshot(), &query_ir);

        assert!(plan.budget_exhausted);
        assert_eq!(plan.candidates.len(), 1);
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Symbol
                && trace.status == PlannerRouteStatus::Executed
        }));
        assert!(plan.traces.iter().any(|trace| {
            trace.route_kind == PlannerRouteKind::Semantic
                && trace.status == PlannerRouteStatus::Skipped
                && trace
                    .notes
                    .iter()
                    .any(|note| note.contains("total timeout budget"))
        }));
    }
}
