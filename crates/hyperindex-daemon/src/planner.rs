use std::cmp::min;
use std::path::Path;
use std::sync::Arc;

use hyperindex_config::LoadedConfig;
use hyperindex_planner::route_adapters::{
    NormalizedPlannerCandidate, PlannerRouteCapabilityReport, PlannerRouteConstraints,
    PlannerRouteExecution, PlannerRouteExecutionState, full_filter_capabilities,
};
use hyperindex_planner::{
    PlannerError, PlannerRouteAdapter, PlannerRouteReadiness, PlannerRouteRegistry,
    PlannerRouteRequest, PlannerRuntimeContext, PlannerWorkspace,
};
use hyperindex_protocol::errors::ProtocolError;
use hyperindex_protocol::impact::{
    ImpactAnalysisState, ImpactAnalyzeParams, ImpactChangeScenario, ImpactDiagnosticSeverity,
    ImpactEntityRef, ImpactReasonPath, ImpactTargetRef,
};
use hyperindex_protocol::planner::{
    PlannerAmbiguity, PlannerAmbiguityReason, PlannerCapabilitiesParams,
    PlannerCapabilitiesResponse, PlannerContextRef, PlannerDiagnostic, PlannerDiagnosticSeverity,
    PlannerEvidenceItem, PlannerEvidenceKind, PlannerExplainParams, PlannerExplainResponse,
    PlannerFilterCapabilities, PlannerQueryParams, PlannerQueryResponse, PlannerRouteKind,
    PlannerStatusParams, PlannerStatusResponse,
};
use hyperindex_protocol::repo::RepoRecord;
use hyperindex_protocol::semantic::{
    SemanticAnalysisState, SemanticDiagnosticSeverity, SemanticQueryFilters, SemanticQueryParams,
    SemanticQueryText,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::symbols::{
    ParseDiagnosticSeverity, SymbolSearchMode, SymbolSearchParams, SymbolSearchQuery,
};
use hyperindex_repo_store::RepoStore;
use hyperindex_symbol_store::{IndexedSnapshotState, SymbolStore};
use tracing::info;

use crate::impact::{ImpactService, build_graph_from_store};
use crate::semantic::SemanticService;
use crate::symbols::ParserSymbolService;

#[derive(Debug, Default, Clone)]
pub struct PlannerService;

impl PlannerService {
    pub fn status(
        &self,
        loaded: &LoadedConfig,
        snapshot: &ComposedSnapshot,
        params: &PlannerStatusParams,
    ) -> Result<PlannerStatusResponse, ProtocolError> {
        validate_snapshot_scope(snapshot, &params.repo_id, &params.snapshot_id)?;
        let context = runtime_context(loaded);
        let registry = route_registry(loaded);
        let route_reports = registry.capabilities(&context, snapshot);
        let route_capabilities = route_reports
            .iter()
            .map(PlannerRouteCapabilityReport::to_public_capability)
            .collect::<Vec<_>>();

        Ok(PlannerStatusResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            state: context.query_state_for_routes(&route_capabilities),
            capabilities: context.capabilities_for_routes(route_capabilities),
            diagnostics: status_diagnostics(&context, &route_reports),
        })
    }

    pub fn capabilities(
        &self,
        loaded: &LoadedConfig,
        snapshot: &ComposedSnapshot,
        params: &PlannerCapabilitiesParams,
    ) -> Result<PlannerCapabilitiesResponse, ProtocolError> {
        validate_snapshot_scope(snapshot, &params.repo_id, &params.snapshot_id)?;
        let context = runtime_context(loaded);
        let registry = route_registry(loaded);
        let route_reports = registry.capabilities(&context, snapshot);
        let route_capabilities = route_reports
            .iter()
            .map(PlannerRouteCapabilityReport::to_public_capability)
            .collect::<Vec<_>>();

        Ok(PlannerCapabilitiesResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            default_mode: context.default_mode.clone(),
            default_limit: context.default_limit,
            max_limit: context.max_limit,
            budgets: context.budget_policy.clone(),
            capabilities: context.capabilities_for_routes(route_capabilities),
            diagnostics: status_diagnostics(&context, &route_reports),
        })
    }

    pub fn query(
        &self,
        loaded: &LoadedConfig,
        snapshot: &ComposedSnapshot,
        params: &PlannerQueryParams,
    ) -> Result<PlannerQueryResponse, ProtocolError> {
        info!(
            repo_id = %params.repo_id,
            snapshot_id = %params.snapshot_id,
            include_trace = params.include_trace,
            explain = params.explain,
            "serving phase7 unified planner query"
        );

        let context = runtime_context(loaded);
        PlannerWorkspace::with_route_registry(route_registry(loaded))
            .plan(&context, params, snapshot)
            .map_err(map_planner_error)
    }

    pub fn explain(
        &self,
        loaded: &LoadedConfig,
        snapshot: &ComposedSnapshot,
        params: &PlannerExplainParams,
    ) -> Result<PlannerExplainResponse, ProtocolError> {
        info!(
            repo_id = %params.query.repo_id,
            snapshot_id = %params.query.snapshot_id,
            "serving phase7 planner explain trace"
        );

        let context = runtime_context(loaded);
        PlannerWorkspace::with_route_registry(route_registry(loaded))
            .explain(&context, &params.query, snapshot)
            .map_err(map_planner_error)
    }
}

fn route_registry(loaded: &LoadedConfig) -> PlannerRouteRegistry {
    PlannerRouteRegistry::new(vec![
        Arc::new(hyperindex_planner::exact_route::UnavailableExactRouteProvider),
        Arc::new(SymbolPlannerRouteAdapter::new(loaded)),
        Arc::new(SemanticPlannerRouteAdapter::new(loaded)),
        Arc::new(ImpactPlannerRouteAdapter::new(loaded)),
    ])
}

fn status_diagnostics(
    context: &PlannerRuntimeContext,
    route_reports: &[PlannerRouteCapabilityReport],
) -> Vec<PlannerDiagnostic> {
    let mut diagnostics = hyperindex_planner::daemon_integration::runtime_diagnostics(context);
    diagnostics.extend(
        route_reports
            .iter()
            .flat_map(|report| report.diagnostics.clone()),
    );
    diagnostics
}

fn runtime_context(loaded: &LoadedConfig) -> PlannerRuntimeContext {
    PlannerRuntimeContext {
        planner_enabled: loaded.config.planner.enabled,
        exact_enabled: loaded.config.planner.routes.exact_enabled,
        symbol_available: loaded.config.symbol_index.enabled
            && loaded.config.planner.routes.symbol_enabled,
        semantic_available: loaded.config.semantic.enabled
            && loaded.config.planner.routes.semantic_enabled,
        impact_available: loaded.config.impact.enabled
            && loaded.config.planner.routes.impact_enabled,
        exact_available: false,
        default_mode: loaded.config.planner.default_mode.clone(),
        default_limit: loaded.config.planner.default_limit as u32,
        max_limit: loaded.config.planner.max_limit as u32,
        budget_policy: loaded.config.planner.budgets.clone(),
    }
}

fn validate_snapshot_scope(
    snapshot: &ComposedSnapshot,
    repo_id: &str,
    snapshot_id: &str,
) -> Result<(), ProtocolError> {
    if snapshot.repo_id != repo_id || snapshot.snapshot_id != snapshot_id {
        return Err(ProtocolError::invalid_request(format!(
            "planner snapshot mismatch: requested repo={repo_id} snapshot={snapshot_id}, loaded repo={} snapshot={}",
            snapshot.repo_id, snapshot.snapshot_id
        )));
    }
    Ok(())
}

fn map_planner_error(error: PlannerError) -> ProtocolError {
    match error {
        PlannerError::InvalidQuery(message) => ProtocolError::invalid_field(
            "query.text",
            message,
            Some("non-empty planner query text".to_string()),
        ),
        PlannerError::SnapshotMismatch { .. } => ProtocolError::invalid_request(error.to_string()),
    }
}

#[derive(Debug, Clone)]
struct SymbolPlannerRouteAdapter {
    loaded: LoadedConfig,
}

impl SymbolPlannerRouteAdapter {
    fn new(loaded: &LoadedConfig) -> Self {
        Self {
            loaded: loaded.clone(),
        }
    }
}

impl PlannerRouteAdapter for SymbolPlannerRouteAdapter {
    fn kind(&self) -> PlannerRouteKind {
        PlannerRouteKind::Symbol
    }

