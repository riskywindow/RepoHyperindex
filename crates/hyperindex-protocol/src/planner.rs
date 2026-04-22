use serde::{Deserialize, Serialize};

use crate::impact::ImpactEntityRef;
use crate::symbols::{LanguageId, SourceSpan, SymbolId, SymbolKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerMode {
    Auto,
    Exact,
    Symbol,
    Semantic,
    Impact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerModeSelectionSource {
    ExplicitOverride,
    Heuristic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerQueryState {
    Disabled,
    Ready,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerRouteKind {
    Exact,
    Symbol,
    Semantic,
    Impact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerQueryStyle {
    ExactLookup,
    SymbolLookup,
    SemanticLookup,
    ImpactAnalysis,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerIntentSignal {
    RegexLike,
    QuotedLiteral,
    PathLike,
    GlobLike,
    IdentifierLike,
    QualifiedSymbolLike,
    NaturalLanguageQuestion,
    MultiTokenQuery,
    ImpactPhrase,
    SelectedSymbolContext,
    SelectedFileContext,
    SelectedSpanContext,
    SelectedPackageContext,
    SelectedWorkspaceContext,
    SelectedImpactContext,
    TargetContextProvided,
    FilterScopeProvided,
    ExplicitModeOverride,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerRouteStatus {
    Planned,
    Skipped,
    Deferred,
    Executed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerRouteSkipReason {
    ExactEngineUnavailable,
    CapabilityDisabled,
    FilteredByMode,
    FilteredByRouteHint,
    ExecutionDeferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerEvidenceKind {
    ExactMatch,
    SymbolHit,
    SemanticHit,
    ImpactHit,
    ContextSeed,
    FilterMatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerTrustTier {
    High,
    Medium,
    Low,
    NeedsReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerNoAnswerReason {
    PlannerDisabled,
    NoRouteAvailable,
    NoCandidateMatched,
    FiltersExcludedAllCandidates,
    ExecutionDeferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerAmbiguityReason {
    MultipleCandidateSeeds,
    MixedRouteSignals,
    MultipleAnchorsRemain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerUserQuery {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "context_kind", rename_all = "snake_case")]
pub enum PlannerContextRef {
    Symbol {
        symbol_id: SymbolId,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
    },
    Span {
        path: String,
        span: SourceSpan,
    },
    File {
        path: String,
    },
    Package {
        package_name: String,
        package_root: String,
    },
    Workspace {
        workspace_root: String,
    },
    Impact {
        entity: ImpactEntityRef,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PlannerQueryFilters {
    #[serde(default)]
    pub path_globs: Vec<String>,
    #[serde(default)]
    pub package_names: Vec<String>,
    #[serde(default)]
    pub package_roots: Vec<String>,
    #[serde(default)]
    pub workspace_roots: Vec<String>,
    #[serde(default)]
    pub languages: Vec<LanguageId>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub symbol_kinds: Vec<SymbolKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PlannerRouteHints {
    #[serde(default)]
    pub preferred_routes: Vec<PlannerRouteKind>,
    #[serde(default)]
    pub disabled_routes: Vec<PlannerRouteKind>,
    #[serde(default)]
    pub require_exact_seed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerRouteBudget {
    pub route_kind: PlannerRouteKind,
    pub max_candidates: u32,
    pub max_groups: u32,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PlannerBudgetHints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_groups: Option<u32>,
    #[serde(default)]
    pub route_budgets: Vec<PlannerRouteBudget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerBudgetPolicy {
    pub total_timeout_ms: u64,
    pub max_groups: u32,
    #[serde(default)]
    pub route_budgets: Vec<PlannerRouteBudget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerQueryParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub query: PlannerUserQuery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_override: Option<PlannerMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_context: Option<PlannerContextRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_context: Option<PlannerContextRef>,
    #[serde(default)]
    pub filters: PlannerQueryFilters,
    #[serde(default)]
    pub route_hints: PlannerRouteHints,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<PlannerBudgetHints>,
    pub limit: u32,
    #[serde(default)]
    pub explain: bool,
    #[serde(default)]
    pub include_trace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerModeDecision {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_mode: Option<PlannerMode>,
    pub selected_mode: PlannerMode,
    pub source: PlannerModeSelectionSource,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerExactMatchStyle {
    Literal,
    Regex,
    Path,
    Glob,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerExactQueryIntent {
    pub normalized_term: String,
    pub match_style: PlannerExactMatchStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerSymbolQueryIntent {
    pub normalized_symbol: String,
    #[serde(default)]
    pub segments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerSemanticQueryIntent {
    pub normalized_text: String,
    #[serde(default)]
    pub tokens: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerImpactQueryIntent {
    pub normalized_text: String,
    #[serde(default)]
    pub action_terms: Vec<String>,
    #[serde(default)]
    pub subject_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerQueryIr {
    pub repo_id: String,
    pub snapshot_id: String,
    pub surface_query: String,
    pub normalized_query: String,
    pub selected_mode: PlannerMode,
    pub primary_style: PlannerQueryStyle,
    #[serde(default)]
    pub candidate_styles: Vec<PlannerQueryStyle>,
    #[serde(default)]
    pub planned_routes: Vec<PlannerRouteKind>,
    #[serde(default)]
    pub intent_signals: Vec<PlannerIntentSignal>,
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_context: Option<PlannerContextRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_context: Option<PlannerContextRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_query: Option<PlannerExactQueryIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_query: Option<PlannerSymbolQueryIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_query: Option<PlannerSemanticQueryIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact_query: Option<PlannerImpactQueryIntent>,
    pub filters: PlannerQueryFilters,
    pub route_hints: PlannerRouteHints,
    pub budgets: PlannerBudgetPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "anchor_kind", rename_all = "snake_case")]
pub enum PlannerAnchor {
    Symbol {
        symbol_id: SymbolId,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
    Span {
        path: String,
        span: SourceSpan,
    },
    Impact {
        entity: ImpactEntityRef,
    },
    File {
        path: String,
    },
    Package {
        package_name: String,
        package_root: String,
    },
    Workspace {
        workspace_root: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerEvidenceItem {
    pub evidence_kind: PlannerEvidenceKind,
    pub route_kind: PlannerRouteKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<SymbolId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact_entity: Option<ImpactEntityRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<u32>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerCandidate {
    pub candidate_id: String,
    pub route_kind: PlannerRouteKind,
    pub label: String,
    pub anchor: PlannerAnchor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_score: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_score: Option<u32>,
    #[serde(default)]
    pub evidence: Vec<PlannerEvidenceItem>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerExplanationPayload {
    pub template_id: String,
    pub summary: String,
    #[serde(default)]
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerTrustPayload {
    pub tier: PlannerTrustTier,
    pub deterministic: bool,
    pub evidence_count: u32,
    pub route_agreement_count: u32,
    pub template_id: String,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerResultGroup {
    pub group_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<PlannerAnchor>,
    #[serde(default)]
    pub routes: Vec<PlannerRouteKind>,
    pub trust: PlannerTrustPayload,
    pub explanation: PlannerExplanationPayload,
    #[serde(default)]
    pub evidence: Vec<PlannerEvidenceItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerDiagnostic {
    pub severity: PlannerDiagnosticSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerRouteTrace {
    pub route_kind: PlannerRouteKind,
    pub available: bool,
    pub selected: bool,
    pub status: PlannerRouteStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<PlannerRouteSkipReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<PlannerRouteBudget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerTraceStep {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerTrace {
    pub planner_version: String,
    pub selected_mode: PlannerMode,
    #[serde(default)]
    pub steps: Vec<PlannerTraceStep>,
    #[serde(default)]
    pub routes: Vec<PlannerRouteTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerNoAnswer {
    pub reason: PlannerNoAnswerReason,
    #[serde(default)]
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerAmbiguity {
    pub reason: PlannerAmbiguityReason,
    #[serde(default)]
    pub details: Vec<String>,
    #[serde(default)]
    pub candidate_contexts: Vec<PlannerContextRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerQueryStats {
    pub limit_requested: u32,
    pub routes_considered: u32,
    pub routes_available: u32,
    pub candidates_considered: u32,
    pub groups_returned: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerStatusParams {
    pub repo_id: String,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerCapabilitiesParams {
    pub repo_id: String,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerExplainParams {
    #[serde(flatten)]
    pub query: PlannerQueryParams,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerRouteCapability {
    pub route_kind: PlannerRouteKind,
    pub enabled: bool,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerFilterCapabilities {
    pub path_globs: bool,
    pub package_names: bool,
    pub package_roots: bool,
    pub workspace_roots: bool,
    pub languages: bool,
    pub extensions: bool,
    pub symbol_kinds: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerCapabilities {
    pub status: bool,
    pub query: bool,
    pub explain: bool,
    pub trace: bool,
    pub explicit_mode_override: bool,
    #[serde(default)]
    pub modes: Vec<PlannerMode>,
    pub filters: PlannerFilterCapabilities,
    #[serde(default)]
    pub routes: Vec<PlannerRouteCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerStatusResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub state: PlannerQueryState,
    pub capabilities: PlannerCapabilities,
    #[serde(default)]
    pub diagnostics: Vec<PlannerDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerCapabilitiesResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub default_mode: PlannerMode,
    pub default_limit: u32,
    pub max_limit: u32,
    pub budgets: PlannerBudgetPolicy,
    pub capabilities: PlannerCapabilities,
    #[serde(default)]
    pub diagnostics: Vec<PlannerDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerQueryResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub mode: PlannerModeDecision,
    pub ir: PlannerQueryIr,
    #[serde(default)]
    pub groups: Vec<PlannerResultGroup>,
    #[serde(default)]
    pub diagnostics: Vec<PlannerDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<PlannerTrace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_answer: Option<PlannerNoAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambiguity: Option<PlannerAmbiguity>,
    pub stats: PlannerQueryStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerExplainResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub mode: PlannerModeDecision,
    pub ir: PlannerQueryIr,
    #[serde(default)]
    pub candidates: Vec<PlannerCandidate>,
    #[serde(default)]
    pub groups: Vec<PlannerResultGroup>,
    #[serde(default)]
    pub diagnostics: Vec<PlannerDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<PlannerTrace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_answer: Option<PlannerNoAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambiguity: Option<PlannerAmbiguity>,
    pub stats: PlannerQueryStats,
}

#[cfg(test)]
mod tests {
    use super::{
        PlannerAnchor, PlannerBudgetPolicy, PlannerCapabilities, PlannerDiagnostic,
        PlannerDiagnosticSeverity, PlannerEvidenceItem, PlannerEvidenceKind,
        PlannerExactMatchStyle, PlannerExactQueryIntent, PlannerExplainResponse,
        PlannerExplanationPayload, PlannerFilterCapabilities, PlannerImpactQueryIntent,
        PlannerIntentSignal, PlannerMode, PlannerModeDecision, PlannerModeSelectionSource,
        PlannerNoAnswer, PlannerNoAnswerReason, PlannerQueryFilters, PlannerQueryIr,
        PlannerQueryResponse, PlannerQueryStats, PlannerQueryStyle, PlannerResultGroup,
        PlannerRouteBudget, PlannerRouteCapability, PlannerRouteKind, PlannerRouteStatus,
        PlannerRouteTrace, PlannerSemanticQueryIntent, PlannerTrace, PlannerTraceStep,
        PlannerTrustPayload, PlannerTrustTier,
    };

    #[test]
    fn planner_query_response_roundtrips_cleanly() {
        let original = PlannerQueryResponse {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            mode: PlannerModeDecision {
                requested_mode: Some(PlannerMode::Auto),
                selected_mode: PlannerMode::Semantic,
                source: PlannerModeSelectionSource::Heuristic,
                reasons: vec![
                    "natural-language query routed to semantic-first planning".to_string(),
                ],
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
                    package_names: vec!["@hyperindex/auth".to_string()],
                    package_roots: Vec::new(),
                    workspace_roots: Vec::new(),
                    languages: Vec::new(),
                    extensions: Vec::new(),
                    symbol_kinds: Vec::new(),
                },
                route_hints: super::PlannerRouteHints::default(),
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
            groups: vec![PlannerResultGroup {
                group_id: "group-1".to_string(),
                label: "invalidateSession".to_string(),
                anchor: Some(PlannerAnchor::File {
                    path: "packages/auth/src/session/service.ts".to_string(),
                }),
                routes: vec![PlannerRouteKind::Semantic],
                trust: PlannerTrustPayload {
                    tier: PlannerTrustTier::Medium,
                    deterministic: true,
                    evidence_count: 1,
                    route_agreement_count: 1,
                    template_id: "planner.trust.single_route".to_string(),
                    reasons: vec!["one semantic route supplied grounded evidence".to_string()],
                    warnings: vec![
                        "exact route is still unavailable in the current repo".to_string(),
                    ],
                },
                explanation: PlannerExplanationPayload {
                    template_id: "planner.group.semantic".to_string(),
                    summary: "Semantic retrieval surfaced the session invalidation implementation."
                        .to_string(),
                    details: vec![
                        "The grouped anchor is still evidence-first and machine-readable."
                            .to_string(),
                    ],
                },
                evidence: vec![PlannerEvidenceItem {
                    evidence_kind: PlannerEvidenceKind::SemanticHit,
                    route_kind: PlannerRouteKind::Semantic,
                    label: "semantic chunk".to_string(),
                    path: Some("packages/auth/src/session/service.ts".to_string()),
                    span: None,
                    symbol_id: None,
                    impact_entity: None,
                    snippet: Some("export function invalidateSession(...) { ... }".to_string()),
                    score: Some(940),
                    notes: vec!["semantic score normalized into planner band".to_string()],
                }],
                score: Some(940),
            }],
            diagnostics: vec![PlannerDiagnostic {
                severity: PlannerDiagnosticSeverity::Info,
                code: "planner_contract_fixture".to_string(),
                message: "planner query response fixture is grounded and deterministic".to_string(),
            }],
            trace: Some(PlannerTrace {
                planner_version: "phase7-query-contract-v1".to_string(),
                selected_mode: PlannerMode::Semantic,
                steps: vec![PlannerTraceStep {
                    code: "mode_selected".to_string(),
                    message: "semantic mode selected by heuristic classifier".to_string(),
                }],
                routes: vec![PlannerRouteTrace {
                    route_kind: PlannerRouteKind::Semantic,
                    available: true,
                    selected: true,
                    status: PlannerRouteStatus::Deferred,
                    skip_reason: None,
                    budget: Some(PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Semantic,
                        max_candidates: 16,
                        max_groups: 8,
                        timeout_ms: 400,
                    }),
                    candidate_count: None,
                    group_count: None,
                    elapsed_ms: None,
                    notes: vec!["live route execution is intentionally deferred".to_string()],
                }],
            }),
            no_answer: Some(PlannerNoAnswer {
                reason: PlannerNoAnswerReason::ExecutionDeferred,
                details: vec![
                    "The public contract is implemented before live planning.".to_string(),
                ],
            }),
            ambiguity: None,
            stats: PlannerQueryStats {
                limit_requested: 10,
                routes_considered: 1,
                routes_available: 1,
                candidates_considered: 0,
                groups_returned: 1,
                elapsed_ms: 0,
            },
        };

        let encoded = serde_json::to_string_pretty(&original).unwrap();
        let decoded: PlannerQueryResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn planner_explain_response_roundtrips_cleanly() {
        let original = PlannerExplainResponse {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            mode: PlannerModeDecision {
                requested_mode: Some(PlannerMode::Exact),
                selected_mode: PlannerMode::Exact,
                source: PlannerModeSelectionSource::ExplicitOverride,
                reasons: vec!["exact mode was requested explicitly".to_string()],
            },
            ir: PlannerQueryIr {
                repo_id: "repo-123".to_string(),
                snapshot_id: "snap-123".to_string(),
                surface_query: "packages/auth/src/session/service.ts".to_string(),
                normalized_query: "packages/auth/src/session/service.ts".to_string(),
                selected_mode: PlannerMode::Exact,
                primary_style: PlannerQueryStyle::ExactLookup,
                candidate_styles: vec![PlannerQueryStyle::ExactLookup],
                planned_routes: vec![
                    PlannerRouteKind::Exact,
                    PlannerRouteKind::Symbol,
                    PlannerRouteKind::Semantic,
                ],
                intent_signals: vec![PlannerIntentSignal::PathLike],
                limit: 5,
                selected_context: None,
                target_context: None,
                exact_query: Some(PlannerExactQueryIntent {
                    normalized_term: "packages/auth/src/session/service.ts".to_string(),
                    match_style: PlannerExactMatchStyle::Path,
                }),
                symbol_query: None,
                semantic_query: None,
                impact_query: None,
                filters: PlannerQueryFilters::default(),
                route_hints: super::PlannerRouteHints::default(),
                budgets: PlannerBudgetPolicy {
                    total_timeout_ms: 1_500,
                    max_groups: 10,
                    route_budgets: vec![PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Exact,
                        max_candidates: 25,
                        max_groups: 10,
                        timeout_ms: 150,
                    }],
                },
            },
            candidates: Vec::new(),
            groups: Vec::new(),
            diagnostics: Vec::new(),
            trace: None,
            no_answer: Some(PlannerNoAnswer {
                reason: PlannerNoAnswerReason::ExecutionDeferred,
                details: vec!["Explain data is contract-only in this phase.".to_string()],
            }),
            ambiguity: None,
            stats: PlannerQueryStats {
                limit_requested: 5,
                routes_considered: 1,
                routes_available: 0,
                candidates_considered: 0,
                groups_returned: 0,
                elapsed_ms: 0,
            },
        };

        let encoded = serde_json::to_string_pretty(&original).unwrap();
        let decoded: PlannerExplainResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn planner_capabilities_roundtrip_cleanly() {
        let original = PlannerCapabilities {
            status: true,
            query: true,
            explain: true,
            trace: true,
            explicit_mode_override: true,
            modes: vec![
                PlannerMode::Auto,
                PlannerMode::Exact,
                PlannerMode::Symbol,
                PlannerMode::Semantic,
                PlannerMode::Impact,
            ],
            filters: PlannerFilterCapabilities {
                path_globs: true,
                package_names: true,
                package_roots: true,
                workspace_roots: true,
                languages: true,
                extensions: true,
                symbol_kinds: true,
            },
            routes: vec![
                PlannerRouteCapability {
                    route_kind: PlannerRouteKind::Exact,
                    enabled: true,
                    available: false,
                    reason: Some(
                        "exact route boundary exists but no exact engine ships yet".to_string(),
                    ),
                },
                PlannerRouteCapability {
                    route_kind: PlannerRouteKind::Semantic,
                    enabled: true,
                    available: true,
                    reason: None,
                },
            ],
        };

        let encoded = serde_json::to_string_pretty(&original).unwrap();
        let decoded: PlannerCapabilities = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }
}
