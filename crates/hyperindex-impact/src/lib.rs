pub mod common;
pub mod impact_engine;
pub mod impact_enrichment;
pub mod impact_model;
pub mod incremental;
pub mod reason_paths;
pub mod test_ranking;

use hyperindex_protocol::impact::ImpactAnalyzeParams;
use hyperindex_protocol::impact::ImpactAnalyzeResponse;
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_symbols::SymbolGraph;
use thiserror::Error;
use tracing::info;

pub use common::{IMPACT_PHASE, ImpactComponentStatus, ImpactScaffoldReport};
pub use impact_engine::{ImpactEngine, ImpactEnginePlan};
pub use impact_enrichment::{
    ImpactDeferredFeature, ImpactDeferredFeatureKind, ImpactEnrichmentPlan,
    ImpactEnrichmentPlanner, ImpactGraphAudit, ImpactPackageEvidence, ImpactPackageMembership,
    ImpactReverseReference, ImpactTestAssociation, ImpactTestAssociationEvidence,
};
pub use impact_model::{ImpactModel, ImpactModelSeed};
pub use incremental::{
    ImpactFileContribution, ImpactFileSignature, ImpactMaterializedState,
    ImpactRebuildFallbackReason, ImpactRefreshResult, IncrementalImpactBuilder,
    StoredReverseReference,
};
pub use reason_paths::{ReasonPathScaffold, ReasonPathSummary};
pub use test_ranking::{TestRankingCandidate, TestRankingPolicy};

#[derive(Debug, Error)]
pub enum ImpactError {
    #[error("{0} is not implemented in the Phase 5 scaffold")]
    NotImplemented(&'static str),
    #[error("impact target {0} was not found")]
    TargetNotFound(String),
}

pub type ImpactResult<T> = Result<T, ImpactError>;

#[derive(Debug, Clone)]
pub struct ImpactWorkspace {
    model: ImpactModel,
    enrichments: ImpactEnrichmentPlanner,
    engine: ImpactEngine,
    reason_paths: ReasonPathScaffold,
    test_ranking: TestRankingPolicy,
}

impl Default for ImpactWorkspace {
    fn default() -> Self {
        Self {
            model: ImpactModel::default(),
            enrichments: ImpactEnrichmentPlanner::default(),
            engine: ImpactEngine::default(),
            reason_paths: ReasonPathScaffold::default(),
            test_ranking: TestRankingPolicy::default(),
        }
    }
}

impl ImpactWorkspace {
    pub fn scaffold_report(&self) -> ImpactScaffoldReport {
        ImpactScaffoldReport::new(vec![
            self.model.status(),
            self.enrichments.status(),
            self.engine.status(),
            self.reason_paths.status(),
            self.test_ranking.status(),
        ])
    }

    pub fn plan_analysis(
        &self,
        graph: &SymbolGraph,
        params: &ImpactAnalyzeParams,
    ) -> ImpactResult<ImpactEnginePlan> {
        self.plan_analysis_with_snapshot(graph, None, params)
    }

    pub fn build_enrichment(
        &self,
        graph: &SymbolGraph,
        snapshot: Option<&ComposedSnapshot>,
    ) -> ImpactEnrichmentPlan {
        self.enrichments.build(graph, snapshot)
    }

    pub fn analyze_with_enrichment(
        &self,
        graph: &SymbolGraph,
        _snapshot: Option<&ComposedSnapshot>,
        enrichment: &ImpactEnrichmentPlan,
        params: &ImpactAnalyzeParams,
    ) -> ImpactResult<ImpactAnalyzeResponse> {
        let seed = self.model.seed_for(graph, enrichment, params)?;
        let reason_paths = self.reason_paths.summary(params.include_reason_paths);
        let test_candidates = self.test_ranking.rank_candidates(&seed, enrichment);
        Ok(self.engine.analyze_precomputed(
            params,
            &seed,
            enrichment,
            &reason_paths,
            &test_candidates,
        ))
    }

    pub fn plan_analysis_with_snapshot(
        &self,
        graph: &SymbolGraph,
        snapshot: Option<&ComposedSnapshot>,
        params: &ImpactAnalyzeParams,
    ) -> ImpactResult<ImpactEnginePlan> {
        info!(
            repo_id = %params.repo_id,
            snapshot_id = %params.snapshot_id,
            target_kind = ?params.target.target_kind(),
            include_transitive = params.include_transitive,
            symbol_count = graph.symbol_count,
            "planning phase5 impact analysis scaffold"
        );
        let enrichments = self.enrichments.build(graph, snapshot);
        let seed = self.model.seed_for(graph, &enrichments, params)?;
        let reason_paths = self.reason_paths.summary(params.include_reason_paths);
        let test_candidates = self.test_ranking.rank_candidates(&seed, &enrichments);
        Ok(self
            .engine
            .plan(seed, enrichments, reason_paths, test_candidates))
    }

    pub fn analyze(
        &self,
        graph: &SymbolGraph,
        params: &ImpactAnalyzeParams,
    ) -> ImpactResult<ImpactAnalyzeResponse> {
        self.analyze_with_snapshot(graph, None, params)
    }

    pub fn analyze_with_snapshot(
        &self,
        graph: &SymbolGraph,
        snapshot: Option<&ComposedSnapshot>,
        params: &ImpactAnalyzeParams,
    ) -> ImpactResult<ImpactAnalyzeResponse> {
        let plan = self.plan_analysis_with_snapshot(graph, snapshot, params)?;
        Ok(self.engine.analyze(params, &plan))
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::impact::{ImpactAnalyzeParams, ImpactChangeScenario, ImpactTargetRef};
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, SnapshotFile, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use hyperindex_symbols::SymbolWorkspace;

    use super::ImpactWorkspace;

    fn snapshot(files: Vec<(&str, &str)>) -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: "snap-1".to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "abc123".to_string(),
                digest: "base".to_string(),
                file_count: files.len(),
                files: files
                    .into_iter()
                    .map(|(path, contents)| SnapshotFile {
                        path: path.to_string(),
                        content_sha256: format!("sha-{path}"),
                        content_bytes: contents.len(),
                        contents: contents.to_string(),
                    })
                    .collect(),
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    #[test]
    fn scaffold_report_lists_phase5_components() {
        let workspace = ImpactWorkspace::default();
        let report = workspace.scaffold_report();
        assert_eq!(report.phase, "phase5");
        assert_eq!(report.components.len(), 5);
        assert!(
            report
                .components
                .iter()
                .any(|component| component.name == "impact_engine")
        );
    }

    #[test]
    fn plan_analysis_returns_placeholder_plan() {
        let workspace = ImpactWorkspace::default();
        let snapshot = snapshot(vec![(
            "packages/auth/src/session/service.ts",
            r#"
            export function invalidateSession() {
              return 1;
            }
            "#,
        )]);
        let mut symbol_workspace = SymbolWorkspace::default();
        let index = symbol_workspace.prepare_snapshot(&snapshot).unwrap();
        let params = ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-1".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            limit: 20,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        };

        let plan = workspace
            .plan_analysis_with_snapshot(&index.graph, Some(&snapshot), &params)
            .unwrap();

        assert_eq!(
            plan.seed.requested_target.selector_value(),
            params.target.selector_value()
        );
        assert_eq!(plan.enrichment.recomputed_layers, 4);
        assert_eq!(plan.reason_paths.path_count, 0);
        assert!(plan.test_candidates.is_empty());
    }
}