    fn capability(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
    ) -> PlannerRouteCapabilityReport {
        if !context.route_enabled(PlannerRouteKind::Symbol) {
            return disabled_route_capability(
                PlannerRouteKind::Symbol,
                "symbol route is disabled by runtime configuration",
                symbol_filter_capabilities(),
                symbol_constraints(),
            );
        }

        let indexed = symbol_index_state(
            &self.loaded.config.symbol_index.store_dir,
            &snapshot.repo_id,
            &snapshot.snapshot_id,
        )
        .is_some();
        PlannerRouteCapabilityReport {
            route_kind: PlannerRouteKind::Symbol,
            enabled: true,
            available: true,
            readiness: if indexed {
                PlannerRouteReadiness::Ready
            } else {
                PlannerRouteReadiness::Unbuilt
            },
            reason: if indexed {
                None
            } else {
                Some("symbol build will be materialized on demand for this snapshot".to_string())
            },
            supported_filters: symbol_filter_capabilities(),
            constraints: symbol_constraints(),
            diagnostics: if indexed {
                Vec::new()
            } else {
                vec![planner_diagnostic(
                    PlannerDiagnosticSeverity::Info,
                    "symbol_route_build_missing",
                    "symbol search is available, but this snapshot will build symbol facts on demand",
                )]
            },
            notes: vec![if indexed {
                "symbol route has a ready indexed snapshot".to_string()
            } else {
                "symbol route will build facts lazily for this snapshot".to_string()
            }],
        }
    }

