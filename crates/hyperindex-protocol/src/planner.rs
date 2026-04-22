use serde::{Deserialize, Serialize};

use crate::impact::ImpactEntityRef;
use crate::symbols::{SourceSpan, SymbolId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerIntentKind {
    Lookup,
    Semantic,
    Impact,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerIntentSource {
    ExplicitHint,
    Heuristic,
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
pub enum PlannerRouteStatus {
    Planned,
    Skipped,
    Executed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerSkipReason {
    ExactEngineUnavailable,
    CapabilityDisabled,
    ExecutionDeferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerDiagnosticSeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerQueryText {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerQueryParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub query: PlannerQueryText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_hint: Option<PlannerIntentKind>,
    #[serde(default)]
    pub path_globs: Vec<String>,
    pub limit: u32,
    #[serde(default)]
    pub include_trace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerIntentDecision {
    pub selected_intent: PlannerIntentKind,
    pub source: PlannerIntentSource,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerQueryIr {
    pub repo_id: String,
    pub snapshot_id: String,
    pub surface_query: String,
    pub normalized_query: String,
    pub intent: PlannerIntentKind,
    pub limit: u32,
    #[serde(default)]
    pub path_globs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerRouteBudget {
    pub route_kind: PlannerRouteKind,
    pub max_candidates: u32,
    pub max_groups: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerRouteTrace {
    pub route_kind: PlannerRouteKind,
    pub available: bool,
    pub status: PlannerRouteStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<PlannerSkipReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<PlannerRouteBudget>,
    #[serde(default)]
    pub notes: Vec<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerEvidenceRef {
    pub route_kind: PlannerRouteKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<SymbolId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact_entity: Option<ImpactEntityRef>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerTrustPayload {
    pub evidence_count: u32,
    pub deterministic: bool,
    pub explanation_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerResultGroup {
    pub group_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<PlannerAnchor>,
    pub trust: PlannerTrustPayload,
    #[serde(default)]
    pub evidence: Vec<PlannerEvidenceRef>,
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
pub struct PlannerTrace {
    pub planner_version: String,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub routes: Vec<PlannerRouteTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerQueryStats {
    pub limit_requested: u32,
    pub routes_considered: u32,
    pub routes_available: u32,
    pub groups_returned: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerQueryResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub intent: PlannerIntentDecision,
    pub ir: PlannerQueryIr,
    #[serde(default)]
    pub groups: Vec<PlannerResultGroup>,
    #[serde(default)]
    pub diagnostics: Vec<PlannerDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<PlannerTrace>,
    pub stats: PlannerQueryStats,
}

#[cfg(test)]
mod tests {
    use super::{
        PlannerIntentDecision, PlannerIntentKind, PlannerIntentSource, PlannerQueryIr,
        PlannerQueryResponse, PlannerQueryStats, PlannerRouteKind, PlannerRouteStatus,
        PlannerRouteTrace, PlannerTrace,
    };

    #[test]
    fn planner_response_roundtrips_cleanly() {
        let original = PlannerQueryResponse {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            intent: PlannerIntentDecision {
                selected_intent: PlannerIntentKind::Hybrid,
                source: PlannerIntentSource::Heuristic,
                reasons: vec!["natural-language routing scaffold".to_string()],
            },
            ir: PlannerQueryIr {
                repo_id: "repo-123".to_string(),
                snapshot_id: "snap-123".to_string(),
                surface_query: "where do we invalidate sessions?".to_string(),
                normalized_query: "where do we invalidate sessions?".to_string(),
                intent: PlannerIntentKind::Hybrid,
                limit: 10,
                path_globs: vec!["packages/**".to_string()],
            },
            groups: Vec::new(),
            diagnostics: Vec::new(),
            trace: Some(PlannerTrace {
                planner_version: "phase7-planner-scaffold-v1".to_string(),
                events: vec!["routes_considered=4".to_string()],
                routes: vec![PlannerRouteTrace {
                    route_kind: PlannerRouteKind::Exact,
                    available: false,
                    status: PlannerRouteStatus::Skipped,
                    skip_reason: None,
                    budget: None,
                    notes: vec!["exact_engine_unavailable".to_string()],
                }],
            }),
            stats: PlannerQueryStats {
                limit_requested: 10,
                routes_considered: 4,
                routes_available: 3,
                groups_returned: 0,
                elapsed_ms: 0,
            },
        };

        let encoded = serde_json::to_string_pretty(&original).unwrap();
        let decoded: PlannerQueryResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }
}
