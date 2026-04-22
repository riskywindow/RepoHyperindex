use std::sync::Arc;

use hyperindex_protocol::planner::{
    PlannerAmbiguity, PlannerDiagnostic, PlannerQueryIr, PlannerRouteKind, PlannerRouteSkipReason,
    PlannerRouteStatus, PlannerRouteTrace,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerRoutePlan {
    pub traces: Vec<PlannerRouteTrace>,
    pub capabilities: Vec<PlannerRouteCapabilityReport>,
    pub candidates: Vec<NormalizedPlannerCandidate>,
    pub diagnostics: Vec<PlannerDiagnostic>,
    pub ambiguity: Option<PlannerAmbiguity>,
    pub raw_candidate_count: u32,
}

impl PlannerRoutePlan {
    pub fn routes_considered(&self) -> u32 {
        self.traces.len() as u32
    }

    pub fn routes_available(&self) -> u32 {
        self.traces.iter().filter(|trace| trace.available).count() as u32
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
        let mut traces = Vec::with_capacity(capabilities.len());
        let mut diagnostics = capabilities
            .iter()
            .flat_map(|report| report.diagnostics.clone())
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        let mut ambiguity = None;
        let mut raw_candidate_count = 0_u32;

        for capability in &capabilities {
            let route_kind = capability.route_kind.clone();
            let budget = budget_for_route(&ir.budgets, route_kind.clone());
            let disabled_by_hint = ir.route_hints.disabled_routes.contains(&route_kind);
            let selected = ir.planned_routes.contains(&route_kind);

            if disabled_by_hint {
                traces.push(PlannerRouteTrace {
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
                });
                continue;
            }

            if !selected {
                traces.push(PlannerRouteTrace {
                    route_kind,
                    available: capability.available,
                    selected: false,
                    status: PlannerRouteStatus::Skipped,
                    skip_reason: Some(PlannerRouteSkipReason::FilteredByMode),
                    budget: Some(budget),
                    candidate_count: None,
                    group_count: None,
                    elapsed_ms: None,
                    notes: vec![
                        "route omitted from the deterministic planner route graph".to_string(),
                    ],
                });
                continue;
            }

            if !capability.available {
                let skip_reason = if matches!(route_kind, PlannerRouteKind::Exact) {
                    PlannerRouteSkipReason::ExactEngineUnavailable
                } else {
                    PlannerRouteSkipReason::CapabilityDisabled
                };
                traces.push(PlannerRouteTrace {
                    route_kind,
                    available: false,
                    selected,
                    status: PlannerRouteStatus::Skipped,
                    skip_reason: Some(skip_reason),
                    budget: Some(budget),
                    candidate_count: None,
                    group_count: None,
                    elapsed_ms: None,
                    notes: capability.notes.clone(),
                });
                continue;
            }

            let unsupported_filters = capability.unsupported_filters(&ir.filters);
            if !unsupported_filters.is_empty() {
                let filters = unsupported_filters.join(", ");
                diagnostics.push(scaffold_warning(
                    format!("{}_unsupported_filters", route_code(&route_kind)),
                    format!(
                        "{route_kind:?} route skipped because the current adapter does not support requested filters: {filters}"
                    ),
                ));
                traces.push(PlannerRouteTrace {
                    route_kind,
                    available: capability.available,
                    selected,
                    status: PlannerRouteStatus::Skipped,
                    skip_reason: Some(PlannerRouteSkipReason::CapabilityDisabled),
                    budget: Some(budget),
                    candidate_count: None,
                    group_count: None,
                    elapsed_ms: None,
                    notes: vec![format!("unsupported requested filters: {filters}")],
                });
                continue;
            }

            let request = PlannerRouteRequest {
                runtime: context,
                snapshot,
                ir,
                budget: budget.clone(),
            };
            let execution = self
                .adapter_for(&route_kind)
                .map(|adapter| adapter.execute(&request))
                .unwrap_or_else(|| missing_route_execution(route_kind.clone()));
            raw_candidate_count += execution.candidates.len() as u32;

            let raw_route_candidate_count = execution.candidates.len() as u32;
            let filtered = execution
                .candidates
                .into_iter()
                .filter(|candidate| candidate.matches_filters(&ir.filters, snapshot))
                .collect::<Vec<_>>();
            if ambiguity.is_none() {
                ambiguity = execution.ambiguity.clone();
            }
            let filtered_out = raw_route_candidate_count.saturating_sub(filtered.len() as u32);
            let mut notes = capability.notes.clone();
            notes.extend(execution.notes.clone());
            if filtered_out > 0 {
                notes.push(format!(
                    "{filtered_out} candidate(s) filtered after normalization"
                ));
            }
            diagnostics.extend(execution.diagnostics.clone());

            let (status, skip_reason, candidate_count) = match execution.state {
                PlannerRouteExecutionState::Deferred => (
                    PlannerRouteStatus::Deferred,
                    Some(PlannerRouteSkipReason::ExecutionDeferred),
                    None,
                ),
                PlannerRouteExecutionState::Executed => (
                    PlannerRouteStatus::Executed,
                    None,
                    Some(filtered.len() as u32),
                ),
            };

            traces.push(PlannerRouteTrace {
                route_kind,
                available: capability.available,
                selected,
                status,
                skip_reason,
                budget: Some(budget),
                candidate_count,
                group_count: None,
                elapsed_ms: Some(execution.elapsed_ms),
                notes,
            });
            candidates.extend(filtered);
        }

        PlannerRoutePlan {
            traces,
            capabilities,
            candidates,
            diagnostics,
            ambiguity,
            raw_candidate_count,
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
        PlannerAnchor, PlannerEvidenceItem, PlannerEvidenceKind, PlannerMode, PlannerQueryFilters,
        PlannerQueryIr, PlannerQueryStyle, PlannerRouteHints, PlannerRouteKind, PlannerRouteStatus,
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
}