    fn execute(&self, request: &PlannerRouteRequest<'_>) -> PlannerRouteExecution {
        let query = request
            .ir
            .symbol_query
            .as_ref()
            .map(|query| query.normalized_symbol.clone())
            .unwrap_or_else(|| request.ir.normalized_query.clone());
        let service = ParserSymbolService::from_loaded_config(&self.loaded);
        let params = SymbolSearchParams {
            repo_id: request.snapshot.repo_id.clone(),
            snapshot_id: request.snapshot.snapshot_id.clone(),
            query: SymbolSearchQuery {
                text: query,
                mode: SymbolSearchMode::Exact,
                kinds: request.ir.filters.symbol_kinds.clone(),
                path_prefix: None,
            },
            limit: bounded_limit(request),
        };

        match service.search(
            &repo_record_from_snapshot(request.snapshot),
            request.snapshot,
            &params,
        ) {
            Ok(response) => PlannerRouteExecution {
                state: PlannerRouteExecutionState::Executed,
                candidates: response
                    .hits
                    .iter()
                    .enumerate()
                    .map(|(index, hit)| normalize_symbol_hit(hit, index, request.snapshot))
                    .collect(),
                diagnostics: response
                    .diagnostics
                    .iter()
                    .map(map_parse_diagnostic)
                    .collect(),
                notes: vec![format!("symbol_hits={}", response.hits.len())],
                elapsed_ms: 0,
                ambiguity: None,
            },
            Err(error) => PlannerRouteExecution {
                state: PlannerRouteExecutionState::Executed,
                candidates: Vec::new(),
                diagnostics: vec![planner_diagnostic(
                    PlannerDiagnosticSeverity::Error,
                    "symbol_route_execution_failed",
                    error.to_string(),
                )],
                notes: vec!["symbol route failed cleanly and returned no candidates".to_string()],
                elapsed_ms: 0,
                ambiguity: None,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct SemanticPlannerRouteAdapter {
    loaded: LoadedConfig,
}

impl SemanticPlannerRouteAdapter {
    fn new(loaded: &LoadedConfig) -> Self {
        Self {
            loaded: loaded.clone(),
        }
    }
}

impl PlannerRouteAdapter for SemanticPlannerRouteAdapter {
    fn kind(&self) -> PlannerRouteKind {
        PlannerRouteKind::Semantic
    }

    fn capability(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
    ) -> PlannerRouteCapabilityReport {
        if !context.route_enabled(PlannerRouteKind::Semantic) {
            return disabled_route_capability(
                PlannerRouteKind::Semantic,
                "semantic route is disabled by runtime configuration",
                full_filter_capabilities(),
                semantic_constraints(),
            );
        }

        let status = SemanticService.status(
            &self.loaded.config.semantic.store_dir,
            &self.loaded.config.symbol_index.store_dir,
            &self.loaded.config.semantic,
            &hyperindex_protocol::semantic::SemanticStatusParams {
                repo_id: snapshot.repo_id.clone(),
                snapshot_id: snapshot.snapshot_id.clone(),
                build_id: None,
            },
        );

        match status {
            Ok(status) => {
                let (available, readiness, reason) = match status.state {
                    SemanticAnalysisState::Ready => (true, PlannerRouteReadiness::Ready, None),
                    SemanticAnalysisState::Disabled => (
                        false,
                        PlannerRouteReadiness::Disabled,
                        Some("semantic retrieval is disabled in runtime config".to_string()),
                    ),
                    SemanticAnalysisState::Stale => (
                        false,
                        PlannerRouteReadiness::Degraded,
                        Some("semantic build is stale for the current config".to_string()),
                    ),
                    SemanticAnalysisState::NotReady if status.builds.is_empty() => (
                        false,
                        PlannerRouteReadiness::Unbuilt,
                        Some("semantic build is missing for this snapshot".to_string()),
                    ),
                    SemanticAnalysisState::NotReady => (
                        false,
                        PlannerRouteReadiness::Degraded,
                        Some("semantic route is not query-ready for this snapshot".to_string()),
                    ),
                };
                PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Semantic,
                    enabled: true,
                    available,
                    readiness,
                    reason,
                    supported_filters: full_filter_capabilities(),
                    constraints: semantic_constraints(),
                    diagnostics: status
                        .diagnostics
                        .iter()
                        .map(map_semantic_diagnostic)
                        .collect(),
                    notes: vec![format!("semantic_state={:?}", status.state)],
                }
            }
            Err(error) => degraded_route_capability(
                PlannerRouteKind::Semantic,
                "semantic_route_status_failed",
                error.message.clone(),
                full_filter_capabilities(),
                semantic_constraints(),
                false,
            ),
        }
    }

    fn execute(&self, request: &PlannerRouteRequest<'_>) -> PlannerRouteExecution {
        let query = request
            .ir
            .semantic_query
            .as_ref()
            .map(|query| query.normalized_text.clone())
            .unwrap_or_else(|| request.ir.normalized_query.clone());

        match SemanticService.query(
            &self.loaded.config.semantic.store_dir,
            &self.loaded.config.symbol_index.store_dir,
            &self.loaded.config.semantic,
            &SemanticQueryParams {
                repo_id: request.snapshot.repo_id.clone(),
                snapshot_id: request.snapshot.snapshot_id.clone(),
                query: SemanticQueryText { text: query },
                filters: semantic_filters(&request.ir.filters),
                limit: bounded_limit(request),
                rerank_mode: self
                    .loaded
                    .config
                    .semantic
                    .query
                    .default_rerank_mode
                    .clone(),
            },
        ) {
            Ok(response) => PlannerRouteExecution {
                state: PlannerRouteExecutionState::Executed,
                candidates: response.hits.iter().map(normalize_semantic_hit).collect(),
                diagnostics: response
                    .diagnostics
                    .iter()
                    .map(map_semantic_diagnostic)
                    .collect(),
                notes: vec![format!("semantic_hits={}", response.hits.len())],
                elapsed_ms: response.stats.elapsed_ms,
                ambiguity: None,
            },
            Err(error) => PlannerRouteExecution {
                state: PlannerRouteExecutionState::Executed,
                candidates: Vec::new(),
                diagnostics: vec![planner_diagnostic(
                    PlannerDiagnosticSeverity::Warning,
                    "semantic_route_execution_failed",
                    error.message.clone(),
                )],
                notes: vec!["semantic route returned no candidates".to_string()],
                elapsed_ms: 0,
                ambiguity: None,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct ImpactPlannerRouteAdapter {
    loaded: LoadedConfig,
}

impl ImpactPlannerRouteAdapter {
    fn new(loaded: &LoadedConfig) -> Self {
        Self {
            loaded: loaded.clone(),
        }
    }
}

impl PlannerRouteAdapter for ImpactPlannerRouteAdapter {
    fn kind(&self) -> PlannerRouteKind {
        PlannerRouteKind::Impact
    }

    fn capability(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
    ) -> PlannerRouteCapabilityReport {
        if !context.route_enabled(PlannerRouteKind::Impact) {
            return disabled_route_capability(
                PlannerRouteKind::Impact,
                "impact route is disabled by runtime configuration",
                impact_filter_capabilities(),
                impact_constraints(),
            );
        }

        let status = ImpactService.status(
            &self.loaded.config.impact.store_dir,
            &self.loaded.config.symbol_index.store_dir,
            &self.loaded.config.impact,
            &hyperindex_protocol::impact::ImpactStatusParams {
                repo_id: snapshot.repo_id.clone(),
                snapshot_id: snapshot.snapshot_id.clone(),
            },
        );

        match status {
            Ok(status) => {
                let (available, readiness, reason) = match status.state {
                    ImpactAnalysisState::Ready if status.manifest.is_none() => (
                        true,
                        PlannerRouteReadiness::Unbuilt,
                        Some(
                            "impact build will materialize on demand for this snapshot".to_string(),
                        ),
                    ),
                    ImpactAnalysisState::Ready => (true, PlannerRouteReadiness::Ready, None),
                    ImpactAnalysisState::Stale => (
                        true,
                        PlannerRouteReadiness::Degraded,
                        Some(
                            "stored impact build is stale and will refresh on analyze".to_string(),
                        ),
                    ),
                    ImpactAnalysisState::NotReady => (
                        false,
                        PlannerRouteReadiness::Unbuilt,
                        Some(
                            "impact analysis needs a ready symbol index for this snapshot"
                                .to_string(),
                        ),
                    ),
                };
                PlannerRouteCapabilityReport {
                    route_kind: PlannerRouteKind::Impact,
                    enabled: true,
                    available,
                    readiness,
                    reason,
                    supported_filters: impact_filter_capabilities(),
                    constraints: impact_constraints(),
                    diagnostics: status
                        .diagnostics
                        .iter()
                        .map(map_impact_diagnostic)
                        .collect(),
                    notes: vec![format!("impact_state={:?}", status.state)],
                }
            }
            Err(error) => degraded_route_capability(
                PlannerRouteKind::Impact,
                "impact_route_status_failed",
                error.message.clone(),
                impact_filter_capabilities(),
                impact_constraints(),
                false,
            ),
        }
    }

    fn execute(&self, request: &PlannerRouteRequest<'_>) -> PlannerRouteExecution {
        let resolution = resolve_impact_target(&self.loaded, request);
        if let Some(ambiguity) = resolution.ambiguity {
            return PlannerRouteExecution {
                state: PlannerRouteExecutionState::Executed,
                candidates: Vec::new(),
                diagnostics: resolution.diagnostics,
                notes: resolution.notes,
                elapsed_ms: 0,
                ambiguity: Some(ambiguity),
            };
        }
        let Some(target) = resolution.target else {
            return PlannerRouteExecution {
                state: PlannerRouteExecutionState::Executed,
                candidates: Vec::new(),
                diagnostics: resolution.diagnostics,
                notes: resolution.notes,
                elapsed_ms: 0,
                ambiguity: None,
            };
        };

        let repo_store = match RepoStore::open_from_config(&self.loaded.config) {
            Ok(store) => store,
            Err(error) => {
                return PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: Vec::new(),
                    diagnostics: vec![planner_diagnostic(
                        PlannerDiagnosticSeverity::Error,
                        "impact_repo_store_open_failed",
                        error.to_string(),
                    )],
                    notes: vec!["impact route failed before analysis".to_string()],
                    elapsed_ms: 0,
                    ambiguity: None,
                };
            }
        };
        let graph = match build_graph_from_store(
            &self.loaded.config.symbol_index.store_dir,
            &request.snapshot.repo_id,
            request.snapshot,
        ) {
            Ok(graph) => graph,
            Err(error) => {
                return PlannerRouteExecution {
                    state: PlannerRouteExecutionState::Executed,
                    candidates: Vec::new(),
                    diagnostics: vec![planner_diagnostic(
                        PlannerDiagnosticSeverity::Warning,
                        "impact_symbol_graph_unavailable",
                        error.message.clone(),
                    )],
                    notes: vec!["impact route could not materialize the symbol graph".to_string()],
                    elapsed_ms: 0,
                    ambiguity: None,
                };
            }
        };
        let change_hint = impact_change_hint(request);

        match ImpactService.analyze(
            &self.loaded.config.impact.store_dir,
            &repo_store,
            &self.loaded.config.impact,
            &graph,
            request.snapshot,
            &ImpactAnalyzeParams {
                repo_id: request.snapshot.repo_id.clone(),
                snapshot_id: request.snapshot.snapshot_id.clone(),
                target: target.clone(),
                change_hint: change_hint.clone(),
                limit: bounded_limit(request),
                include_transitive: self.loaded.config.impact.default_include_transitive,
                include_reason_paths: self.loaded.config.impact.default_include_reason_paths,
                max_transitive_depth: Some(self.loaded.config.impact.max_transitive_depth),
                max_nodes_visited: None,
                max_edges_traversed: None,
                max_candidates_considered: None,
            },
        ) {
            Ok(response) => PlannerRouteExecution {
                state: PlannerRouteExecutionState::Executed,
                candidates: response
                    .groups
                    .iter()
                    .flat_map(|group| group.hits.iter())
                    .map(normalize_impact_hit)
                    .collect(),
                diagnostics: response
                    .diagnostics
                    .iter()
                    .map(map_impact_diagnostic)
                    .collect(),
                notes: {
                    let mut notes = resolution.notes;
                    notes.push(format!(
                        "impact_target={} change_hint={:?}",
                        impact_target_label(&target),
                        change_hint
                    ));
                    notes
                },
                elapsed_ms: response.stats.elapsed_ms,
                ambiguity: None,
            },
            Err(error) => PlannerRouteExecution {
                state: PlannerRouteExecutionState::Executed,
                candidates: Vec::new(),
                diagnostics: {
                    let mut diagnostics = resolution.diagnostics;
                    diagnostics.push(planner_diagnostic(
                        PlannerDiagnosticSeverity::Warning,
                        "impact_route_execution_failed",
                        error.message.clone(),
                    ));
                    diagnostics
                },
                notes: resolution.notes,
                elapsed_ms: 0,
                ambiguity: None,
            },
        }
    }
}

#[derive(Debug, Default)]
struct ImpactTargetResolution {
    target: Option<ImpactTargetRef>,
    ambiguity: Option<PlannerAmbiguity>,
    diagnostics: Vec<PlannerDiagnostic>,
    notes: Vec<String>,
}

fn resolve_impact_target(
    loaded: &LoadedConfig,
    request: &PlannerRouteRequest<'_>,
) -> ImpactTargetResolution {
    if let Some(context) = request
        .ir
        .selected_context
        .as_ref()
        .or(request.ir.target_context.as_ref())
    {
        if let Some(target) = impact_target_from_context(context) {
            return ImpactTargetResolution {
                target: Some(target),
                ambiguity: None,
                diagnostics: Vec::new(),
                notes: vec!["impact target resolved from planner context".to_string()],
            };
        }
    }

    let Some(intent) = request.ir.impact_query.as_ref() else {
        return ImpactTargetResolution {
            target: None,
            ambiguity: None,
            diagnostics: vec![planner_diagnostic(
                PlannerDiagnosticSeverity::Warning,
                "impact_target_missing",
                "impact route has no normalized impact seed to analyze",
            )],
            notes: vec!["impact target seed is missing".to_string()],
        };
    };
    let subject = intent.subject_terms.join(" ").trim().to_string();
    if subject.is_empty() {
        return ImpactTargetResolution {
            target: None,
            ambiguity: None,
            diagnostics: vec![planner_diagnostic(
                PlannerDiagnosticSeverity::Warning,
                "impact_target_missing",
                "impact route requires a concrete file or symbol seed",
            )],
            notes: vec!["impact query did not resolve a concrete target".to_string()],
        };
    }
    if looks_like_path(&subject) {
        return ImpactTargetResolution {
            target: Some(ImpactTargetRef::File { path: subject }),
            ambiguity: None,
            diagnostics: Vec::new(),
            notes: vec!["impact target resolved as a file path".to_string()],
        };
    }

    let service = ParserSymbolService::from_loaded_config(loaded);
    let params = SymbolSearchParams {
        repo_id: request.snapshot.repo_id.clone(),
        snapshot_id: request.snapshot.snapshot_id.clone(),
        query: SymbolSearchQuery {
            text: subject.clone(),
            mode: SymbolSearchMode::Exact,
            kinds: request.ir.filters.symbol_kinds.clone(),
            path_prefix: None,
        },
        limit: min(request.budget.max_candidates.max(1), 4),
    };

    match service.search(
        &repo_record_from_snapshot(request.snapshot),
        request.snapshot,
        &params,
    ) {
        Ok(response) if response.hits.len() == 1 => {
            let hit = &response.hits[0];
            ImpactTargetResolution {
                target: Some(ImpactTargetRef::Symbol {
                    value: hit.symbol.display_name.clone(),
                    symbol_id: Some(hit.symbol.symbol_id.clone()),
                    path: Some(hit.symbol.path.clone()),
                }),
                ambiguity: None,
                diagnostics: response
                    .diagnostics
                    .iter()
                    .map(map_parse_diagnostic)
                    .collect(),
                notes: vec!["impact target resolved through symbol lookup".to_string()],
            }
        }
        Ok(response) if response.hits.len() > 1 => ImpactTargetResolution {
            target: None,
            ambiguity: Some(PlannerAmbiguity {
                reason: PlannerAmbiguityReason::MultipleCandidateSeeds,
                details: vec![format!(
                    "impact target '{subject}' matched {} symbol seeds",
                    response.hits.len()
                )],
                candidate_contexts: response
                    .hits
                    .iter()
                    .take(4)
                    .map(|hit| PlannerContextRef::Symbol {
                        symbol_id: hit.symbol.symbol_id.clone(),
                        path: hit.symbol.path.clone(),
                        span: Some(hit.symbol.span.clone()),
                        display_name: Some(hit.symbol.display_name.clone()),
                    })
                    .collect(),
            }),
            diagnostics: response
                .diagnostics
                .iter()
                .map(map_parse_diagnostic)
                .collect(),
            notes: vec!["impact target resolution was ambiguous".to_string()],
        },
        Ok(response) => ImpactTargetResolution {
            target: None,
            ambiguity: None,
            diagnostics: {
                let mut diagnostics = response
                    .diagnostics
                    .iter()
                    .map(map_parse_diagnostic)
                    .collect::<Vec<_>>();
                diagnostics.push(planner_diagnostic(
                    PlannerDiagnosticSeverity::Warning,
                    "impact_target_unresolved",
                    format!("no symbol seed matched impact target '{subject}'"),
                ));
                diagnostics
            },
            notes: vec!["impact target lookup returned no symbol match".to_string()],
        },
        Err(error) => ImpactTargetResolution {
            target: None,
            ambiguity: None,
            diagnostics: vec![planner_diagnostic(
                PlannerDiagnosticSeverity::Warning,
                "impact_target_resolution_failed",
                error.to_string(),
            )],
            notes: vec!["impact target resolution failed cleanly".to_string()],
        },
    }
}

fn impact_target_from_context(context: &PlannerContextRef) -> Option<ImpactTargetRef> {
    match context {
        PlannerContextRef::Symbol {
            symbol_id,
            path,
            display_name,
            ..
        } => Some(ImpactTargetRef::Symbol {
            value: display_name.clone().unwrap_or_else(|| symbol_id.0.clone()),
            symbol_id: Some(symbol_id.clone()),
            path: Some(path.clone()),
        }),
        PlannerContextRef::Span { path, .. } | PlannerContextRef::File { path } => {
            Some(ImpactTargetRef::File { path: path.clone() })
        }
        PlannerContextRef::Impact { entity } => match entity {
            ImpactEntityRef::Symbol {
                symbol_id,
                path,
                display_name,
            } => Some(ImpactTargetRef::Symbol {
                value: display_name.clone(),
                symbol_id: Some(symbol_id.clone()),
                path: Some(path.clone()),
            }),
            ImpactEntityRef::File { path } | ImpactEntityRef::Test { path, .. } => {
                Some(ImpactTargetRef::File { path: path.clone() })
            }
            ImpactEntityRef::Package { .. } => None,
        },
        PlannerContextRef::Package { .. } | PlannerContextRef::Workspace { .. } => None,
    }
}

fn impact_change_hint(request: &PlannerRouteRequest<'_>) -> ImpactChangeScenario {
    let Some(intent) = request.ir.impact_query.as_ref() else {
        return ImpactChangeScenario::ModifyBehavior;
    };
    if intent
        .action_terms
        .iter()
        .any(|term| term.eq_ignore_ascii_case("rename"))
    {
        ImpactChangeScenario::Rename
    } else if intent.action_terms.iter().any(|term| {
        matches!(
            term.to_ascii_lowercase().as_str(),
            "delete" | "remove" | "drop"
        )
    }) {
        ImpactChangeScenario::Delete
    } else if intent.action_terms.iter().any(|term| {
        matches!(
            term.to_ascii_lowercase().as_str(),
            "signature" | "parameter" | "parameters"
        )
    }) {
        ImpactChangeScenario::SignatureChange
    } else {
        ImpactChangeScenario::ModifyBehavior
    }
}

fn normalize_symbol_hit(
    hit: &hyperindex_protocol::symbols::SymbolSearchHit,
    index: usize,
    snapshot: &ComposedSnapshot,
) -> NormalizedPlannerCandidate {
    NormalizedPlannerCandidate {
        candidate_id: format!("symbol:{}", hit.symbol.symbol_id.0),
        route_kind: PlannerRouteKind::Symbol,
        engine_type: PlannerRouteKind::Symbol,
        label: hit.symbol.display_name.clone(),
        anchor: hyperindex_protocol::planner::PlannerAnchor::Symbol {
            symbol_id: hit.symbol.symbol_id.clone(),
            path: hit.symbol.path.clone(),
            span: Some(hit.symbol.span.clone()),
        },
        rank: Some((index + 1) as u32),
        engine_score: Some(hit.score),
        normalized_score: None,
        primary_path: Some(hit.symbol.path.clone()),
        primary_symbol_id: Some(hit.symbol.symbol_id.clone()),
        primary_span: Some(hit.symbol.span.clone()),
        language: Some(hit.symbol.language.clone()),
        extension: path_extension(&hit.symbol.path),
        symbol_kind: Some(hit.symbol.kind.clone()),
        package_name: None,
        package_root: None,
        workspace_root: Some(snapshot.repo_root.clone()),
        evidence: vec![PlannerEvidenceItem {
            evidence_kind: PlannerEvidenceKind::SymbolHit,
            route_kind: PlannerRouteKind::Symbol,
            label: hit.reason.clone(),
            path: Some(hit.symbol.path.clone()),
            span: Some(hit.symbol.span.clone()),
            symbol_id: Some(hit.symbol.symbol_id.clone()),
            impact_entity: None,
            snippet: None,
            score: Some(hit.score),
            notes: vec![format!("match_kind={:?}", hit.match_kind)],
        }],
        engine_diagnostics: Vec::new(),
        notes: vec![format!("match_kind={:?}", hit.match_kind)],
    }
}

fn normalize_semantic_hit(
    hit: &hyperindex_protocol::semantic::SemanticRetrievalHit,
) -> NormalizedPlannerCandidate {
    let anchor = if let Some(symbol_id) = hit.chunk.symbol_id.clone() {
        hyperindex_protocol::planner::PlannerAnchor::Symbol {
            symbol_id,
            path: hit.chunk.path.clone(),
            span: hit.chunk.span.clone(),
        }
    } else if let Some(span) = hit.chunk.span.clone() {
        hyperindex_protocol::planner::PlannerAnchor::Span {
            path: hit.chunk.path.clone(),
            span,
        }
    } else {
        hyperindex_protocol::planner::PlannerAnchor::File {
            path: hit.chunk.path.clone(),
        }
    };

    let mut notes = vec![
        hit.reason.clone(),
        format!("semantic_score={}", hit.semantic_score),
        format!("rerank_score={}", hit.rerank_score),
        format!("chunk_kind={:?}", hit.chunk.chunk_kind),
    ];
    if let Some(explanation) = &hit.explanation {
        if !explanation.query_terms.is_empty() {
            notes.push(format!("query_terms={}", explanation.query_terms.join(",")));
        }
        if !explanation.symbol_term_hits.is_empty() {
            notes.push(format!(
                "symbol_terms={}",
                explanation.symbol_term_hits.join(",")
            ));
        }
    }

    NormalizedPlannerCandidate {
        candidate_id: format!("semantic:{}", hit.chunk.chunk_id.0),
        route_kind: PlannerRouteKind::Semantic,
        engine_type: PlannerRouteKind::Semantic,
        label: hit
            .chunk
            .symbol_display_name
            .clone()
            .unwrap_or_else(|| hit.chunk.path.clone()),
        anchor,
        rank: Some(hit.rank),
        engine_score: Some(hit.score),
        normalized_score: None,
        primary_path: Some(hit.chunk.path.clone()),
        primary_symbol_id: hit.chunk.symbol_id.clone(),
        primary_span: hit.chunk.span.clone(),
        language: hit.chunk.language.clone(),
        extension: hit
            .chunk
            .extension
            .clone()
            .or_else(|| path_extension(&hit.chunk.path)),
        symbol_kind: hit.chunk.symbol_kind.clone(),
        package_name: hit.chunk.package_name.clone(),
        package_root: hit.chunk.package_root.clone(),
        workspace_root: hit.chunk.workspace_root.clone(),
        evidence: vec![PlannerEvidenceItem {
            evidence_kind: PlannerEvidenceKind::SemanticHit,
            route_kind: PlannerRouteKind::Semantic,
            label: hit.reason.clone(),
            path: Some(hit.chunk.path.clone()),
            span: hit.chunk.span.clone(),
            symbol_id: hit.chunk.symbol_id.clone(),
            impact_entity: None,
            snippet: Some(hit.snippet.clone()),
            score: Some(hit.score),
            notes: notes.clone(),
        }],
        engine_diagnostics: Vec::new(),
        notes,
    }
}

fn normalize_impact_hit(
    hit: &hyperindex_protocol::impact::ImpactHit,
) -> NormalizedPlannerCandidate {
    let (anchor, label, primary_path, primary_symbol_id, package_name, package_root) =
        impact_anchor_and_provenance(&hit.entity);
    let mut evidence_notes = vec![
        format!("certainty={:?}", hit.certainty),
        format!("primary_reason={:?}", hit.primary_reason),
        format!("direct={}", hit.direct),
        format!("depth={}", hit.depth),
    ];
    if let Some(explanation) = &hit.explanation {
        evidence_notes.push(explanation.why.clone());
        evidence_notes.push(explanation.change_effect.clone());
    }
    if let Some(summary) = hit.reason_paths.first().map(reason_path_summary) {
        evidence_notes.push(summary);
    }

    NormalizedPlannerCandidate {
        candidate_id: format!("impact:{}", impact_entity_key(&hit.entity)),
        route_kind: PlannerRouteKind::Impact,
        engine_type: PlannerRouteKind::Impact,
        label,
        anchor,
        rank: Some(hit.rank),
        engine_score: Some(hit.score),
        normalized_score: None,
        primary_path,
        primary_symbol_id,
        primary_span: None,
        language: None,
        extension: None,
        symbol_kind: None,
        package_name,
        package_root,
        workspace_root: None,
        evidence: vec![PlannerEvidenceItem {
            evidence_kind: PlannerEvidenceKind::ImpactHit,
            route_kind: PlannerRouteKind::Impact,
            label: format!("{:?} {:?}", hit.certainty, hit.primary_reason),
            path: impact_entity_path(&hit.entity),
            span: None,
            symbol_id: impact_entity_symbol_id(&hit.entity),
            impact_entity: Some(hit.entity.clone()),
            snippet: None,
            score: Some(hit.score),
            notes: evidence_notes.clone(),
        }],
        engine_diagnostics: Vec::new(),
        notes: evidence_notes,
    }
}

fn impact_anchor_and_provenance(
    entity: &ImpactEntityRef,
) -> (
    hyperindex_protocol::planner::PlannerAnchor,
    String,
    Option<String>,
    Option<hyperindex_protocol::symbols::SymbolId>,
    Option<String>,
    Option<String>,
) {
    match entity {
        ImpactEntityRef::Symbol {
            symbol_id,
            path,
            display_name,
        } => (
            hyperindex_protocol::planner::PlannerAnchor::Impact {
                entity: entity.clone(),
            },
            display_name.clone(),
            Some(path.clone()),
            Some(symbol_id.clone()),
            None,
            None,
        ),
        ImpactEntityRef::File { path } => (
            hyperindex_protocol::planner::PlannerAnchor::Impact {
                entity: entity.clone(),
            },
            path.clone(),
            Some(path.clone()),
            None,
            None,
            None,
        ),
        ImpactEntityRef::Package {
            package_name,
            package_root,
        } => (
            hyperindex_protocol::planner::PlannerAnchor::Impact {
                entity: entity.clone(),
            },
            package_name.clone(),
            None,
            None,
            Some(package_name.clone()),
            Some(package_root.clone()),
        ),
        ImpactEntityRef::Test {
            path,
            display_name,
            symbol_id,
        } => (
            hyperindex_protocol::planner::PlannerAnchor::Impact {
                entity: entity.clone(),
            },
            display_name.clone(),
            Some(path.clone()),
            symbol_id.clone(),
            None,
            None,
        ),
    }
}

fn impact_entity_path(entity: &ImpactEntityRef) -> Option<String> {
    match entity {
        ImpactEntityRef::Symbol { path, .. }
        | ImpactEntityRef::File { path }
        | ImpactEntityRef::Test { path, .. } => Some(path.clone()),
        ImpactEntityRef::Package { .. } => None,
    }
}

fn impact_entity_symbol_id(
    entity: &ImpactEntityRef,
) -> Option<hyperindex_protocol::symbols::SymbolId> {
    match entity {
        ImpactEntityRef::Symbol { symbol_id, .. } => Some(symbol_id.clone()),
        ImpactEntityRef::Test { symbol_id, .. } => symbol_id.clone(),
        ImpactEntityRef::File { .. } | ImpactEntityRef::Package { .. } => None,
    }
}

fn impact_entity_key(entity: &ImpactEntityRef) -> String {
    match entity {
        ImpactEntityRef::Symbol { symbol_id, .. } => format!("symbol:{}", symbol_id.0),
        ImpactEntityRef::File { path } => format!("file:{path}"),
        ImpactEntityRef::Package { package_name, .. } => format!("package:{package_name}"),
        ImpactEntityRef::Test {
            path,
            symbol_id,
            display_name,
        } => symbol_id
            .as_ref()
            .map(|symbol_id| format!("test-symbol:{}", symbol_id.0))
            .unwrap_or_else(|| format!("test:{path}:{display_name}")),
    }
}

fn reason_path_summary(path: &ImpactReasonPath) -> String {
    path.summary.clone()
}

fn impact_target_label(target: &ImpactTargetRef) -> String {
    match target {
        ImpactTargetRef::Symbol { value, .. } => format!("symbol:{value}"),
        ImpactTargetRef::File { path } => format!("file:{path}"),
    }
}

fn semantic_filters(
    filters: &hyperindex_protocol::planner::PlannerQueryFilters,
) -> SemanticQueryFilters {
    SemanticQueryFilters {
        path_globs: filters.path_globs.clone(),
        package_names: filters.package_names.clone(),
        package_roots: filters.package_roots.clone(),
        workspace_roots: filters.workspace_roots.clone(),
        languages: filters.languages.clone(),
        extensions: filters.extensions.clone(),
        symbol_kinds: filters.symbol_kinds.clone(),
    }
}

fn bounded_limit(request: &PlannerRouteRequest<'_>) -> u32 {
    min(request.ir.limit, request.budget.max_candidates)
}

fn repo_record_from_snapshot(snapshot: &ComposedSnapshot) -> RepoRecord {
    RepoRecord {
        repo_id: snapshot.repo_id.clone(),
        repo_root: snapshot.repo_root.clone(),
        display_name: snapshot.repo_id.clone(),
        created_at: "planner-route".to_string(),
        updated_at: "planner-route".to_string(),
        branch: None,
        head_commit: Some(snapshot.base.commit.clone()),
        is_dirty: !snapshot.working_tree.entries.is_empty() || !snapshot.buffers.is_empty(),
        last_snapshot_id: Some(snapshot.snapshot_id.clone()),
        notes: Vec::new(),
        warnings: Vec::new(),
        ignore_settings: hyperindex_protocol::repo::RepoIgnoreSettings::default(),
    }
}

fn symbol_index_state(
    store_dir: &Path,
    repo_id: &str,
    snapshot_id: &str,
) -> Option<IndexedSnapshotState> {
    let store = SymbolStore::open(store_dir, repo_id).ok()?;
    store
        .load_indexed_snapshot_state(snapshot_id)
        .ok()
        .flatten()
}

fn disabled_route_capability(
    route_kind: PlannerRouteKind,
    reason: impl Into<String>,
    supported_filters: PlannerFilterCapabilities,
    constraints: PlannerRouteConstraints,
) -> PlannerRouteCapabilityReport {
    PlannerRouteCapabilityReport {
        route_kind,
        enabled: false,
        available: false,
        readiness: PlannerRouteReadiness::Disabled,
        reason: Some(reason.into()),
        supported_filters,
        constraints,
        diagnostics: Vec::new(),
        notes: Vec::new(),
    }
}

fn degraded_route_capability(
    route_kind: PlannerRouteKind,
    code: impl Into<String>,
    message: impl Into<String>,
    supported_filters: PlannerFilterCapabilities,
    constraints: PlannerRouteConstraints,
    available: bool,
) -> PlannerRouteCapabilityReport {
    let message = message.into();
    PlannerRouteCapabilityReport {
        route_kind,
        enabled: true,
        available,
        readiness: PlannerRouteReadiness::Degraded,
        reason: Some(message.clone()),
        supported_filters,
        constraints,
        diagnostics: vec![planner_diagnostic(
            PlannerDiagnosticSeverity::Warning,
            code,
            message,
        )],
        notes: Vec::new(),
    }
}

fn planner_diagnostic(
    severity: PlannerDiagnosticSeverity,
    code: impl Into<String>,
    message: impl Into<String>,
) -> PlannerDiagnostic {
    PlannerDiagnostic {
        severity,
        code: code.into(),
        message: message.into(),
    }
}

fn map_parse_diagnostic(
    diagnostic: &hyperindex_protocol::symbols::ParseDiagnostic,
) -> PlannerDiagnostic {
    let severity = match diagnostic.severity {
        ParseDiagnosticSeverity::Error => PlannerDiagnosticSeverity::Error,
        ParseDiagnosticSeverity::Warning => PlannerDiagnosticSeverity::Warning,
        ParseDiagnosticSeverity::Info => PlannerDiagnosticSeverity::Info,
    };
    planner_diagnostic(
        severity,
        format!("parse::{:?}", diagnostic.code).to_ascii_lowercase(),
        diagnostic.message.clone(),
    )
}

fn map_semantic_diagnostic(
    diagnostic: &hyperindex_protocol::semantic::SemanticDiagnostic,
) -> PlannerDiagnostic {
    let severity = match diagnostic.severity {
        SemanticDiagnosticSeverity::Warning => PlannerDiagnosticSeverity::Warning,
        SemanticDiagnosticSeverity::Info => PlannerDiagnosticSeverity::Info,
    };
    planner_diagnostic(
        severity,
        diagnostic.code.clone(),
        diagnostic.message.clone(),
    )
}

fn map_impact_diagnostic(
    diagnostic: &hyperindex_protocol::impact::ImpactDiagnostic,
) -> PlannerDiagnostic {
    let severity = match diagnostic.severity {
        ImpactDiagnosticSeverity::Warning => PlannerDiagnosticSeverity::Warning,
        ImpactDiagnosticSeverity::Info => PlannerDiagnosticSeverity::Info,
    };
    planner_diagnostic(
        severity,
        diagnostic.code.clone(),
        diagnostic.message.clone(),
    )
}

fn symbol_filter_capabilities() -> PlannerFilterCapabilities {
    PlannerFilterCapabilities {
        path_globs: true,
        package_names: false,
        package_roots: false,
        workspace_roots: true,
        languages: true,
        extensions: true,
        symbol_kinds: true,
    }
}

fn impact_filter_capabilities() -> PlannerFilterCapabilities {
    PlannerFilterCapabilities {
        path_globs: true,
        package_names: true,
        package_roots: true,
        workspace_roots: true,
        languages: false,
        extensions: true,
        symbol_kinds: false,
    }
}

fn symbol_constraints() -> PlannerRouteConstraints {
    PlannerRouteConstraints {
        emits_engine_local_scores: true,
        returns_file_provenance: true,
        returns_symbol_provenance: true,
        returns_span_provenance: true,
        planner_applies_filters_post_retrieval: true,
        ..PlannerRouteConstraints::default()
    }
}

fn semantic_constraints() -> PlannerRouteConstraints {
    PlannerRouteConstraints {
        emits_engine_local_scores: true,
        returns_file_provenance: true,
        returns_symbol_provenance: true,
        returns_span_provenance: true,
        ..PlannerRouteConstraints::default()
    }
}

fn impact_constraints() -> PlannerRouteConstraints {
    PlannerRouteConstraints {
        requires_unique_target: true,
        emits_engine_local_scores: true,
        returns_file_provenance: true,
        returns_symbol_provenance: true,
        planner_applies_filters_post_retrieval: true,
        ..PlannerRouteConstraints::default()
    }
}

fn path_extension(path: &str) -> Option<String> {
    path.rsplit_once('.')
        .map(|(_, extension)| extension.to_string())
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || value.rsplit_once('.').is_some()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use hyperindex_config::LoadedConfig;
    use hyperindex_protocol::config::RuntimeConfig;
    use hyperindex_protocol::planner::{
        PlannerCapabilitiesParams, PlannerContextRef, PlannerExplainParams, PlannerMode,
        PlannerQueryFilters, PlannerQueryParams, PlannerStatusParams, PlannerUserQuery,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, SnapshotFile, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use hyperindex_repo_store::RepoStore;
    use hyperindex_symbol_store::{IndexedSnapshotState, SymbolStore};
    use hyperindex_symbols::SymbolWorkspace;
    use tempfile::TempDir;

    use super::PlannerService;

    fn test_loaded_config(tempdir: &TempDir) -> LoadedConfig {
        let runtime_root = tempdir.path().join(".hyperindex");
        let mut config = RuntimeConfig::default();
        config.directories.runtime_root = runtime_root.clone();
        config.directories.state_dir = runtime_root.join("state");
        config.directories.data_dir = runtime_root.join("data");
        config.directories.manifests_dir = runtime_root.join("data/manifests");
        config.directories.logs_dir = runtime_root.join("logs");
        config.directories.temp_dir = runtime_root.join("tmp");
        config.transport.socket_path = runtime_root.join("hyperd.sock");
        config.repo_registry.sqlite_path = runtime_root.join("state/runtime.sqlite3");
        config.repo_registry.manifests_dir = runtime_root.join("data/manifests");
        config.parser.artifact_dir = runtime_root.join("data/parse-artifacts");
        config.symbol_index.store_dir = runtime_root.join("data/symbols");
        config.semantic.store_dir = runtime_root.join("data/semantic");
        config.impact.store_dir = runtime_root.join("data/impact");
        for path in [
            config.directories.runtime_root.as_path(),
            config.directories.state_dir.as_path(),
            config.directories.data_dir.as_path(),
            config.directories.manifests_dir.as_path(),
            config.directories.logs_dir.as_path(),
            config.directories.temp_dir.as_path(),
            config.parser.artifact_dir.as_path(),
            config.symbol_index.store_dir.as_path(),
            config.semantic.store_dir.as_path(),
            config.impact.store_dir.as_path(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        LoadedConfig {
            config_path: tempdir.path().join("config.toml"),
            config,
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
                file_count: 3,
                files: vec![
                    SnapshotFile {
                        path: "src/session.ts".to_string(),
                        content_sha256: "sha-session".to_string(),
                        content_bytes: 140,
                        contents: r#"export function invalidateSession(sessionId: string) {
  return `session:${sessionId}`;
}
"#
                        .to_string(),
                    },
                    SnapshotFile {
                        path: "src/usage.ts".to_string(),
                        content_sha256: "sha-usage".to_string(),
                        content_bytes: 140,
                        contents: r#"import { invalidateSession } from "./session";

export function logout(id: string) {
  return invalidateSession(id);
}
"#
                        .to_string(),
                    },
                    SnapshotFile {
                        path: "tests/session.test.ts".to_string(),
                        content_sha256: "sha-test".to_string(),
                        content_bytes: 150,
                        contents: r#"import { logout } from "../src/usage";

test("logout", () => {
  expect(logout("abc")).toContain("session");
});
"#
                        .to_string(),
                    },
                ],
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    fn open_repo_store(config: &RuntimeConfig) -> RepoStore {
        RepoStore::open_from_config(config).unwrap()
    }

    fn persist_snapshot_and_index(loaded: &LoadedConfig, snapshot: &ComposedSnapshot) {
        let repo_store = open_repo_store(&loaded.config);
        repo_store.persist_manifest(snapshot).unwrap();
        let mut workspace = SymbolWorkspace::default();
        let index = workspace.prepare_snapshot(snapshot).unwrap();
        let store =
            SymbolStore::open(&loaded.config.symbol_index.store_dir, &snapshot.repo_id).unwrap();
        store
            .persist_facts(&snapshot.repo_id, &snapshot.snapshot_id, &index.facts)
            .unwrap();
        store
            .record_indexed_snapshot_state(&IndexedSnapshotState {
                repo_id: snapshot.repo_id.clone(),
                snapshot_id: snapshot.snapshot_id.clone(),
                parser_config_digest: "planner-tests".to_string(),
                schema_version: 1,
                indexed_file_count: index.graph.indexed_files,
                refresh_mode: "incremental".to_string(),
            })
            .unwrap();
    }

    fn build_semantic_index(loaded: &LoadedConfig, snapshot: &ComposedSnapshot) {
        persist_snapshot_and_index(loaded, snapshot);
        let repo_store = open_repo_store(&loaded.config);
        crate::semantic::SemanticService
            .build(
                &repo_store,
                &loaded.config.semantic.store_dir,
                &loaded.config.symbol_index.store_dir,
                &loaded.config.semantic,
                snapshot,
                &hyperindex_protocol::semantic::SemanticBuildParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    force: true,
                },
            )
            .unwrap();
    }

    #[test]
    fn planner_service_returns_fused_groups_from_symbol_route() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        persist_snapshot_and_index(&loaded, &snapshot);

        let response = PlannerService
            .query(
                &loaded,
                &snapshot,
                &PlannerQueryParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    query: PlannerUserQuery {
                        text: "invalidateSession".to_string(),
                    },
                    mode_override: Some(PlannerMode::Symbol),
                    selected_context: None,
                    target_context: None,
                    filters: PlannerQueryFilters::default(),
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 8,
                    explain: false,
                    include_trace: true,
                },
            )
            .unwrap();

        assert!(response.trace.is_some());
        assert!(!response.groups.is_empty());
        assert!(response.no_answer.is_none());
        assert!(response.stats.candidates_considered > 0);
        assert!(response.stats.groups_returned > 0);
    }

    #[test]
    fn planner_service_explain_returns_normalized_candidates_from_multiple_engines() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        build_semantic_index(&loaded, &snapshot);

        let response = PlannerService
            .explain(
                &loaded,
                &snapshot,
                &PlannerExplainParams {
                    query: PlannerQueryParams {
                        repo_id: snapshot.repo_id.clone(),
                        snapshot_id: snapshot.snapshot_id.clone(),
                        query: PlannerUserQuery {
                            text: "where is invalidateSession used".to_string(),
                        },
                        mode_override: Some(PlannerMode::Auto),
                        selected_context: None,
                        target_context: None,
                        filters: PlannerQueryFilters::default(),
                        route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                        budgets: None,
                        limit: 8,
                        explain: true,
                        include_trace: true,
                    },
                },
            )
            .unwrap();

        assert!(response.trace.is_some());
        assert!(response.no_answer.is_none());
        assert!(response.candidates.iter().any(|candidate| {
            candidate.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Semantic
        }));
        assert!(response.trace.as_ref().unwrap().routes.iter().any(|trace| {
            trace.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Symbol
                && trace.selected
        }));
        assert!(response.trace.as_ref().unwrap().routes.iter().any(|trace| {
            trace.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Semantic
                && trace.selected
                && matches!(
                    trace.status,
                    hyperindex_protocol::planner::PlannerRouteStatus::Executed
                )
        }));
        assert!(response.candidates.iter().all(|candidate| {
            candidate.route_score.is_some() && !candidate.evidence.is_empty()
        }));
    }

    #[test]
    fn planner_service_reports_readiness_for_real_route_capabilities() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        persist_snapshot_and_index(&loaded, &snapshot);

        let status = PlannerService
            .status(
                &loaded,
                &snapshot,
                &PlannerStatusParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                },
            )
            .unwrap();
        let capabilities = PlannerService
            .capabilities(
                &loaded,
                &snapshot,
                &PlannerCapabilitiesParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                },
            )
            .unwrap();

        let exact = capabilities
            .capabilities
            .routes
            .iter()
            .find(|route| route.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Exact)
            .unwrap();
        let symbol = capabilities
            .capabilities
            .routes
            .iter()
            .find(|route| {
                route.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Symbol
            })
            .unwrap();
        let semantic = capabilities
            .capabilities
            .routes
            .iter()
            .find(|route| {
                route.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Semantic
            })
            .unwrap();
        let impact = capabilities
            .capabilities
            .routes
            .iter()
            .find(|route| {
                route.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Impact
            })
            .unwrap();

        assert!(!exact.available);
        assert!(symbol.available);
        assert!(!semantic.available);
        assert!(impact.available);
        assert!(
            status
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "semantic_build_missing")
        );
    }

    #[test]
    fn planner_service_handles_impact_target_failures_explicitly() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        persist_snapshot_and_index(&loaded, &snapshot);

        let response = PlannerService
            .explain(
                &loaded,
                &snapshot,
                &PlannerExplainParams {
                    query: PlannerQueryParams {
                        repo_id: snapshot.repo_id.clone(),
                        snapshot_id: snapshot.snapshot_id.clone(),
                        query: PlannerUserQuery {
                            text: "what breaks if I rename this file".to_string(),
                        },
                        mode_override: Some(PlannerMode::Impact),
                        selected_context: None,
                        target_context: None,
                        filters: PlannerQueryFilters::default(),
                        route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                        budgets: None,
                        limit: 8,
                        explain: true,
                        include_trace: true,
                    },
                },
            )
            .unwrap();

        assert!(response.candidates.is_empty());
        assert!(response.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_str(),
                "impact_target_missing"
                    | "impact_target_unresolved"
                    | "impact_target_resolution_failed"
            )
        }));
        assert!(response.trace.unwrap().routes.iter().any(|trace| {
            trace.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Impact
                && trace.status == hyperindex_protocol::planner::PlannerRouteStatus::Executed
                && trace.candidate_count == Some(0)
        }));
    }

    #[test]
    fn planner_service_impact_route_can_resolve_selected_file_context() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        persist_snapshot_and_index(&loaded, &snapshot);

        let response = PlannerService
            .explain(
                &loaded,
                &snapshot,
                &PlannerExplainParams {
                    query: PlannerQueryParams {
                        repo_id: snapshot.repo_id.clone(),
                        snapshot_id: snapshot.snapshot_id.clone(),
                        query: PlannerUserQuery {
                            text: "what breaks if I change this file".to_string(),
                        },
                        mode_override: Some(PlannerMode::Impact),
                        selected_context: Some(PlannerContextRef::File {
                            path: "src/session.ts".to_string(),
                        }),
                        target_context: None,
                        filters: PlannerQueryFilters::default(),
                        route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                        budgets: None,
                        limit: 8,
                        explain: true,
                        include_trace: true,
                    },
                },
            )
            .unwrap();

        assert!(response.candidates.iter().all(|candidate| {
            candidate.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Impact
        }));
        assert!(response.candidates.iter().all(|candidate| {
            candidate
                .evidence
                .iter()
                .all(|evidence| evidence.score.is_some())
        }));
    }

    // ── smoke tests: unified planner through daemon service ──

    /// Smoke: an identifier-like query in auto mode routes through
    /// the symbol engine and returns at least one group with a trace.
    #[test]
    fn smoke_auto_mode_exact_ish_query() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        persist_snapshot_and_index(&loaded, &snapshot);

        let response = PlannerService
            .query(
                &loaded,
                &snapshot,
                &PlannerQueryParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    query: PlannerUserQuery {
                        text: "invalidateSession".to_string(),
                    },
                    mode_override: Some(PlannerMode::Auto),
                    selected_context: None,
                    target_context: None,
                    filters: PlannerQueryFilters::default(),
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 10,
                    explain: false,
                    include_trace: true,
                },
            )
            .unwrap();

        // The identifier "invalidateSession" should route through symbol
        assert!(!response.groups.is_empty(), "auto-mode identifier query returned no groups");
        assert!(response.no_answer.is_none());
        assert!(response.stats.candidates_considered > 0);

        // Trace must be present and show symbol route executed
        let trace = response.trace.as_ref().expect("trace should be present");
        let symbol_trace = trace
            .routes
            .iter()
            .find(|r| r.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Symbol)
            .expect("symbol route should appear in trace");
        assert!(symbol_trace.selected);
        assert_eq!(
            symbol_trace.status,
            hyperindex_protocol::planner::PlannerRouteStatus::Executed,
        );
        assert!(symbol_trace.candidate_count.unwrap_or(0) > 0);

        // Groups should have trust payloads and evidence
        for group in &response.groups {
            assert!(!group.evidence.is_empty(), "group should have evidence");
            assert!(
                !group.explanation.summary.is_empty(),
                "group should have explanation summary"
            );
        }

        // JSON serialization should roundtrip
        let json = serde_json::to_string_pretty(&response).unwrap();
        let decoded: hyperindex_protocol::planner::PlannerQueryResponse =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.stats.groups_returned, response.stats.groups_returned);
    }

    /// Smoke: a natural-language query in auto mode routes through
    /// semantic (and possibly symbol) engines.
    #[test]
    fn smoke_auto_mode_semantic_style_query() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        build_semantic_index(&loaded, &snapshot);

        let response = PlannerService
            .query(
                &loaded,
                &snapshot,
                &PlannerQueryParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    query: PlannerUserQuery {
                        text: "where do we invalidate sessions?".to_string(),
                    },
                    mode_override: Some(PlannerMode::Auto),
                    selected_context: None,
                    target_context: None,
                    filters: PlannerQueryFilters::default(),
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 10,
                    explain: false,
                    include_trace: true,
                },
            )
            .unwrap();

        assert!(!response.groups.is_empty(), "semantic-style query returned no groups");
        assert!(response.no_answer.is_none());

        // Trace must show semantic route was executed
        let trace = response.trace.as_ref().expect("trace should be present");
        let semantic_trace = trace
            .routes
            .iter()
            .find(|r| r.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Semantic)
            .expect("semantic route should appear in trace");
        assert!(semantic_trace.selected);
        assert_eq!(
            semantic_trace.status,
            hyperindex_protocol::planner::PlannerRouteStatus::Executed,
        );

        // Natural language should be detected
        assert!(
            response
                .ir
                .intent_signals
                .contains(&hyperindex_protocol::planner::PlannerIntentSignal::NaturalLanguageQuestion),
            "should detect natural language question signal"
        );

        // JSON serialization roundtrip
        let json = serde_json::to_string_pretty(&response).unwrap();
        let decoded: hyperindex_protocol::planner::PlannerQueryResponse =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.groups.len(), response.groups.len());
    }

    /// Smoke: an impact-style query in auto mode with a selected file
    /// context routes through the impact engine.
    #[test]
    fn smoke_auto_mode_impact_style_query() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        persist_snapshot_and_index(&loaded, &snapshot);

        let response = PlannerService
            .query(
                &loaded,
                &snapshot,
                &PlannerQueryParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    query: PlannerUserQuery {
                        text: "what breaks if I change session.ts".to_string(),
                    },
                    mode_override: Some(PlannerMode::Auto),
                    selected_context: Some(PlannerContextRef::File {
                        path: "src/session.ts".to_string(),
                    }),
                    target_context: None,
                    filters: PlannerQueryFilters::default(),
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 10,
                    explain: false,
                    include_trace: true,
                },
            )
            .unwrap();

        // Impact route should be in the trace
        let trace = response.trace.as_ref().expect("trace should be present");
        let impact_trace = trace
            .routes
            .iter()
            .find(|r| r.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Impact)
            .expect("impact route should appear in trace");
        assert!(impact_trace.selected);
        assert_eq!(
            impact_trace.status,
            hyperindex_protocol::planner::PlannerRouteStatus::Executed,
        );

        // Should detect impact phrase signal
        assert!(
            response
                .ir
                .intent_signals
                .contains(&hyperindex_protocol::planner::PlannerIntentSignal::ImpactPhrase),
            "should detect impact phrase signal"
        );

        // JSON roundtrip
        let json = serde_json::to_string_pretty(&response).unwrap();
        assert!(json.contains("\"include_trace\"") || json.contains("trace"));
    }

    /// North-star demo smoke: the full flow from question to grouped
    /// evidence to impact through the unified planner surface.
    #[test]
    fn smoke_north_star_demo_flow() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        build_semantic_index(&loaded, &snapshot);

        // Step 1: query "where do we invalidate sessions?"
        let query_response = PlannerService
            .query(
                &loaded,
                &snapshot,
                &PlannerQueryParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    query: PlannerUserQuery {
                        text: "where do we invalidate sessions?".to_string(),
                    },
                    mode_override: Some(PlannerMode::Auto),
                    selected_context: None,
                    target_context: None,
                    filters: PlannerQueryFilters::default(),
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 10,
                    explain: false,
                    include_trace: true,
                },
            )
            .unwrap();

        assert!(!query_response.groups.is_empty(), "step 1: query should return groups");
        assert!(query_response.no_answer.is_none(), "step 1: no no_answer expected");

        // Step 2: inspect grouped evidence
        let first_group = &query_response.groups[0];
        assert!(
            !first_group.evidence.is_empty(),
            "step 2: first group should have evidence"
        );
        assert!(
            !first_group.explanation.summary.is_empty(),
            "step 2: first group should have explanation"
        );
        assert!(
            first_group.trust.evidence_count > 0,
            "step 2: trust payload should report evidence count"
        );

        // Step 3: pick a symbol from the result and ask for impact
        // through the same unified planner surface
        let symbol_context = first_group
            .anchor
            .as_ref()
            .map(|anchor| match anchor {
                hyperindex_protocol::planner::PlannerAnchor::Symbol {
                    symbol_id,
                    path,
                    span,
                } => PlannerContextRef::Symbol {
                    symbol_id: symbol_id.clone(),
                    path: path.clone(),
                    span: span.clone(),
                    display_name: Some(first_group.label.clone()),
                },
                hyperindex_protocol::planner::PlannerAnchor::File { path } => {
                    PlannerContextRef::File { path: path.clone() }
                }
                other => PlannerContextRef::File {
                    path: format!("{:?}", other),
                },
            })
            .or_else(|| {
                // Fallback: use a known file from the test corpus
                Some(PlannerContextRef::File {
                    path: "src/session.ts".to_string(),
                })
            });

        let impact_response = PlannerService
            .query(
                &loaded,
                &snapshot,
                &PlannerQueryParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    query: PlannerUserQuery {
                        text: "what breaks if I change this".to_string(),
                    },
                    mode_override: Some(PlannerMode::Impact),
                    selected_context: symbol_context,
                    target_context: None,
                    filters: PlannerQueryFilters::default(),
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 10,
                    explain: false,
                    include_trace: true,
                },
            )
            .unwrap();

        // The impact route should have been executed
        let impact_trace = impact_response
            .trace
            .as_ref()
            .expect("step 3: trace should be present")
            .routes
            .iter()
            .find(|r| r.route_kind == hyperindex_protocol::planner::PlannerRouteKind::Impact)
            .expect("step 3: impact route should appear in trace");
        assert!(impact_trace.selected);
        assert_eq!(
            impact_trace.status,
            hyperindex_protocol::planner::PlannerRouteStatus::Executed,
        );

        // Step 4: inspect the planner explanation payload via explain endpoint
        let explain_response = PlannerService
            .explain(
                &loaded,
                &snapshot,
                &PlannerExplainParams {
                    query: PlannerQueryParams {
                        repo_id: snapshot.repo_id.clone(),
                        snapshot_id: snapshot.snapshot_id.clone(),
                        query: PlannerUserQuery {
                            text: "where do we invalidate sessions?".to_string(),
                        },
                        mode_override: Some(PlannerMode::Auto),
                        selected_context: None,
                        target_context: None,
                        filters: PlannerQueryFilters::default(),
                        route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                        budgets: None,
                        limit: 10,
                        explain: true,
                        include_trace: true,
                    },
                },
            )
            .unwrap();

        // Explain should return candidates, trace, and groups
        assert!(
            explain_response.trace.is_some(),
            "step 4: explain should include trace"
        );
        assert!(
            !explain_response.candidates.is_empty(),
            "step 4: explain should include candidates"
        );
        assert!(
            explain_response.no_answer.is_none(),
            "step 4: explain should not report no_answer"
        );

        // Candidates should have scored evidence
        for candidate in &explain_response.candidates {
            assert!(candidate.route_score.is_some(), "candidate should have route_score");
            assert!(!candidate.evidence.is_empty(), "candidate should have evidence");
        }

        // Explain trace should show planner version
        let explain_trace = explain_response.trace.as_ref().unwrap();
        assert!(
            !explain_trace.planner_version.is_empty(),
            "step 4: planner version should be present"
        );

        // JSON roundtrip for the full explain response
        let explain_json = serde_json::to_string_pretty(&explain_response).unwrap();
        let decoded: hyperindex_protocol::planner::PlannerExplainResponse =
            serde_json::from_str(&explain_json).unwrap();
        assert_eq!(decoded.candidates.len(), explain_response.candidates.len());

        // Verify planner status is queryable
        let status = PlannerService
            .status(
                &loaded,
                &snapshot,
                &PlannerStatusParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                },
            )
            .unwrap();
        assert_eq!(
            status.state,
            hyperindex_protocol::planner::PlannerQueryState::Ready,
        );

        // Verify planner capabilities are queryable
        let capabilities = PlannerService
            .capabilities(
                &loaded,
                &snapshot,
                &PlannerCapabilitiesParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                },
            )
            .unwrap();
        assert!(capabilities.capabilities.query);
        assert!(capabilities.capabilities.explain);
        assert!(capabilities.capabilities.trace);

        // Verify CLI rendering for all response types
        let query_human =
            hyperindex_planner::cli_integration::render_query_response(&query_response, false)
                .unwrap();
        assert!(query_human.contains("planner query"));

        let query_json =
            hyperindex_planner::cli_integration::render_query_response(&query_response, true)
                .unwrap();
        assert!(query_json.contains("\"snapshot_id\""));

        let explain_human =
            hyperindex_planner::cli_integration::render_explain_response(&explain_response, false)
                .unwrap();
        assert!(explain_human.contains("planner explain"));

        let status_human =
            hyperindex_planner::cli_integration::render_status_response(&status, false).unwrap();
        assert!(status_human.contains("planner status"));

        let caps_human =
            hyperindex_planner::cli_integration::render_capabilities_response(&capabilities, false)
                .unwrap();
        assert!(caps_human.contains("planner capabilities"));
    }

    /// Smoke: verify that all planner CLI renderers produce valid JSON
    /// for machine consumption.
    #[test]
    fn smoke_json_output_is_first_class() {
        let tempdir = TempDir::new().unwrap();
        let loaded = test_loaded_config(&tempdir);
        let snapshot = snapshot();
        build_semantic_index(&loaded, &snapshot);

        let query_response = PlannerService
            .query(
                &loaded,
                &snapshot,
                &PlannerQueryParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    query: PlannerUserQuery {
                        text: "invalidateSession".to_string(),
                    },
                    mode_override: None,
                    selected_context: None,
                    target_context: None,
                    filters: PlannerQueryFilters::default(),
                    route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                    budgets: None,
                    limit: 10,
                    explain: false,
                    include_trace: true,
                },
            )
            .unwrap();

        // JSON output must parse back to the same type
        let json_str =
            hyperindex_planner::cli_integration::render_query_response(&query_response, true)
                .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.is_object());
        assert!(parsed["groups"].is_array());
        assert!(parsed["stats"]["groups_returned"].is_number());
        assert!(parsed["trace"].is_object());
        assert!(parsed["ir"]["intent_signals"].is_array());

        // Explain JSON
        let explain_response = PlannerService
            .explain(
                &loaded,
                &snapshot,
                &PlannerExplainParams {
                    query: PlannerQueryParams {
                        repo_id: snapshot.repo_id.clone(),
                        snapshot_id: snapshot.snapshot_id.clone(),
                        query: PlannerUserQuery {
                            text: "invalidateSession".to_string(),
                        },
                        mode_override: None,
                        selected_context: None,
                        target_context: None,
                        filters: PlannerQueryFilters::default(),
                        route_hints: hyperindex_protocol::planner::PlannerRouteHints::default(),
                        budgets: None,
                        limit: 10,
                        explain: true,
                        include_trace: true,
                    },
                },
            )
            .unwrap();

        let explain_json =
            hyperindex_planner::cli_integration::render_explain_response(&explain_response, true)
                .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&explain_json).unwrap();
        assert!(parsed.is_object());
        assert!(parsed["candidates"].is_array());
        assert!(parsed["trace"]["routes"].is_array());

        // Status JSON
        let status = PlannerService
            .status(
                &loaded,
                &snapshot,
                &PlannerStatusParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                },
            )
            .unwrap();

        let status_json =
            hyperindex_planner::cli_integration::render_status_response(&status, true).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&status_json).unwrap();
        assert!(parsed.is_object());
        assert!(parsed["state"].is_string());
        assert!(parsed["capabilities"]["routes"].is_array());

        // Capabilities JSON
        let caps = PlannerService
            .capabilities(
                &loaded,
                &snapshot,
                &PlannerCapabilitiesParams {
                    repo_id: snapshot.repo_id.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                },
            )
            .unwrap();

        let caps_json =
            hyperindex_planner::cli_integration::render_capabilities_response(&caps, true).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&caps_json).unwrap();
        assert!(parsed.is_object());
        assert!(parsed["capabilities"]["modes"].is_array());
        assert!(parsed["budgets"]["route_budgets"].is_array());
    }
}
