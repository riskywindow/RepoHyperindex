use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::symbols::{SymbolId, SymbolIndexBuildId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct ImpactBuildId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactTargetKind {
    Symbol,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactChangeScenario {
    ModifyBehavior,
    SignatureChange,
    Rename,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactCertaintyTier {
    Certain,
    Likely,
    Possible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactedEntityKind {
    Symbol,
    File,
    Package,
    Test,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactReasonEdgeKind {
    Seed,
    Contains,
    Defines,
    References,
    Imports,
    Exports,
    DeclaredInFile,
    ReferencedByFile,
    ImportedByFile,
    PackageMember,
    TestReference,
    TestAffinity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactGraphEnrichmentKind {
    CanonicalAlias,
    FileAdjacency,
    PackageMembership,
    TestAffinity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactGraphEnrichmentState {
    Available,
    Deferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactMaterializationMode {
    LiveOnly,
    PreferPersisted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactRefreshMode {
    FullRebuild,
    Incremental,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactRefreshTrigger {
    Bootstrap,
    SnapshotDiff,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactStorageFormat {
    Sqlite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactAnalysisState {
    NotReady,
    Ready,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactDiagnosticSeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "target_kind", rename_all = "snake_case")]
pub enum ImpactTargetRef {
    Symbol {
        value: String,
        #[serde(default)]
        symbol_id: Option<SymbolId>,
        #[serde(default)]
        path: Option<String>,
    },
    File {
        path: String,
    },
}

impl ImpactTargetRef {
    pub fn target_kind(&self) -> ImpactTargetKind {
        match self {
            Self::Symbol { .. } => ImpactTargetKind::Symbol,
            Self::File { .. } => ImpactTargetKind::File,
        }
    }

    pub fn selector_value(&self) -> &str {
        match self {
            Self::Symbol { value, .. } => value,
            Self::File { path } => path,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "entity_kind", rename_all = "snake_case")]
pub enum ImpactEntityRef {
    Symbol {
        symbol_id: SymbolId,
        path: String,
        display_name: String,
    },
    File {
        path: String,
    },
    Package {
        package_name: String,
        package_root: String,
    },
    Test {
        path: String,
        display_name: String,
        #[serde(default)]
        symbol_id: Option<SymbolId>,
    },
}

impl ImpactEntityRef {
    pub fn entity_kind(&self) -> ImpactedEntityKind {
        match self {
            Self::Symbol { .. } => ImpactedEntityKind::Symbol,
            Self::File { .. } => ImpactedEntityKind::File,
            Self::Package { .. } => ImpactedEntityKind::Package,
            Self::Test { .. } => ImpactedEntityKind::Test,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactEvidenceEdge {
    pub edge_kind: ImpactReasonEdgeKind,
    pub from: ImpactEntityRef,
    pub to: ImpactEntityRef,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactReasonPath {
    pub summary: String,
    pub edges: Vec<ImpactEvidenceEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactHitExplanation {
    pub why: String,
    pub change_effect: String,
    pub primary_path: ImpactReasonPath,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactHit {
    pub rank: u32,
    pub score: u32,
    pub entity: ImpactEntityRef,
    pub certainty: ImpactCertaintyTier,
    pub primary_reason: ImpactReasonEdgeKind,
    pub depth: u32,
    pub direct: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<ImpactHitExplanation>,
    #[serde(default)]
    pub reason_paths: Vec<ImpactReasonPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactResultGroup {
    pub direct: bool,
    pub certainty: ImpactCertaintyTier,
    pub entity_kind: ImpactedEntityKind,
    pub hit_count: u32,
    pub hits: Vec<ImpactHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactCertaintyCounts {
    pub certain: u32,
    pub likely: u32,
    pub possible: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactSummary {
    pub direct_count: u32,
    pub transitive_count: u32,
    pub certainty_counts: ImpactCertaintyCounts,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactTraversalStats {
    pub nodes_visited: u32,
    pub edges_traversed: u32,
    pub depth_reached: u32,
    pub candidates_considered: u32,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub cutoffs_triggered: Vec<String>,
}

fn impact_traversal_stats_is_default(stats: &ImpactTraversalStats) -> bool {
    stats == &ImpactTraversalStats::default()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactDiagnostic {
    pub severity: ImpactDiagnosticSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactGraphEnrichmentMetadata {
    pub kind: ImpactGraphEnrichmentKind,
    pub state: ImpactGraphEnrichmentState,
    pub evidence_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactStorageMetadata {
    pub format: ImpactStorageFormat,
    pub path: String,
    pub schema_version: u32,
    pub materialization_mode: ImpactMaterializationMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactRefreshStats {
    pub mode: ImpactRefreshMode,
    pub trigger: ImpactRefreshTrigger,
    pub files_touched: u64,
    pub entities_recomputed: u64,
    pub edges_refreshed: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactManifest {
    pub build_id: ImpactBuildId,
    pub repo_id: String,
    pub snapshot_id: String,
    pub symbol_index_build_id: Option<SymbolIndexBuildId>,
    pub created_at: String,
    #[serde(default)]
    pub enrichments: Vec<ImpactGraphEnrichmentMetadata>,
    pub storage: Option<ImpactStorageMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_stats: Option<ImpactRefreshStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub loaded_from_existing_build: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactCapabilities {
    pub status: bool,
    pub analyze: bool,
    pub explain: bool,
    pub materialized_store: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactStatusParams {
    pub repo_id: String,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactStatusResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub state: ImpactAnalysisState,
    pub capabilities: ImpactCapabilities,
    pub supported_targets: Vec<ImpactTargetKind>,
    pub supported_change_scenarios: Vec<ImpactChangeScenario>,
    pub supported_result_kinds: Vec<ImpactedEntityKind>,
    pub certainty_tiers: Vec<ImpactCertaintyTier>,
    pub manifest: Option<ImpactManifest>,
    #[serde(default)]
    pub diagnostics: Vec<ImpactDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactAnalyzeParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub target: ImpactTargetRef,
    pub change_hint: ImpactChangeScenario,
    pub limit: u32,
    pub include_transitive: bool,
    pub include_reason_paths: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_transitive_depth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_nodes_visited: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_edges_traversed: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_candidates_considered: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactAnalyzeResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub target: ImpactTargetRef,
    pub change_hint: ImpactChangeScenario,
    pub summary: ImpactSummary,
    #[serde(default, skip_serializing_if = "impact_traversal_stats_is_default")]
    pub stats: ImpactTraversalStats,
    #[serde(default)]
    pub groups: Vec<ImpactResultGroup>,
    #[serde(default)]
    pub diagnostics: Vec<ImpactDiagnostic>,
    pub manifest: Option<ImpactManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactExplainParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub target: ImpactTargetRef,
    pub change_hint: ImpactChangeScenario,
    pub impacted: ImpactEntityRef,
    pub max_reason_paths: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactExplainResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub target: ImpactTargetRef,
    pub impacted: ImpactEntityRef,
    pub certainty: ImpactCertaintyTier,
    pub direct: bool,
    #[serde(default)]
    pub reason_paths: Vec<ImpactReasonPath>,
    #[serde(default)]
    pub diagnostics: Vec<ImpactDiagnostic>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use crate::symbols::{SymbolId, SymbolIndexBuildId};

    use super::{
        ImpactAnalysisState, ImpactAnalyzeParams, ImpactAnalyzeResponse, ImpactBuildId,
        ImpactCapabilities, ImpactCertaintyCounts, ImpactCertaintyTier, ImpactChangeScenario,
        ImpactDiagnostic, ImpactDiagnosticSeverity, ImpactEntityRef, ImpactEvidenceEdge,
        ImpactExplainParams, ImpactExplainResponse, ImpactGraphEnrichmentKind,
        ImpactGraphEnrichmentMetadata, ImpactGraphEnrichmentState, ImpactHit, ImpactHitExplanation,
        ImpactManifest, ImpactMaterializationMode, ImpactReasonEdgeKind, ImpactReasonPath,
        ImpactRefreshMode, ImpactRefreshStats, ImpactRefreshTrigger, ImpactResultGroup,
        ImpactStatusParams, ImpactStatusResponse, ImpactStorageFormat, ImpactStorageMetadata,
        ImpactSummary, ImpactTargetKind, ImpactTargetRef, ImpactTraversalStats, ImpactedEntityKind,
    };

    fn manifest_fixture() -> ImpactManifest {
        ImpactManifest {
            build_id: ImpactBuildId("impact-build-001".to_string()),
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-1".to_string(),
            symbol_index_build_id: Some(SymbolIndexBuildId("sym-index-build-001".to_string())),
            created_at: "2026-04-09T12:00:00Z".to_string(),
            enrichments: vec![
                ImpactGraphEnrichmentMetadata {
                    kind: ImpactGraphEnrichmentKind::CanonicalAlias,
                    state: ImpactGraphEnrichmentState::Available,
                    evidence_count: Some(18),
                },
                ImpactGraphEnrichmentMetadata {
                    kind: ImpactGraphEnrichmentKind::PackageMembership,
                    state: ImpactGraphEnrichmentState::Deferred,
                    evidence_count: None,
                },
            ],
            storage: Some(ImpactStorageMetadata {
                format: ImpactStorageFormat::Sqlite,
                path: ".hyperindex/data/impact/repo-1/impact.sqlite3".to_string(),
                schema_version: 1,
                materialization_mode: ImpactMaterializationMode::PreferPersisted,
            }),
            refresh_stats: Some(ImpactRefreshStats {
                mode: ImpactRefreshMode::Incremental,
                trigger: ImpactRefreshTrigger::SnapshotDiff,
                files_touched: 2,
                entities_recomputed: 7,
                edges_refreshed: 9,
                elapsed_ms: 4,
            }),
            refresh_mode: Some("incremental".to_string()),
            fallback_reason: None,
            loaded_from_existing_build: false,
        }
    }

    #[test]
    fn impact_params_roundtrip_cleanly() {
        let params = ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-1".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: Some(SymbolId("sym.invalidate_session".to_string())),
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            limit: 20,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: Some(6),
            max_nodes_visited: Some(128),
            max_edges_traversed: Some(512),
            max_candidates_considered: Some(256),
        };

        let encoded = serde_json::to_string_pretty(&params).unwrap();
        let decoded: ImpactAnalyzeParams = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, params);
    }

    #[test]
    fn impact_status_roundtrip_cleanly() {
        let status = ImpactStatusResponse {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-1".to_string(),
            state: ImpactAnalysisState::NotReady,
            capabilities: ImpactCapabilities {
                status: true,
                analyze: false,
                explain: false,
                materialized_store: true,
            },
            supported_targets: vec![ImpactTargetKind::Symbol, ImpactTargetKind::File],
            supported_change_scenarios: vec![
                ImpactChangeScenario::ModifyBehavior,
                ImpactChangeScenario::SignatureChange,
                ImpactChangeScenario::Rename,
                ImpactChangeScenario::Delete,
            ],
            supported_result_kinds: vec![
                ImpactedEntityKind::Symbol,
                ImpactedEntityKind::File,
                ImpactedEntityKind::Package,
                ImpactedEntityKind::Test,
            ],
            certainty_tiers: vec![
                ImpactCertaintyTier::Certain,
                ImpactCertaintyTier::Likely,
                ImpactCertaintyTier::Possible,
            ],
            manifest: Some(manifest_fixture()),
            diagnostics: vec![ImpactDiagnostic {
                severity: ImpactDiagnosticSeverity::Warning,
                code: "impact_scaffold_only".to_string(),
                message: "impact analysis is not implemented yet".to_string(),
            }],
        };

        let encoded = serde_json::to_string_pretty(&status).unwrap();
        let decoded: ImpactStatusResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, status);
    }

    #[test]
    fn impact_explain_roundtrip_cleanly() {
        let explain = ImpactExplainResponse {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-1".to_string(),
            target: ImpactTargetRef::File {
                path: "packages/api/src/routes/logout.ts".to_string(),
            },
            impacted: ImpactEntityRef::Test {
                path: "packages/api/tests/logout-route.test.ts".to_string(),
                display_name: "logout route test".to_string(),
                symbol_id: None,
            },
            certainty: ImpactCertaintyTier::Likely,
            direct: false,
            reason_paths: vec![ImpactReasonPath {
                summary: "route file to covering test affinity".to_string(),
                edges: vec![ImpactEvidenceEdge {
                    edge_kind: ImpactReasonEdgeKind::TestAffinity,
                    from: ImpactEntityRef::File {
                        path: "packages/api/src/routes/logout.ts".to_string(),
                    },
                    to: ImpactEntityRef::Test {
                        path: "packages/api/tests/logout-route.test.ts".to_string(),
                        display_name: "logout route test".to_string(),
                        symbol_id: None,
                    },
                    metadata: BTreeMap::from([(
                        "policy".to_string(),
                        "conservative_file_to_test".to_string(),
                    )]),
                }],
            }],
            diagnostics: Vec::new(),
        };

        let encoded = serde_json::to_string_pretty(&explain).unwrap();
        let decoded: ImpactExplainResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, explain);
    }

    #[test]
    fn analyze_response_serializes_grouped_hits() {
        let response = ImpactAnalyzeResponse {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-1".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: Some(SymbolId("sym.invalidate_session".to_string())),
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            summary: ImpactSummary {
                direct_count: 1,
                transitive_count: 1,
                certainty_counts: ImpactCertaintyCounts {
                    certain: 1,
                    likely: 1,
                    possible: 0,
                },
            },
            stats: ImpactTraversalStats {
                nodes_visited: 4,
                edges_traversed: 6,
                depth_reached: 2,
                candidates_considered: 5,
                elapsed_ms: 3,
                cutoffs_triggered: Vec::new(),
            },
            groups: vec![ImpactResultGroup {
                direct: true,
                certainty: ImpactCertaintyTier::Certain,
                entity_kind: ImpactedEntityKind::Symbol,
                hit_count: 1,
                hits: vec![ImpactHit {
                    rank: 1,
                    score: 100,
                    entity: ImpactEntityRef::Symbol {
                        symbol_id: SymbolId("sym.invalidate_session".to_string()),
                        path: "packages/auth/src/session/service.ts".to_string(),
                        display_name: "invalidateSession".to_string(),
                    },
                    certainty: ImpactCertaintyTier::Certain,
                    primary_reason: ImpactReasonEdgeKind::Seed,
                    depth: 0,
                    direct: true,
                    explanation: Some(ImpactHitExplanation {
                        why: "invalidateSession is the selected impact target".to_string(),
                        change_effect: "modify_behavior starts from the selected target before any deterministic propagation".to_string(),
                        primary_path: ImpactReasonPath {
                            summary: "invalidateSession is the selected impact target".to_string(),
                            edges: vec![ImpactEvidenceEdge {
                                edge_kind: ImpactReasonEdgeKind::Seed,
                                from: ImpactEntityRef::Symbol {
                                    symbol_id: SymbolId("sym.invalidate_session".to_string()),
                                    path: "packages/auth/src/session/service.ts".to_string(),
                                    display_name: "invalidateSession".to_string(),
                                },
                                to: ImpactEntityRef::Symbol {
                                    symbol_id: SymbolId("sym.invalidate_session".to_string()),
                                    path: "packages/auth/src/session/service.ts".to_string(),
                                    display_name: "invalidateSession".to_string(),
                                },
                                metadata: BTreeMap::new(),
                            }],
                        },
                    }),
                    reason_paths: Vec::new(),
                }],
            }],
            diagnostics: Vec::new(),
            manifest: Some(manifest_fixture()),
        };

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["groups"][0]["entity_kind"], json!("symbol"));
        assert_eq!(
            value["groups"][0]["hits"][0]["entity"]["entity_kind"],
            json!("symbol")
        );
    }

    #[test]
    fn target_selector_helpers_report_kind_and_value() {
        let symbol = ImpactTargetRef::Symbol {
            value: "sym.invalidate_session".to_string(),
            symbol_id: None,
            path: None,
        };
        let file = ImpactTargetRef::File {
            path: "packages/api/src/routes/logout.ts".to_string(),
        };

        assert_eq!(symbol.target_kind(), ImpactTargetKind::Symbol);
        assert_eq!(symbol.selector_value(), "sym.invalidate_session");
        assert_eq!(file.target_kind(), ImpactTargetKind::File);
        assert_eq!(file.selector_value(), "packages/api/src/routes/logout.ts");
    }

    #[test]
    fn explain_params_roundtrip_cleanly() {
        let params = ImpactExplainParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-1".to_string(),
            target: ImpactTargetRef::File {
                path: "packages/api/src/routes/logout.ts".to_string(),
            },
            change_hint: ImpactChangeScenario::Rename,
            impacted: ImpactEntityRef::File {
                path: "packages/web/src/auth/logout-client.ts".to_string(),
            },
            max_reason_paths: 4,
        };

        let encoded = serde_json::to_string_pretty(&params).unwrap();
        let decoded: ImpactExplainParams = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, params);
    }

    #[test]
    fn status_params_roundtrip_cleanly() {
        let params = ImpactStatusParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-1".to_string(),
        };

        let encoded = serde_json::to_string_pretty(&params).unwrap();
        let decoded: ImpactStatusParams = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, params);
    }
}
