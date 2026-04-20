use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::time::Instant;

use hyperindex_protocol::impact::{
    ImpactAnalyzeParams, ImpactAnalyzeResponse, ImpactCertaintyCounts, ImpactCertaintyTier,
    ImpactChangeScenario, ImpactDiagnostic, ImpactDiagnosticSeverity, ImpactEntityRef,
    ImpactEvidenceEdge, ImpactHit, ImpactHitExplanation, ImpactReasonEdgeKind, ImpactReasonPath,
    ImpactResultGroup, ImpactSummary, ImpactTraversalStats, ImpactedEntityKind,
};
use hyperindex_protocol::symbols::SymbolId;

use crate::common::{ImpactComponentStatus, implemented_status};
use crate::impact_enrichment::{
    ImpactEnrichmentPlan, ImpactPackageMembership, ImpactTestAssociation,
    ImpactTestAssociationEvidence,
};
use crate::impact_model::{ImpactModelSeed, ResolvedImpactTarget};
use crate::test_ranking::TestRankingCandidate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactEnginePlan {
    pub seed: ImpactModelSeed,
    pub enrichment: ImpactEnrichmentPlan,
    pub reason_paths: crate::reason_paths::ReasonPathSummary,
    pub test_candidates: Vec<TestRankingCandidate>,
    pub test_bonus_by_path: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Copy)]
struct ImpactEngineInputs<'a> {
    seed: &'a ImpactModelSeed,
    enrichment: &'a ImpactEnrichmentPlan,
    reason_paths: &'a crate::reason_paths::ReasonPathSummary,
    test_bonus_by_path: &'a BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateHit {
    entity: ImpactEntityRef,
    certainty: ImpactCertaintyTier,
    primary_reason: ImpactReasonEdgeKind,
    depth: u32,
    direct: bool,
    score: u32,
    shortest_reason_path: u32,
    path_relevance: u32,
    test_relevance: u8,
    explanation: ImpactHitExplanation,
    reason_paths: Vec<ImpactReasonPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TraversalNode {
    Symbol(String),
    File(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TraversalFrame {
    node: TraversalNode,
    entity: ImpactEntityRef,
    certainty: ImpactCertaintyTier,
    depth: u32,
    score: u32,
    path_edges: Vec<ImpactEvidenceEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TraversalStep {
    entity: ImpactEntityRef,
    next_node: Option<TraversalNode>,
    certainty: ImpactCertaintyTier,
    reason: ImpactReasonEdgeKind,
    score: u32,
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy)]
struct ScenarioPolicy {
    include_consumer_files: bool,
    include_heuristic_tests: bool,
    max_depth: u32,
    max_nodes_visited: u32,
    max_edges_traversed: u32,
    max_candidates_considered: u32,
}

#[derive(Debug, Default, Clone)]
pub struct ImpactEngine;

impl ImpactEngine {
    pub fn plan(
        &self,
        seed: ImpactModelSeed,
        enrichment: ImpactEnrichmentPlan,
        reason_paths: crate::reason_paths::ReasonPathSummary,
        test_candidates: Vec<TestRankingCandidate>,
    ) -> ImpactEnginePlan {
        let test_bonus_by_path = test_candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate.path.clone(), 20u32.saturating_sub(index as u32)))
            .collect();
        ImpactEnginePlan {
            seed,
            enrichment,
            reason_paths,
            test_candidates,
            test_bonus_by_path,
        }
    }

    pub fn analyze(
        &self,
        params: &ImpactAnalyzeParams,
        plan: &ImpactEnginePlan,
    ) -> ImpactAnalyzeResponse {
        self.analyze_inputs(params, ImpactEngineInputs::from(plan))
    }

    pub fn analyze_precomputed(
        &self,
        params: &ImpactAnalyzeParams,
        seed: &ImpactModelSeed,
        enrichment: &ImpactEnrichmentPlan,
        reason_paths: &crate::reason_paths::ReasonPathSummary,
        test_candidates: &[TestRankingCandidate],
    ) -> ImpactAnalyzeResponse {
        let test_bonus_by_path = test_candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate.path.clone(), 20u32.saturating_sub(index as u32)))
            .collect::<BTreeMap<_, _>>();
        self.analyze_inputs(
            params,
            ImpactEngineInputs {
                seed,
                enrichment,
                reason_paths,
                test_bonus_by_path: &test_bonus_by_path,
            },
        )
    }

    fn analyze_inputs(
        &self,
        params: &ImpactAnalyzeParams,
        plan: ImpactEngineInputs<'_>,
    ) -> ImpactAnalyzeResponse {
        let started_at = Instant::now();
        let policy = scenario_policy(&plan.seed.change_hint, params);
        let seed_entity = seed_entity(plan);
        let seed_frame = seed_frame(plan, &seed_entity);
        let mut candidates = BTreeMap::<String, CandidateHit>::new();
        let mut diagnostics = Vec::new();
        let mut stats = ImpactTraversalStats {
            candidates_considered: 1,
            ..ImpactTraversalStats::default()
        };
        let mut queue = VecDeque::from([seed_frame.clone()]);
        let mut best_frames = BTreeMap::from([(traversal_node_key(&seed_frame.node), seed_frame)]);

        add_candidate(
            &mut candidates,
            candidate_hit(
                plan,
                seed_entity.clone(),
                ImpactCertaintyTier::Certain,
                ImpactReasonEdgeKind::Seed,
                0,
                true,
                1000,
                seed_reason_paths(seed_entity.clone()),
            ),
        );

        let mut cutoff_triggered = false;
        while let Some(frame) = queue.pop_front() {
            let node_key = traversal_node_key(&frame.node);
            let Some(best_frame) = best_frames.get(&node_key) else {
                continue;
            };
            if frame_cmp_key(&frame) != frame_cmp_key(best_frame) {
                continue;
            }
            if stats.nodes_visited >= policy.max_nodes_visited {
                record_cutoff(&mut stats, "nodes_visited");
                break;
            }

            stats.nodes_visited += 1;
            stats.depth_reached = stats.depth_reached.max(frame.depth);

            if frame.depth >= policy.max_depth {
                continue;
            }

            let mut steps = self.expand_node(plan, &frame, &policy);
            steps.sort_by(compare_steps);
            steps.dedup_by(|left, right| step_identity(left) == step_identity(right));

            for step in steps {
                if stats.edges_traversed >= policy.max_edges_traversed {
                    record_cutoff(&mut stats, "edges_traversed");
                    cutoff_triggered = true;
                    break;
                }
                if stats.candidates_considered >= policy.max_candidates_considered {
                    record_cutoff(&mut stats, "candidates_considered");
                    cutoff_triggered = true;
                    break;
                }

                stats.edges_traversed += 1;
                stats.candidates_considered += 1;

                let next_depth = frame.depth + 1;
                let certainty = combine_certainty(&frame.certainty, &step.certainty);
                let score = apply_depth_penalty(step.score, next_depth);
                let path_edges = extend_reason_path(&frame.path_edges, &frame.entity, &step);
                let reason_paths = vec![reason_path_from_edges(&path_edges)];

                if entity_key(&step.entity) != entity_key(&seed_entity) {
                    add_candidate(
                        &mut candidates,
                        candidate_hit(
                            plan,
                            step.entity.clone(),
                            certainty.clone(),
                            step.reason.clone(),
                            next_depth,
                            next_depth <= 1,
                            score,
                            reason_paths,
                        ),
                    );
                }

                stats.depth_reached = stats.depth_reached.max(next_depth);

                let Some(next_node) = step.next_node.clone() else {
                    continue;
                };
                let next_frame = TraversalFrame {
                    node: next_node,
                    entity: step.entity,
                    certainty,
                    depth: next_depth,
                    score,
                    path_edges,
                };
                let next_key = traversal_node_key(&next_frame.node);
                let should_enqueue = best_frames
                    .get(&next_key)
                    .map(|existing| frame_cmp_key(&next_frame) < frame_cmp_key(existing))
                    .unwrap_or(true);
                if should_enqueue {
                    best_frames.insert(next_key, next_frame.clone());
                    queue.push_back(next_frame);
                }
            }

            if cutoff_triggered {
                break;
            }
        }

        for cutoff in &stats.cutoffs_triggered {
            diagnostics.push(ImpactDiagnostic {
                severity: ImpactDiagnosticSeverity::Warning,
                code: format!("impact_cutoff_{cutoff}"),
                message: cutoff_message(cutoff, &policy),
            });
        }
        stats.elapsed_ms = started_at.elapsed().as_millis() as u64;

        let mut hits = candidates.into_values().collect::<Vec<_>>();
        hits.sort_by(compare_candidate_hits);
        if params.limit == 0 {
            hits.clear();
        } else {
            hits.truncate(params.limit as usize);
        }

        let certainty_counts = ImpactCertaintyCounts {
            certain: hits
                .iter()
                .filter(|hit| hit.certainty == ImpactCertaintyTier::Certain)
                .count() as u32,
            likely: hits
                .iter()
                .filter(|hit| hit.certainty == ImpactCertaintyTier::Likely)
                .count() as u32,
            possible: hits
                .iter()
                .filter(|hit| hit.certainty == ImpactCertaintyTier::Possible)
                .count() as u32,
        };
        let direct_count = hits.iter().filter(|hit| hit.direct).count() as u32;
        let transitive_count = hits.iter().filter(|hit| !hit.direct).count() as u32;

        let ranked_hits = hits
            .into_iter()
            .enumerate()
            .map(|(index, hit)| ImpactHit {
                rank: index as u32 + 1,
                score: hit.score,
                entity: hit.entity,
                certainty: hit.certainty,
                primary_reason: hit.primary_reason,
                depth: hit.depth,
                direct: hit.direct,
                explanation: Some(hit.explanation),
                reason_paths: if plan.reason_paths.included {
                    hit.reason_paths
                } else {
                    Vec::new()
                },
            })
            .collect::<Vec<_>>();

        ImpactAnalyzeResponse {
            repo_id: params.repo_id.clone(),
            snapshot_id: params.snapshot_id.clone(),
            target: params.target.clone(),
            change_hint: params.change_hint.clone(),
            summary: ImpactSummary {
                direct_count,
                transitive_count,
                certainty_counts,
            },
            stats,
            groups: group_hits(&ranked_hits),
            diagnostics,
            manifest: None,
        }
    }

    pub fn status(&self) -> ImpactComponentStatus {
        implemented_status(
            "impact_engine",
            "bounded direct and transitive impact traversal is available over explicit graph and enrichment evidence",
        )
    }

    fn expand_node(
        &self,
        plan: ImpactEngineInputs<'_>,
        frame: &TraversalFrame,
        policy: &ScenarioPolicy,
    ) -> Vec<TraversalStep> {
        match &frame.node {
            TraversalNode::Symbol(symbol_id) => {
                self.expand_symbol_node(plan, frame, symbol_id, policy)
            }
            TraversalNode::File(path) => self.expand_file_node(plan, frame, path, policy),
        }
    }

    fn expand_symbol_node(
        &self,
        plan: ImpactEngineInputs<'_>,
        frame: &TraversalFrame,
        symbol_id: &str,
        policy: &ScenarioPolicy,
    ) -> Vec<TraversalStep> {
        let canonical_seed_symbol_id = canonical_symbol_id(plan, symbol_id);
        let symbol = symbol_entity(plan, &canonical_seed_symbol_id);
        let display_name = entity_label(&symbol);
        let mut steps = Vec::new();

        if let Some(path) = plan
            .enrichment
            .symbol_to_file
            .get(&canonical_seed_symbol_id)
        {
            steps.push(TraversalStep {
                entity: file_entity(path),
                next_node: Some(TraversalNode::File(path.clone())),
                certainty: ImpactCertaintyTier::Certain,
                reason: ImpactReasonEdgeKind::DeclaredInFile,
                score: 900,
                metadata: BTreeMap::new(),
            });
        }

        if let Some(package) = plan
            .enrichment
            .package_by_symbol
            .get(&canonical_seed_symbol_id)
        {
            steps.push(TraversalStep {
                entity: package_entity(package),
                next_node: None,
                certainty: ImpactCertaintyTier::Likely,
                reason: ImpactReasonEdgeKind::PackageMember,
                score: 620,
                metadata: BTreeMap::from([(
                    "package_root".to_string(),
                    package.package_root.clone(),
                )]),
            });
        }

        for reverse_reference in plan
            .enrichment
            .reverse_references_by_symbol
            .get(&canonical_seed_symbol_id)
            .into_iter()
            .flatten()
        {
            if let Some(owner_symbol_id) = reverse_reference.owner_symbol_id.as_deref() {
                let owner_canonical = canonical_symbol_id(plan, owner_symbol_id);
                if owner_canonical != canonical_seed_symbol_id {
                    steps.push(TraversalStep {
                        entity: symbol_entity(plan, &owner_canonical),
                        next_node: Some(TraversalNode::Symbol(owner_canonical)),
                        certainty: ImpactCertaintyTier::Certain,
                        reason: ImpactReasonEdgeKind::References,
                        score: 950,
                        metadata: BTreeMap::from([
                            (
                                "occurrence_id".to_string(),
                                reverse_reference.occurrence_id.clone(),
                            ),
                            ("path".to_string(), reverse_reference.path.clone()),
                        ]),
                    });
                }
            }

            if policy.include_consumer_files || reverse_reference.owner_symbol_id.is_none() {
                let certainty = file_certainty_for_symbol_consumers(&plan.seed.change_hint);
                steps.push(TraversalStep {
                    entity: file_entity(&reverse_reference.path),
                    next_node: (!is_test_path(&reverse_reference.path))
                        .then_some(TraversalNode::File(reverse_reference.path.clone())),
                    certainty,
                    reason: ImpactReasonEdgeKind::ReferencedByFile,
                    score: 760,
                    metadata: BTreeMap::from([
                        (
                            "occurrence_id".to_string(),
                            reverse_reference.occurrence_id.clone(),
                        ),
                        ("path".to_string(), reverse_reference.path.clone()),
                    ]),
                });
            }
        }

        for importer_symbol_id in plan
            .enrichment
            .reverse_imports_by_symbol
            .get(&canonical_seed_symbol_id)
            .into_iter()
            .flatten()
        {
            let importer_canonical = canonical_symbol_id(plan, importer_symbol_id);
            steps.push(TraversalStep {
                entity: symbol_entity(plan, &importer_canonical),
                next_node: Some(TraversalNode::Symbol(importer_canonical.clone())),
                certainty: ImpactCertaintyTier::Certain,
                reason: ImpactReasonEdgeKind::Imports,
                score: 930,
                metadata: BTreeMap::new(),
            });

            if policy.include_consumer_files {
                if let Some(path) = plan.enrichment.symbol_to_file.get(&importer_canonical) {
                    steps.push(TraversalStep {
                        entity: file_entity(path),
                        next_node: Some(TraversalNode::File(path.clone())),
                        certainty: file_certainty_for_symbol_consumers(&plan.seed.change_hint),
                        reason: ImpactReasonEdgeKind::ImportedByFile,
                        score: 770,
                        metadata: BTreeMap::from([("path".to_string(), path.clone())]),
                    });
                }
            }
        }

        for exporter_symbol_id in plan
            .enrichment
            .reverse_exports_by_symbol
            .get(&canonical_seed_symbol_id)
            .into_iter()
            .flatten()
        {
            let exporter_canonical = canonical_symbol_id(plan, exporter_symbol_id);
            steps.push(TraversalStep {
                entity: symbol_entity(plan, &exporter_canonical),
                next_node: Some(TraversalNode::Symbol(exporter_canonical)),
                certainty: ImpactCertaintyTier::Certain,
                reason: ImpactReasonEdgeKind::Exports,
                score: 920,
                metadata: BTreeMap::new(),
            });
        }

        self.add_test_steps_for_symbol(
            &mut steps,
            plan,
            frame,
            &canonical_seed_symbol_id,
            &display_name,
            policy,
        );
        filter_self_steps(steps, &frame.entity)
    }

    fn expand_file_node(
        &self,
        plan: ImpactEngineInputs<'_>,
        frame: &TraversalFrame,
        path: &str,
        policy: &ScenarioPolicy,
    ) -> Vec<TraversalStep> {
        let mut steps = Vec::new();

        for symbol_id in plan
            .enrichment
            .file_to_symbols
            .get(path)
            .into_iter()
            .flatten()
        {
            let canonical = canonical_symbol_id(plan, symbol_id);
            steps.push(TraversalStep {
                entity: symbol_entity(plan, &canonical),
                next_node: Some(TraversalNode::Symbol(canonical)),
                certainty: ImpactCertaintyTier::Certain,
                reason: ImpactReasonEdgeKind::Contains,
                score: 900,
                metadata: BTreeMap::new(),
            });
        }

        if let Some(package) = plan.enrichment.package_by_file.get(path) {
            steps.push(TraversalStep {
                entity: package_entity(package),
                next_node: None,
                certainty: ImpactCertaintyTier::Likely,
                reason: ImpactReasonEdgeKind::PackageMember,
                score: 620,
                metadata: BTreeMap::from([(
                    "package_root".to_string(),
                    package.package_root.clone(),
                )]),
            });
        }

        for dependent_path in plan
            .enrichment
            .reverse_dependents_by_file
            .get(path)
            .into_iter()
            .flatten()
        {
            if is_test_path(dependent_path) {
                steps.push(TraversalStep {
                    entity: test_entity(plan, dependent_path, None),
                    next_node: None,
                    certainty: ImpactCertaintyTier::Certain,
                    reason: ImpactReasonEdgeKind::TestReference,
                    score: 880 + test_seed_bonus(plan, dependent_path),
                    metadata: BTreeMap::from([("path".to_string(), dependent_path.clone())]),
                });
                continue;
            }
            steps.push(TraversalStep {
                entity: file_entity(dependent_path),
                next_node: Some(TraversalNode::File(dependent_path.clone())),
                certainty: file_certainty_for_file_consumers(&plan.seed.change_hint),
                reason: ImpactReasonEdgeKind::ImportedByFile,
                score: 860,
                metadata: BTreeMap::from([("path".to_string(), dependent_path.clone())]),
            });
        }

        self.add_test_steps_for_file(&mut steps, plan, frame, path, policy);
        filter_self_steps(steps, &frame.entity)
    }

    fn add_test_steps_for_symbol(
        &self,
        steps: &mut Vec<TraversalStep>,
        plan: ImpactEngineInputs<'_>,
        _frame: &TraversalFrame,
        symbol_id: &str,
        display_name: &str,
        policy: &ScenarioPolicy,
    ) {
        for association in plan
            .enrichment
            .tests_by_symbol
            .get(symbol_id)
            .into_iter()
            .flatten()
        {
            if let Some(step) = test_step(plan, association, policy) {
                steps.push(step);
            }
        }

        if let Some(path) = plan.enrichment.symbol_to_file.get(symbol_id) {
            for association in plan
                .enrichment
                .tests_by_file
                .get(path)
                .into_iter()
                .flatten()
            {
                if let Some(mut step) = test_step(plan, association, policy) {
                    step.metadata
                        .insert("scope".to_string(), display_name.to_string());
                    steps.push(step);
                }
            }
        }
    }

    fn add_test_steps_for_file(
        &self,
        steps: &mut Vec<TraversalStep>,
        plan: ImpactEngineInputs<'_>,
        _frame: &TraversalFrame,
        path: &str,
        policy: &ScenarioPolicy,
    ) {
        for association in plan
            .enrichment
            .tests_by_file
            .get(path)
            .into_iter()
            .flatten()
        {
            if let Some(step) = test_step(plan, association, policy) {
                steps.push(step);
            }
        }
    }
}

fn scenario_policy(
    change_hint: &ImpactChangeScenario,
    params: &ImpactAnalyzeParams,
) -> ScenarioPolicy {
    let mut policy = match change_hint {
        ImpactChangeScenario::ModifyBehavior => ScenarioPolicy {
            include_consumer_files: false,
            include_heuristic_tests: true,
            max_depth: 3,
            max_nodes_visited: 96,
            max_edges_traversed: 256,
            max_candidates_considered: 192,
        },
        ImpactChangeScenario::SignatureChange => ScenarioPolicy {
            include_consumer_files: true,
            include_heuristic_tests: true,
            max_depth: 4,
            max_nodes_visited: 160,
            max_edges_traversed: 512,
            max_candidates_considered: 320,
        },
        ImpactChangeScenario::Rename => ScenarioPolicy {
            include_consumer_files: true,
            include_heuristic_tests: false,
            max_depth: 3,
            max_nodes_visited: 128,
            max_edges_traversed: 384,
            max_candidates_considered: 256,
        },
        ImpactChangeScenario::Delete => ScenarioPolicy {
            include_consumer_files: true,
            include_heuristic_tests: true,
            max_depth: 4,
            max_nodes_visited: 192,
            max_edges_traversed: 640,
            max_candidates_considered: 384,
        },
    };

    if !params.include_transitive {
        policy.max_depth = policy.max_depth.min(1);
    }
    if let Some(value) = params.max_transitive_depth {
        policy.max_depth = policy.max_depth.min(value);
    }
    if let Some(value) = params.max_nodes_visited {
        policy.max_nodes_visited = policy.max_nodes_visited.min(value);
    }
    if let Some(value) = params.max_edges_traversed {
        policy.max_edges_traversed = policy.max_edges_traversed.min(value);
    }
    if let Some(value) = params.max_candidates_considered {
        policy.max_candidates_considered = policy.max_candidates_considered.min(value);
    }

    policy
}

impl<'a> From<&'a ImpactEnginePlan> for ImpactEngineInputs<'a> {
    fn from(plan: &'a ImpactEnginePlan) -> Self {
        Self {
            seed: &plan.seed,
            enrichment: &plan.enrichment,
            reason_paths: &plan.reason_paths,
            test_bonus_by_path: &plan.test_bonus_by_path,
        }
    }
}

fn seed_entity(plan: ImpactEngineInputs<'_>) -> ImpactEntityRef {
    match &plan.seed.resolved_target {
        ResolvedImpactTarget::Symbol(symbol) => symbol_entity(plan, &symbol.canonical_symbol_id),
        ResolvedImpactTarget::File(file) => file_entity(&file.path),
    }
}

fn seed_frame(plan: ImpactEngineInputs<'_>, entity: &ImpactEntityRef) -> TraversalFrame {
    let node = match &plan.seed.resolved_target {
        ResolvedImpactTarget::Symbol(symbol) => {
            TraversalNode::Symbol(symbol.canonical_symbol_id.clone())
        }
        ResolvedImpactTarget::File(file) => TraversalNode::File(file.path.clone()),
    };
    TraversalFrame {
        node,
        entity: entity.clone(),
        certainty: ImpactCertaintyTier::Certain,
        depth: 0,
        score: 1000,
        path_edges: Vec::new(),
    }
}

fn candidate_hit(
    plan: ImpactEngineInputs<'_>,
    entity: ImpactEntityRef,
    certainty: ImpactCertaintyTier,
    primary_reason: ImpactReasonEdgeKind,
    depth: u32,
    direct: bool,
    score: u32,
    reason_paths: Vec<ImpactReasonPath>,
) -> CandidateHit {
    let shortest_reason_path = shortest_reason_path_len(&reason_paths, depth);
    let path_relevance = path_relevance(plan, &entity);
    let test_relevance = test_relevance_rank(&entity, &primary_reason, &reason_paths);
    let explanation = explanation_for_hit(&plan.seed.change_hint, &entity, &reason_paths);

    CandidateHit {
        entity,
        certainty,
        primary_reason,
        depth,
        direct,
        score,
        shortest_reason_path,
        path_relevance,
        test_relevance,
        explanation,
        reason_paths,
    }
}

fn filter_self_steps(steps: Vec<TraversalStep>, current: &ImpactEntityRef) -> Vec<TraversalStep> {
    steps
        .into_iter()
        .filter(|step| entity_key(&step.entity) != entity_key(current))
        .collect()
}

fn shortest_reason_path_len(reason_paths: &[ImpactReasonPath], depth: u32) -> u32 {
    reason_paths
        .iter()
        .map(|path| {
            if path
                .edges
                .first()
                .map(|edge| edge.edge_kind == ImpactReasonEdgeKind::Seed)
                .unwrap_or(false)
            {
                path.edges.len().saturating_sub(1) as u32
            } else {
                path.edges.len() as u32
            }
        })
        .min()
        .unwrap_or(depth)
}

fn path_relevance(plan: ImpactEngineInputs<'_>, entity: &ImpactEntityRef) -> u32 {
    let Some(target_anchor) = target_anchor_path(&plan.seed.resolved_target) else {
        return u32::MAX;
    };
    let Some(entity_anchor) = entity_anchor_path(entity) else {
        return u32::MAX;
    };
    lexical_path_distance(&target_anchor, &entity_anchor)
}

fn target_anchor_path(target: &ResolvedImpactTarget) -> Option<String> {
    match target {
        ResolvedImpactTarget::Symbol(symbol) => Some(symbol.path.clone()),
        ResolvedImpactTarget::File(file) => Some(file.path.clone()),
    }
}

fn entity_anchor_path(entity: &ImpactEntityRef) -> Option<String> {
    match entity {
        ImpactEntityRef::Symbol { path, .. } => Some(path.clone()),
        ImpactEntityRef::File { path } => Some(path.clone()),
        ImpactEntityRef::Package { package_root, .. } => Some(package_root.clone()),
        ImpactEntityRef::Test { path, .. } => Some(path.clone()),
    }
}

fn lexical_path_distance(left: &str, right: &str) -> u32 {
    let left_parts = left.split('/').collect::<Vec<_>>();
    let right_parts = right.split('/').collect::<Vec<_>>();
    let shared_prefix = left_parts
        .iter()
        .zip(right_parts.iter())
        .take_while(|(left, right)| left == right)
        .count();
    (left_parts.len() + right_parts.len() - (shared_prefix * 2)) as u32
}

fn test_relevance_rank(
    entity: &ImpactEntityRef,
    primary_reason: &ImpactReasonEdgeKind,
    reason_paths: &[ImpactReasonPath],
) -> u8 {
    if !matches!(entity, ImpactEntityRef::Test { .. }) {
        return 0;
    }

    match primary_reason {
        ImpactReasonEdgeKind::TestReference => reason_paths
            .first()
            .and_then(|path| path.edges.last())
            .and_then(|edge| edge.metadata.get("evidence"))
            .map(|evidence| match evidence.as_str() {
                "referencessymbol" => 0,
                "importsfile" => 1,
                _ => 2,
            })
            .unwrap_or(1),
        ImpactReasonEdgeKind::TestAffinity => 3,
        _ => 4,
    }
}

fn explanation_for_hit(
    change_hint: &ImpactChangeScenario,
    entity: &ImpactEntityRef,
    reason_paths: &[ImpactReasonPath],
) -> ImpactHitExplanation {
    let primary_path = reason_paths
        .first()
        .cloned()
        .unwrap_or_else(|| ImpactReasonPath {
            summary: format!("{} is the selected impact target", entity_label(entity)),
            edges: Vec::new(),
        });

    ImpactHitExplanation {
        why: render_why(entity, &primary_path),
        change_effect: render_change_effect(change_hint, &primary_path),
        primary_path,
    }
}

fn compare_steps(left: &TraversalStep, right: &TraversalStep) -> Ordering {
    step_cmp_key(left)
        .cmp(&step_cmp_key(right))
        .then_with(|| entity_key(&left.entity).cmp(&entity_key(&right.entity)))
}

fn step_cmp_key(step: &TraversalStep) -> (u8, u8, u8, String, String) {
    (
        certainty_rank(&step.certainty),
        relationship_rank(&step.reason),
        entity_kind_rank(&step.entity.entity_kind()),
        next_node_key(step.next_node.as_ref()),
        metadata_key(&step.metadata),
    )
}

fn step_identity(step: &TraversalStep) -> (String, u8, String, String) {
    (
        entity_key(&step.entity),
        relationship_rank(&step.reason),
        next_node_key(step.next_node.as_ref()),
        metadata_key(&step.metadata),
    )
}

fn next_node_key(node: Option<&TraversalNode>) -> String {
    match node {
        Some(TraversalNode::Symbol(symbol_id)) => format!("symbol:{symbol_id}"),
        Some(TraversalNode::File(path)) => format!("file:{path}"),
        None => String::new(),
    }
}

fn combine_certainty(
    current: &ImpactCertaintyTier,
    next: &ImpactCertaintyTier,
) -> ImpactCertaintyTier {
    if certainty_rank(current) >= certainty_rank(next) {
        current.clone()
    } else {
        next.clone()
    }
}

fn apply_depth_penalty(score: u32, depth: u32) -> u32 {
    score.saturating_sub(depth.saturating_sub(1) * 40)
}

fn extend_reason_path(
    prior_edges: &[ImpactEvidenceEdge],
    from: &ImpactEntityRef,
    step: &TraversalStep,
) -> Vec<ImpactEvidenceEdge> {
    let mut edges = prior_edges.to_vec();
    edges.push(ImpactEvidenceEdge {
        edge_kind: step.reason.clone(),
        from: from.clone(),
        to: step.entity.clone(),
        metadata: step.metadata.clone(),
    });
    edges
}

fn reason_path_from_edges(edges: &[ImpactEvidenceEdge]) -> ImpactReasonPath {
    ImpactReasonPath {
        summary: summarize_reason_path(edges),
        edges: edges.to_vec(),
    }
}

fn summarize_reason_path(edges: &[ImpactEvidenceEdge]) -> String {
    if edges.is_empty() {
        return "selected impact target".to_string();
    }
    let mut parts = Vec::new();
    for (index, edge) in edges.iter().enumerate() {
        if index == 0 {
            parts.push(entity_label(&edge.from));
        }
        parts.push(format!(
            "{} {}",
            reason_edge_label(&edge.edge_kind),
            entity_label(&edge.to)
        ));
    }
    parts.join(" -> ")
}

fn render_why(entity: &ImpactEntityRef, path: &ImpactReasonPath) -> String {
    let Some(edge) = path.edges.last() else {
        return format!("{} is the selected impact target.", entity_label(entity));
    };

    let detail = match edge.edge_kind {
        ImpactReasonEdgeKind::Seed => "it is the selected impact target".to_string(),
        ImpactReasonEdgeKind::Contains => {
            format!("{} contains it", entity_label(&edge.from))
        }
        ImpactReasonEdgeKind::Defines => {
            format!("{} defines it", entity_label(&edge.from))
        }
        ImpactReasonEdgeKind::References => {
            format!("it explicitly references {}", entity_label(&edge.from))
        }
        ImpactReasonEdgeKind::Imports => {
            format!("it imports {}", entity_label(&edge.from))
        }
        ImpactReasonEdgeKind::Exports => {
            format!("it re-exports {}", entity_label(&edge.from))
        }
        ImpactReasonEdgeKind::DeclaredInFile => {
            format!("{} is declared in it", entity_label(&edge.from))
        }
        ImpactReasonEdgeKind::ReferencedByFile => {
            format!(
                "it contains an explicit reference to {}",
                entity_label(&edge.from)
            )
        }
        ImpactReasonEdgeKind::ImportedByFile => {
            format!("it imports or depends on {}", entity_label(&edge.from))
        }
        ImpactReasonEdgeKind::PackageMember => {
            format!("{} belongs to that package", entity_label(&edge.from))
        }
        ImpactReasonEdgeKind::TestReference => edge
            .metadata
            .get("detail")
            .map(|detail| format!("the test has explicit evidence: {detail}"))
            .unwrap_or_else(|| {
                format!(
                    "the test explicitly imports or references {}",
                    entity_label(&edge.from)
                )
            }),
        ImpactReasonEdgeKind::TestAffinity => edge
            .metadata
            .get("detail")
            .map(|detail| format!("the test is conservatively associated by {detail}"))
            .unwrap_or_else(|| {
                format!(
                    "the test is conservatively associated with {}",
                    entity_label(&edge.from)
                )
            }),
    };

    format!(
        "{} is in the blast radius because {}.",
        entity_label(entity),
        detail
    )
}

fn render_change_effect(change_hint: &ImpactChangeScenario, path: &ImpactReasonPath) -> String {
    let edge_kind = path
        .edges
        .last()
        .map(|edge| edge.edge_kind.clone())
        .unwrap_or(ImpactReasonEdgeKind::Seed);

    match change_hint {
        ImpactChangeScenario::ModifyBehavior => match edge_kind {
            ImpactReasonEdgeKind::PackageMember => {
                "modify_behavior keeps package spillover conservative and below explicit consumer evidence".to_string()
            }
            ImpactReasonEdgeKind::TestAffinity => {
                "modify_behavior keeps heuristic tests as conservative fallout rather than direct proof".to_string()
            }
            ImpactReasonEdgeKind::ReferencedByFile | ImpactReasonEdgeKind::ImportedByFile => {
                "modify_behavior treats downstream files as likely only when the graph shows deterministic adjacency".to_string()
            }
            _ => {
                "modify_behavior follows explicit references, imports, exports, ownership, and direct test evidence first".to_string()
            }
        },
        ImpactChangeScenario::SignatureChange => match edge_kind {
            ImpactReasonEdgeKind::ReferencedByFile | ImpactReasonEdgeKind::ImportedByFile => {
                "signature_change promotes direct consumer files because call sites or imports may need edits".to_string()
            }
            ImpactReasonEdgeKind::TestAffinity => {
                "signature_change can elevate deterministic test affinity when explicit file-level adjacency exists".to_string()
            }
            _ => {
                "signature_change prioritizes explicit consumers that can break at use sites or imports".to_string()
            }
        },
        ImpactChangeScenario::Rename => match edge_kind {
            ImpactReasonEdgeKind::TestAffinity => {
                "rename excludes heuristic-only tests from ranked results in this phase".to_string()
            }
            _ => {
                "rename propagates through explicit references, imports, exports, and ownership without heuristic-only test expansion".to_string()
            }
        },
        ImpactChangeScenario::Delete => match edge_kind {
            ImpactReasonEdgeKind::PackageMember | ImpactReasonEdgeKind::TestAffinity => {
                "delete allows broader conservative fallout after explicit consumers have been ranked".to_string()
            }
            _ => {
                "delete keeps explicit consumers certain and then expands conservatively through deterministic adjacency".to_string()
            }
        },
    }
}

fn record_cutoff(stats: &mut ImpactTraversalStats, cutoff: &str) {
    let cutoff = cutoff.to_string();
    if !stats.cutoffs_triggered.contains(&cutoff) {
        stats.cutoffs_triggered.push(cutoff);
    }
}

fn cutoff_message(cutoff: &str, policy: &ScenarioPolicy) -> String {
    match cutoff {
        "nodes_visited" => format!(
            "impact traversal stopped after visiting {} nodes under the current scenario policy",
            policy.max_nodes_visited
        ),
        "edges_traversed" => format!(
            "impact traversal stopped after traversing {} edges under the current scenario policy",
            policy.max_edges_traversed
        ),
        "candidates_considered" => format!(
            "impact traversal stopped after considering {} candidates under the current scenario policy",
            policy.max_candidates_considered
        ),
        _ => "impact traversal stopped at a configured cutoff".to_string(),
    }
}

fn add_candidate(map: &mut BTreeMap<String, CandidateHit>, candidate: CandidateHit) {
    let key = entity_key(&candidate.entity);
    let entry = map.entry(key).or_insert_with(|| candidate.clone());
    let candidate_key = candidate_cmp_key(&candidate);
    let entry_key = candidate_cmp_key(entry);
    if candidate_key < entry_key
        || (candidate_key == entry_key
            && compare_reason_path_sets(&candidate.reason_paths, &entry.reason_paths)
                == Ordering::Less)
    {
        entry.entity = candidate.entity.clone();
        entry.certainty = candidate.certainty.clone();
        entry.primary_reason = candidate.primary_reason.clone();
        entry.depth = candidate.depth;
        entry.direct = candidate.direct;
        entry.score = candidate.score;
        entry.shortest_reason_path = candidate.shortest_reason_path;
        entry.path_relevance = candidate.path_relevance;
        entry.test_relevance = candidate.test_relevance;
        entry.explanation = candidate.explanation.clone();
        entry.reason_paths = candidate.reason_paths.clone();
    } else if entry.reason_paths.is_empty() && !candidate.reason_paths.is_empty() {
        entry.reason_paths = candidate.reason_paths;
        entry.explanation = candidate.explanation;
    }
}

fn compare_reason_path_sets(left: &[ImpactReasonPath], right: &[ImpactReasonPath]) -> Ordering {
    match (left.first(), right.first()) {
        (Some(left), Some(right)) => compare_reason_paths(left, right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_reason_paths(left: &ImpactReasonPath, right: &ImpactReasonPath) -> Ordering {
    left.edges
        .len()
        .cmp(&right.edges.len())
        .then_with(|| reason_path_key(left).cmp(&reason_path_key(right)))
}

fn reason_path_key(path: &ImpactReasonPath) -> String {
    path.edges
        .iter()
        .map(|edge| {
            format!(
                "{}:{}:{}:{}",
                relationship_rank(&edge.edge_kind),
                entity_key(&edge.from),
                entity_key(&edge.to),
                metadata_key(&edge.metadata)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn group_hits(hits: &[ImpactHit]) -> Vec<ImpactResultGroup> {
    let mut groups = Vec::<ImpactResultGroup>::new();
    for hit in hits {
        let Some(group) = groups.last_mut() else {
            groups.push(new_group(hit.clone()));
            continue;
        };
        if group.direct == hit.direct
            && group.certainty == hit.certainty
            && group.entity_kind == hit.entity.entity_kind()
        {
            group.hit_count += 1;
            group.hits.push(hit.clone());
        } else {
            groups.push(new_group(hit.clone()));
        }
    }
    groups
}

fn new_group(hit: ImpactHit) -> ImpactResultGroup {
    ImpactResultGroup {
        direct: hit.direct,
        certainty: hit.certainty.clone(),
        entity_kind: hit.entity.entity_kind(),
        hit_count: 1,
        hits: vec![hit],
    }
}

fn compare_candidate_hits(left: &CandidateHit, right: &CandidateHit) -> Ordering {
    candidate_cmp_key(left)
        .cmp(&candidate_cmp_key(right))
        .then_with(|| entity_key(&left.entity).cmp(&entity_key(&right.entity)))
}

fn candidate_cmp_key(hit: &CandidateHit) -> (u8, u32, u8, u32, u8, u32, u32, u8) {
    (
        certainty_rank(&hit.certainty),
        hit.shortest_reason_path,
        relationship_rank(&hit.primary_reason),
        hit.depth,
        hit.test_relevance,
        hit.path_relevance,
        u32::MAX - hit.score,
        entity_kind_rank(&hit.entity.entity_kind()),
    )
}

fn frame_cmp_key(frame: &TraversalFrame) -> (u8, u32, u32, String) {
    (
        certainty_rank(&frame.certainty),
        frame.depth,
        u32::MAX - frame.score,
        frame
            .path_edges
            .iter()
            .map(|edge| {
                format!(
                    "{}:{}:{}:{}",
                    relationship_rank(&edge.edge_kind),
                    entity_key(&edge.from),
                    entity_key(&edge.to),
                    metadata_key(&edge.metadata)
                )
            })
            .collect::<Vec<_>>()
            .join("|"),
    )
}

fn certainty_rank(certainty: &ImpactCertaintyTier) -> u8 {
    match certainty {
        ImpactCertaintyTier::Certain => 0,
        ImpactCertaintyTier::Likely => 1,
        ImpactCertaintyTier::Possible => 2,
    }
}

fn relationship_rank(reason: &ImpactReasonEdgeKind) -> u8 {
    match reason {
        ImpactReasonEdgeKind::Seed => 0,
        ImpactReasonEdgeKind::References => 1,
        ImpactReasonEdgeKind::Imports => 2,
        ImpactReasonEdgeKind::Exports => 3,
        ImpactReasonEdgeKind::DeclaredInFile => 4,
        ImpactReasonEdgeKind::Contains => 5,
        ImpactReasonEdgeKind::ImportedByFile => 6,
        ImpactReasonEdgeKind::ReferencedByFile => 7,
        ImpactReasonEdgeKind::TestReference => 8,
        ImpactReasonEdgeKind::PackageMember => 9,
        ImpactReasonEdgeKind::TestAffinity => 10,
        ImpactReasonEdgeKind::Defines => 11,
    }
}

fn entity_kind_rank(kind: &ImpactedEntityKind) -> u8 {
    match kind {
        ImpactedEntityKind::Symbol => 0,
        ImpactedEntityKind::File => 1,
        ImpactedEntityKind::Test => 2,
        ImpactedEntityKind::Package => 3,
    }
}

fn metadata_key(metadata: &BTreeMap<String, String>) -> String {
    metadata
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(";")
}

fn seed_reason_paths(entity: ImpactEntityRef) -> Vec<ImpactReasonPath> {
    vec![ImpactReasonPath {
        summary: format!("{} is the selected impact target", entity_label(&entity)),
        edges: vec![ImpactEvidenceEdge {
            edge_kind: ImpactReasonEdgeKind::Seed,
            from: entity.clone(),
            to: entity,
            metadata: BTreeMap::new(),
        }],
    }]
}

fn canonical_symbol_id(plan: ImpactEngineInputs<'_>, symbol_id: &str) -> String {
    plan.enrichment
        .canonical_symbol_by_symbol
        .get(symbol_id)
        .cloned()
        .unwrap_or_else(|| symbol_id.to_string())
}

fn traversal_node_key(node: &TraversalNode) -> String {
    match node {
        TraversalNode::Symbol(symbol_id) => format!("symbol:{symbol_id}"),
        TraversalNode::File(path) => format!("file:{path}"),
    }
}

fn symbol_entity(plan: ImpactEngineInputs<'_>, symbol_id: &str) -> ImpactEntityRef {
    let path = plan
        .enrichment
        .symbol_to_file
        .get(symbol_id)
        .cloned()
        .unwrap_or_default();
    let display_name = plan
        .enrichment
        .symbol_display_name_by_symbol
        .get(symbol_id)
        .cloned()
        .unwrap_or_else(|| symbol_id.to_string());
    ImpactEntityRef::Symbol {
        symbol_id: SymbolId(symbol_id.to_string()),
        path,
        display_name,
    }
}

fn file_entity(path: &str) -> ImpactEntityRef {
    ImpactEntityRef::File {
        path: path.to_string(),
    }
}

fn package_entity(package: &ImpactPackageMembership) -> ImpactEntityRef {
    ImpactEntityRef::Package {
        package_name: package.package_name.clone(),
        package_root: package.package_root.clone(),
    }
}

fn test_entity(
    plan: ImpactEngineInputs<'_>,
    path: &str,
    symbol_id: Option<&str>,
) -> ImpactEntityRef {
    let display_name = symbol_id
        .and_then(|symbol_id| plan.enrichment.symbol_display_name_by_symbol.get(symbol_id))
        .map(|name| format!("{name} test"))
        .unwrap_or_else(|| {
            Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(path)
                .to_string()
        });
    ImpactEntityRef::Test {
        path: path.to_string(),
        display_name,
        symbol_id: symbol_id.map(|symbol_id| SymbolId(symbol_id.to_string())),
    }
}

fn entity_key(entity: &ImpactEntityRef) -> String {
    match entity {
        ImpactEntityRef::Symbol { symbol_id, .. } => format!("symbol:{}", symbol_id.0),
        ImpactEntityRef::File { path } => format!("file:{path}"),
        ImpactEntityRef::Package {
            package_name,
            package_root,
        } => format!("package:{package_root}:{package_name}"),
        ImpactEntityRef::Test {
            path, symbol_id, ..
        } => format!(
            "test:{path}:{}",
            symbol_id
                .as_ref()
                .map(|symbol_id| symbol_id.0.as_str())
                .unwrap_or("")
        ),
    }
}

fn entity_label(entity: &ImpactEntityRef) -> String {
    match entity {
        ImpactEntityRef::Symbol { display_name, .. } => display_name.clone(),
        ImpactEntityRef::File { path } => path.clone(),
        ImpactEntityRef::Package { package_name, .. } => package_name.clone(),
        ImpactEntityRef::Test { display_name, .. } => display_name.clone(),
    }
}

fn reason_edge_label(edge_kind: &ImpactReasonEdgeKind) -> &'static str {
    match edge_kind {
        ImpactReasonEdgeKind::Seed => "seed",
        ImpactReasonEdgeKind::Contains => "contains",
        ImpactReasonEdgeKind::Defines => "defines",
        ImpactReasonEdgeKind::References => "references",
        ImpactReasonEdgeKind::Imports => "imports",
        ImpactReasonEdgeKind::Exports => "exports",
        ImpactReasonEdgeKind::DeclaredInFile => "declared in",
        ImpactReasonEdgeKind::ReferencedByFile => "referenced by file",
        ImpactReasonEdgeKind::ImportedByFile => "imported by file",
        ImpactReasonEdgeKind::PackageMember => "belongs to package",
        ImpactReasonEdgeKind::TestReference => "covered by test",
        ImpactReasonEdgeKind::TestAffinity => "affine to test",
    }
}

fn test_step(
    plan: ImpactEngineInputs<'_>,
    association: &ImpactTestAssociation,
    policy: &ScenarioPolicy,
) -> Option<TraversalStep> {
    if association.evidence == ImpactTestAssociationEvidence::PathHeuristic
        && !policy.include_heuristic_tests
    {
        return None;
    }

    let (certainty, primary_reason, score) = match association.evidence {
        ImpactTestAssociationEvidence::ImportsFile => (
            ImpactCertaintyTier::Certain,
            ImpactReasonEdgeKind::TestReference,
            880,
        ),
        ImpactTestAssociationEvidence::ReferencesSymbol => (
            ImpactCertaintyTier::Certain,
            ImpactReasonEdgeKind::TestReference,
            890,
        ),
        ImpactTestAssociationEvidence::PathHeuristic => (
            heuristic_test_certainty(&plan.seed.change_hint),
            ImpactReasonEdgeKind::TestAffinity,
            420,
        ),
    };

    let mut metadata = BTreeMap::from([
        (
            "evidence".to_string(),
            format!("{:?}", association.evidence).to_ascii_lowercase(),
        ),
        ("detail".to_string(), association.detail.clone()),
        ("test_path".to_string(), association.test_path.clone()),
    ]);
    if let Some(symbol_id) = association.owner_symbol_id.as_deref() {
        metadata.insert("owner_symbol_id".to_string(), symbol_id.to_string());
    }

    Some(TraversalStep {
        entity: test_entity(
            plan,
            &association.test_path,
            association.owner_symbol_id.as_deref(),
        ),
        next_node: None,
        certainty,
        reason: primary_reason,
        score: score + test_seed_bonus(plan, &association.test_path),
        metadata,
    })
}

fn test_seed_bonus(plan: ImpactEngineInputs<'_>, path: &str) -> u32 {
    plan.test_bonus_by_path.get(path).copied().unwrap_or(0)
}

fn heuristic_test_certainty(change_hint: &ImpactChangeScenario) -> ImpactCertaintyTier {
    match change_hint {
        ImpactChangeScenario::SignatureChange => ImpactCertaintyTier::Likely,
        ImpactChangeScenario::ModifyBehavior
        | ImpactChangeScenario::Rename
        | ImpactChangeScenario::Delete => ImpactCertaintyTier::Possible,
    }
}

fn file_certainty_for_symbol_consumers(change_hint: &ImpactChangeScenario) -> ImpactCertaintyTier {
    match change_hint {
        ImpactChangeScenario::ModifyBehavior => ImpactCertaintyTier::Likely,
        ImpactChangeScenario::SignatureChange
        | ImpactChangeScenario::Rename
        | ImpactChangeScenario::Delete => ImpactCertaintyTier::Certain,
    }
}

fn file_certainty_for_file_consumers(change_hint: &ImpactChangeScenario) -> ImpactCertaintyTier {
    match change_hint {
        ImpactChangeScenario::ModifyBehavior => ImpactCertaintyTier::Likely,
        ImpactChangeScenario::SignatureChange
        | ImpactChangeScenario::Rename
        | ImpactChangeScenario::Delete => ImpactCertaintyTier::Certain,
    }
}

fn is_test_path(path: &str) -> bool {
    let lowercase = path.to_ascii_lowercase();
    lowercase.contains("/__tests__/")
        || lowercase.contains("/tests/")
        || lowercase.ends_with(".test.ts")
        || lowercase.ends_with(".test.tsx")
        || lowercase.ends_with(".test.js")
        || lowercase.ends_with(".test.jsx")
        || lowercase.ends_with(".spec.ts")
        || lowercase.ends_with(".spec.tsx")
        || lowercase.ends_with(".spec.js")
        || lowercase.ends_with(".spec.jsx")
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use hyperindex_protocol::impact::{
        ImpactAnalyzeParams, ImpactCertaintyTier, ImpactChangeScenario, ImpactEntityRef,
        ImpactRefreshTrigger, ImpactTargetRef,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, SnapshotFile, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use hyperindex_snapshot::SnapshotAssembler;
    use hyperindex_symbols::SymbolWorkspace;
    use serde_json::json;

    use crate::{ImpactWorkspace, IncrementalImpactBuilder};

    fn snapshot(files: Vec<(String, String)>) -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
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
                        contents,
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

    fn fixture_snapshot() -> ComposedSnapshot {
        snapshot(vec![
            (
                "package.json".to_string(),
                r#"{ "name": "repo-root" }"#.to_string(),
            ),
            (
                "packages/auth/package.json".to_string(),
                r#"{ "name": "@hyperindex/auth" }"#.to_string(),
            ),
            (
                "packages/api/package.json".to_string(),
                r#"{ "name": "@hyperindex/api" }"#.to_string(),
            ),
            (
                "packages/web/package.json".to_string(),
                r#"{ "name": "@hyperindex/web" }"#.to_string(),
            ),
            (
                "packages/auth/src/session/service.ts".to_string(),
                r#"
                export function createSession() {
                  return 1;
                }

                export function invalidateSession() {
                  return createSession();
                }
                "#
                .to_string(),
            ),
            (
                "packages/auth/src/index.ts".to_string(),
                r#"
                export { createSession, invalidateSession } from "./session/service";
                "#
                .to_string(),
            ),
            (
                "packages/api/src/routes/logout.ts".to_string(),
                r#"
                import { invalidateSession } from "@hyperindex/auth";

                export function logout() {
                  return invalidateSession();
                }
                "#
                .to_string(),
            ),
            (
                "packages/api/src/index.ts".to_string(),
                r#"
                export { logout } from "./routes/logout";
                "#
                .to_string(),
            ),
            (
                "packages/web/src/auth/logout-client.ts".to_string(),
                r#"
                import { logout } from "@hyperindex/api";

                export function runLogoutFlow() {
                  return logout();
                }
                "#
                .to_string(),
            ),
            (
                "packages/web/src/auth/logout-audit.ts".to_string(),
                r#"
                import { logout } from "@hyperindex/api";
                import { logout as logoutRoute } from "../../api/src/routes/logout";

                export function auditLogout() {
                  logout();
                  return logoutRoute();
                }
                "#
                .to_string(),
            ),
            (
                "packages/auth/tests/session/service.test.ts".to_string(),
                r#"
                describe("service", () => {
                  it("keeps the stem heuristic deterministic", () => {
                    expect(true).toBe(true);
                  });
                });
                "#
                .to_string(),
            ),
            (
                "packages/auth/tests/session/session-callers.test.ts".to_string(),
                r#"
                import { createSession, invalidateSession } from "../../src/session/service";

                test("session callers", () => {
                  createSession();
                  invalidateSession();
                });
                "#
                .to_string(),
            ),
            (
                "packages/api/tests/logout-route.test.ts".to_string(),
                r#"
                import { logout } from "../src/routes/logout";

                test("logout route", () => {
                  logout();
                });
                "#
                .to_string(),
            ),
            (
                "packages/web/tests/logout-client.test.ts".to_string(),
                r#"
                import { runLogoutFlow } from "../src/auth/logout-client";

                test("logout flow", () => {
                  runLogoutFlow();
                });
                "#
                .to_string(),
            ),
        ])
    }

    fn explosion_snapshot(length: usize) -> ComposedSnapshot {
        let mut files = vec![
            (
                "package.json".to_string(),
                r#"{ "name": "repo-root" }"#.to_string(),
            ),
            (
                "packages/core/package.json".to_string(),
                r#"{ "name": "@hyperindex/core" }"#.to_string(),
            ),
        ];

        files.push((
            "packages/core/src/node0.ts".to_string(),
            r#"
            export function node0() {
              return 0;
            }
            "#
            .to_string(),
        ));

        for index in 1..length {
            files.push((
                format!("packages/core/src/node{index}.ts"),
                format!(
                    r#"
                    import {{ node{} }} from "./node{}";

                    export function node{}() {{
                      return node{}();
                    }}
                    "#,
                    index - 1,
                    index - 1,
                    index,
                    index - 1,
                ),
            ));
        }

        snapshot(files)
    }

    fn ranking_snapshot() -> ComposedSnapshot {
        snapshot(vec![
            (
                "package.json".to_string(),
                r#"{ "name": "repo-root" }"#.to_string(),
            ),
            (
                "packages/auth/package.json".to_string(),
                r#"{ "name": "@hyperindex/auth" }"#.to_string(),
            ),
            (
                "packages/web/package.json".to_string(),
                r#"{ "name": "@hyperindex/web" }"#.to_string(),
            ),
            (
                "packages/auth/src/session/service.ts".to_string(),
                r#"
                export const serviceMarker = 1;

                export function invalidateSession() {
                  return serviceMarker;
                }
                "#
                .to_string(),
            ),
            (
                "packages/auth/src/session/direct-consumer.ts".to_string(),
                r#"
                import { invalidateSession } from "./service";

                export function consumeLocally() {
                  return invalidateSession();
                }
                "#
                .to_string(),
            ),
            (
                "packages/web/src/remote/consumer.ts".to_string(),
                r#"
                import { invalidateSession } from "../../../auth/src/session/service";

                export function consumeRemotely() {
                  return invalidateSession();
                }
                "#
                .to_string(),
            ),
            (
                "packages/auth/tests/session/direct-symbol.test.ts".to_string(),
                r#"
                import { invalidateSession } from "../../src/session/service";

                test("direct symbol", () => {
                  invalidateSession();
                });
                "#
                .to_string(),
            ),
            (
                "packages/auth/tests/session/file-import.test.ts".to_string(),
                r#"
                import { serviceMarker } from "../../src/session/service";

                test("file import", () => {
                  expect(serviceMarker).toBe(1);
                });
                "#
                .to_string(),
            ),
        ])
    }

    fn analyze_with_snapshot(
        snapshot: &ComposedSnapshot,
        params: ImpactAnalyzeParams,
    ) -> hyperindex_protocol::impact::ImpactAnalyzeResponse {
        let mut symbol_workspace = SymbolWorkspace::default();
        let index = symbol_workspace.prepare_snapshot(snapshot).unwrap();
        ImpactWorkspace::default()
            .analyze_with_snapshot(&index.graph, Some(snapshot), &params)
            .unwrap()
    }

    fn analyze(params: ImpactAnalyzeParams) -> hyperindex_protocol::impact::ImpactAnalyzeResponse {
        let snapshot = fixture_snapshot();
        analyze_with_snapshot(&snapshot, params)
    }

    fn profiled_snapshot(length: usize) -> ComposedSnapshot {
        let mut files = vec![
            (
                "package.json".to_string(),
                r#"{ "name": "profiled-root" }"#.to_string(),
            ),
            (
                "packages/core/package.json".to_string(),
                r#"{ "name": "@hyperindex/core" }"#.to_string(),
            ),
            (
                "packages/app/package.json".to_string(),
                r#"{ "name": "@hyperindex/app" }"#.to_string(),
            ),
        ];

        files.push((
            "packages/core/src/node0.ts".to_string(),
            r#"
            export const baseMarker = 1;
            export function node0() {
              return baseMarker;
            }
            "#
            .to_string(),
        ));
        for index in 1..length {
            files.push((
                format!("packages/core/src/node{index}.ts"),
                format!(
                    r#"
                    import {{ node{} }} from "./node{}";
                    export function node{}() {{
                      return node{}();
                    }}
                    "#,
                    index - 1,
                    index - 1,
                    index,
                    index - 1,
                ),
            ));
        }
        for index in 0..length {
            files.push((
                format!("packages/app/src/consumer{index}.ts"),
                format!(
                    r#"
                    import {{ node{index} }} from "../../core/src/node{index}";
                    export function consumer{index}() {{
                      return node{index}();
                    }}
                    "#,
                ),
            ));
        }
        for index in 0..(length / 2).max(1) {
            files.push((
                format!("packages/app/tests/consumer{index}.test.ts"),
                format!(
                    r#"
                    import {{ consumer{index} }} from "../src/consumer{index}";
                    test("consumer{index}", () => {{
                      consumer{index}();
                    }});
                    "#,
                ),
            ));
        }

        snapshot(files)
    }

    fn edited_profiled_snapshot(base: &ComposedSnapshot, target_index: usize) -> ComposedSnapshot {
        let files = base
            .base
            .files
            .iter()
            .map(|file| {
                if file.path == format!("packages/core/src/node{target_index}.ts") {
                    (
                        file.path.clone(),
                        format!(
                            r#"
                            import {{ node{} }} from "./node{}";
                            export function node{}() {{
                              return node{}() + 1;
                            }}
                            "#,
                            target_index.saturating_sub(1),
                            target_index.saturating_sub(1),
                            target_index,
                            target_index.saturating_sub(1),
                        ),
                    )
                } else {
                    (file.path.clone(), file.contents.clone())
                }
            })
            .collect::<Vec<_>>();
        snapshot(files)
    }

    fn all_hits(
        response: &hyperindex_protocol::impact::ImpactAnalyzeResponse,
    ) -> Vec<&hyperindex_protocol::impact::ImpactHit> {
        response
            .groups
            .iter()
            .flat_map(|group| group.hits.iter())
            .collect()
    }

    fn file_hit_count_for_path(
        response: &hyperindex_protocol::impact::ImpactAnalyzeResponse,
        path: &str,
    ) -> usize {
        all_hits(response)
            .into_iter()
            .filter(|hit| matches!(&hit.entity, ImpactEntityRef::File { path: entity_path } if entity_path == path))
            .count()
    }

    fn find_file_hit_by_path<'a>(
        response: &'a hyperindex_protocol::impact::ImpactAnalyzeResponse,
        path: &str,
    ) -> Option<&'a hyperindex_protocol::impact::ImpactHit> {
        all_hits(response)
            .into_iter()
            .find(|hit| matches!(&hit.entity, ImpactEntityRef::File { path: entity_path } if entity_path == path))
    }

    fn find_test_hit_by_path<'a>(
        response: &'a hyperindex_protocol::impact::ImpactAnalyzeResponse,
        path: &str,
    ) -> Option<&'a hyperindex_protocol::impact::ImpactHit> {
        all_hits(response)
            .into_iter()
            .find(|hit| matches!(&hit.entity, ImpactEntityRef::Test { path: entity_path, .. } if entity_path == path))
    }

    fn find_package_hit<'a>(
        response: &'a hyperindex_protocol::impact::ImpactAnalyzeResponse,
        package_root: &str,
        package_name: &str,
    ) -> Option<&'a hyperindex_protocol::impact::ImpactHit> {
        all_hits(response).into_iter().find(|hit| {
            matches!(
                &hit.entity,
                ImpactEntityRef::Package {
                    package_name: name,
                    package_root: root,
                } if name == package_name && root == package_root
            )
        })
    }

    #[test]
    #[ignore = "profiling smoke for Phase 5 hot paths"]
    fn phase5_hot_path_profile_smoke() {
        let snapshot = profiled_snapshot(40);
        let edited = edited_profiled_snapshot(&snapshot, 20);
        let mut symbol_workspace = SymbolWorkspace::default();
        let index = symbol_workspace.prepare_snapshot(&snapshot).unwrap();
        let edited_index = symbol_workspace.prepare_snapshot(&edited).unwrap();
        let workspace = ImpactWorkspace::default();

        let enrichment_build_ms = {
            let started = Instant::now();
            let enrichment = workspace.build_enrichment(&index.graph, Some(&snapshot));
            assert!(!enrichment.reverse_references_by_symbol.is_empty());
            started.elapsed().as_millis()
        };

        let prepared_enrichment = workspace.build_enrichment(&index.graph, Some(&snapshot));

        let direct_params = ImpactAnalyzeParams {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            target: ImpactTargetRef::Symbol {
                value: "packages/core/src/node20.ts#node20".to_string(),
                symbol_id: None,
                path: Some("packages/core/src/node20.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            limit: 20,
            include_transitive: false,
            include_reason_paths: false,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        };
        let transitive_params = ImpactAnalyzeParams {
            include_transitive: true,
            include_reason_paths: false,
            ..direct_params.clone()
        };
        let ranking_params = ImpactAnalyzeParams {
            include_transitive: true,
            include_reason_paths: true,
            limit: 50,
            ..direct_params.clone()
        };

        let direct_impact_200x_ms = {
            let started = Instant::now();
            for _ in 0..200 {
                let response = workspace
                    .analyze_with_enrichment(
                        &index.graph,
                        Some(&snapshot),
                        &prepared_enrichment,
                        &direct_params,
                    )
                    .unwrap();
                assert!(response.summary.direct_count > 0);
            }
            started.elapsed().as_millis()
        };

        let transitive_propagation_100x_ms = {
            let started = Instant::now();
            for _ in 0..100 {
                let response = workspace
                    .analyze_with_enrichment(
                        &index.graph,
                        Some(&snapshot),
                        &prepared_enrichment,
                        &transitive_params,
                    )
                    .unwrap();
                assert!(response.summary.transitive_count > 0);
            }
            started.elapsed().as_millis()
        };

        let ranking_reason_paths_100x_ms = {
            let started = Instant::now();
            for _ in 0..100 {
                let response = workspace
                    .analyze_with_enrichment(
                        &index.graph,
                        Some(&snapshot),
                        &prepared_enrichment,
                        &ranking_params,
                    )
                    .unwrap();
                assert!(
                    response
                        .groups
                        .iter()
                        .flat_map(|group| group.hits.iter())
                        .any(|hit| !hit.reason_paths.is_empty())
                );
            }
            started.elapsed().as_millis()
        };

        let incremental_single_file_update_ms = {
            let config = hyperindex_protocol::config::ImpactConfig::default();
            let builder = IncrementalImpactBuilder::new(&config);
            let base_build = builder.build_full(
                &snapshot,
                &index.graph,
                ImpactRefreshTrigger::Bootstrap,
                None,
            );
            let diff = SnapshotAssembler.diff(&snapshot, &edited);
            let started = Instant::now();
            let refreshed = builder.build_incremental(
                &snapshot,
                &edited,
                &diff,
                &edited_index.graph,
                &base_build.state,
            );
            assert!(refreshed.stats.files_touched > 0);
            started.elapsed().as_millis()
        };

        eprintln!(
            "phase5_hot_path_profile={}",
            serde_json::to_string(&json!({
                "enrichment_build_ms": enrichment_build_ms,
                "direct_impact_200x_ms": direct_impact_200x_ms,
                "transitive_propagation_100x_ms": transitive_propagation_100x_ms,
                "ranking_reason_paths_100x_ms": ranking_reason_paths_100x_ms,
                "incremental_single_file_update_ms": incremental_single_file_update_ms,
                "indexed_files": index.graph.indexed_files,
                "symbols": index.graph.symbol_count,
                "occurrences": index.graph.occurrence_count,
                "edges": index.graph.edge_count,
            }))
            .unwrap()
        );
    }

    fn has_symbol(
        response: &hyperindex_protocol::impact::ImpactAnalyzeResponse,
        path: &str,
        display_name: &str,
    ) -> bool {
        response
            .groups
            .iter()
            .flat_map(|group| group.hits.iter())
            .any(|hit| {
                matches!(
                    &hit.entity,
                    ImpactEntityRef::Symbol {
                        path: symbol_path,
                        display_name: symbol_name,
                        ..
                    } if symbol_path == path && symbol_name == display_name
                )
            })
    }

    fn has_file(response: &hyperindex_protocol::impact::ImpactAnalyzeResponse, path: &str) -> bool {
        response
            .groups
            .iter()
            .flat_map(|group| group.hits.iter())
            .any(|hit| matches!(&hit.entity, ImpactEntityRef::File { path: file_path } if file_path == path))
    }

    fn has_test(response: &hyperindex_protocol::impact::ImpactAnalyzeResponse, path: &str) -> bool {
        response
            .groups
            .iter()
            .flat_map(|group| group.hits.iter())
            .any(|hit| matches!(&hit.entity, ImpactEntityRef::Test { path: test_path, .. } if test_path == path))
    }

    fn has_package(
        response: &hyperindex_protocol::impact::ImpactAnalyzeResponse,
        package_root: &str,
        package_name: &str,
    ) -> bool {
        response
            .groups
            .iter()
            .flat_map(|group| group.hits.iter())
            .any(|hit| {
                matches!(
                    &hit.entity,
                    ImpactEntityRef::Package {
                        package_name: name,
                        package_root: root,
                    } if name == package_name && root == package_root
                )
            })
    }

    #[test]
    fn symbol_target_direct_impact_returns_evidence_backed_hits() {
        let response = analyze(ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            limit: 16,
            include_transitive: false,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        });

        assert!(has_symbol(
            &response,
            "packages/auth/src/session/service.ts",
            "invalidateSession"
        ));
        assert!(has_symbol(
            &response,
            "packages/api/src/routes/logout.ts",
            "logout"
        ));
        assert!(has_file(&response, "packages/auth/src/session/service.ts"));
        assert!(has_test(
            &response,
            "packages/auth/tests/session/session-callers.test.ts"
        ));
        assert!(has_package(&response, "packages/auth", "@hyperindex/auth"));
        assert_eq!(response.summary.transitive_count, 0);
        assert_eq!(response.stats.depth_reached, 1);
    }

    #[test]
    fn transitive_symbol_propagation_reaches_real_bounded_descendants() {
        let response = analyze(ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            limit: 64,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        });

        assert!(has_symbol(
            &response,
            "packages/web/src/auth/logout-client.ts",
            "runLogoutFlow"
        ));
        assert!(has_file(
            &response,
            "packages/web/src/auth/logout-client.ts"
        ));
        assert!(has_test(
            &response,
            "packages/web/tests/logout-client.test.ts"
        ));
        assert!(has_package(&response, "packages/web", "@hyperindex/web"));
        assert!(response.summary.transitive_count > 0);
        assert!(response.stats.nodes_visited > 0);
        assert!(response.stats.edges_traversed > 0);
        assert!(response.stats.depth_reached >= 2);
        assert!(response.stats.candidates_considered >= response.summary.direct_count);
    }

    #[test]
    fn change_scenarios_produce_distinct_transitive_results() {
        let modify_behavior = analyze(ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            limit: 32,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        });
        let rename = analyze(ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::Rename,
            limit: 32,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        });
        let signature_change = analyze(ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::SignatureChange,
            limit: 32,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        });

        assert!(has_test(
            &modify_behavior,
            "packages/auth/tests/session/service.test.ts"
        ));
        assert!(!has_test(
            &rename,
            "packages/auth/tests/session/service.test.ts"
        ));

        let modify_file =
            find_file_hit_by_path(&modify_behavior, "packages/api/src/routes/logout.ts").unwrap();
        let signature_file =
            find_file_hit_by_path(&signature_change, "packages/api/src/routes/logout.ts").unwrap();
        assert!(!modify_file.direct);
        assert!(signature_file.direct);
    }

    #[test]
    fn certainty_tiers_follow_evidence_and_path_type() {
        let modify_behavior = analyze(ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            limit: 32,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        });
        let signature_change = analyze(ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::SignatureChange,
            limit: 32,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        });

        let explicit_test = find_test_hit_by_path(
            &modify_behavior,
            "packages/auth/tests/session/session-callers.test.ts",
        )
        .unwrap();
        let heuristic_test = find_test_hit_by_path(
            &modify_behavior,
            "packages/auth/tests/session/service.test.ts",
        )
        .unwrap();
        let package_hit =
            find_package_hit(&modify_behavior, "packages/auth", "@hyperindex/auth").unwrap();
        let signature_heuristic = find_test_hit_by_path(
            &signature_change,
            "packages/auth/tests/session/service.test.ts",
        )
        .unwrap();

        assert_eq!(explicit_test.certainty, ImpactCertaintyTier::Certain);
        assert_eq!(package_hit.certainty, ImpactCertaintyTier::Likely);
        assert_eq!(heuristic_test.certainty, ImpactCertaintyTier::Possible);
        assert_eq!(signature_heuristic.certainty, ImpactCertaintyTier::Likely);
    }

    #[test]
    fn ranking_prefers_shorter_paths_closer_paths_and_direct_test_evidence() {
        let snapshot = ranking_snapshot();
        let response = analyze_with_snapshot(
            &snapshot,
            ImpactAnalyzeParams {
                repo_id: "repo-1".to_string(),
                snapshot_id: "snap-impact-engine".to_string(),
                target: ImpactTargetRef::Symbol {
                    value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                    symbol_id: None,
                    path: Some("packages/auth/src/session/service.ts".to_string()),
                },
                change_hint: ImpactChangeScenario::Delete,
                limit: 32,
                include_transitive: false,
                include_reason_paths: true,
                max_transitive_depth: None,
                max_nodes_visited: None,
                max_edges_traversed: None,
                max_candidates_considered: None,
            },
        );
        let hits = all_hits(&response);

        let local_consumer_rank = hits
            .iter()
            .find_map(|hit| match &hit.entity {
                ImpactEntityRef::Symbol {
                    path, display_name, ..
                } if path == "packages/auth/src/session/direct-consumer.ts"
                    && display_name == "consumeLocally" =>
                {
                    Some(hit.rank)
                }
                _ => None,
            })
            .unwrap();
        let remote_consumer_rank = hits
            .iter()
            .find_map(|hit| match &hit.entity {
                ImpactEntityRef::Symbol {
                    path, display_name, ..
                } if path == "packages/web/src/remote/consumer.ts"
                    && display_name == "consumeRemotely" =>
                {
                    Some(hit.rank)
                }
                _ => None,
            })
            .unwrap();
        let direct_symbol_test_rank = find_test_hit_by_path(
            &response,
            "packages/auth/tests/session/direct-symbol.test.ts",
        )
        .unwrap()
        .rank;
        let file_import_test_rank =
            find_test_hit_by_path(&response, "packages/auth/tests/session/file-import.test.ts")
                .unwrap()
                .rank;

        assert!(local_consumer_rank < remote_consumer_rank);
        assert!(direct_symbol_test_rank < file_import_test_rank);
    }

    #[test]
    fn explanation_payloads_are_present_even_without_full_reason_paths() {
        let response = analyze(ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::ModifyBehavior,
            limit: 16,
            include_transitive: true,
            include_reason_paths: false,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        });

        for hit in all_hits(&response) {
            let explanation = hit
                .explanation
                .as_ref()
                .expect("every hit should carry an explanation");
            assert!(
                explanation.why.contains("blast radius")
                    || explanation.why.contains("selected impact target")
            );
            assert!(explanation.change_effect.contains("modify_behavior"));
            assert!(!explanation.primary_path.summary.is_empty());
            assert!(!explanation.primary_path.edges.is_empty() || hit.depth == 0);
            assert!(hit.reason_paths.is_empty());
        }
    }

    #[test]
    fn cutoffs_prevent_runaway_traversals() {
        let snapshot = explosion_snapshot(18);
        let response = analyze_with_snapshot(
            &snapshot,
            ImpactAnalyzeParams {
                repo_id: "repo-1".to_string(),
                snapshot_id: "snap-impact-engine".to_string(),
                target: ImpactTargetRef::Symbol {
                    value: "packages/core/src/node0.ts#node0".to_string(),
                    symbol_id: None,
                    path: Some("packages/core/src/node0.ts".to_string()),
                },
                change_hint: ImpactChangeScenario::Delete,
                limit: 50,
                include_transitive: true,
                include_reason_paths: true,
                max_transitive_depth: Some(10),
                max_nodes_visited: Some(3),
                max_edges_traversed: Some(32),
                max_candidates_considered: Some(24),
            },
        );

        assert_eq!(response.stats.nodes_visited, 3);
        assert!(
            response
                .stats
                .cutoffs_triggered
                .contains(&"nodes_visited".to_string())
        );
        assert!(
            response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "impact_cutoff_nodes_visited")
        );
    }

    #[test]
    fn multiple_paths_are_deduplicated_with_stable_reason_paths() {
        let params = ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::Delete,
            limit: 32,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        };

        let left = analyze(params.clone());
        let right = analyze(params);
        let left_hit =
            find_file_hit_by_path(&left, "packages/web/src/auth/logout-audit.ts").unwrap();
        let right_hit =
            find_file_hit_by_path(&right, "packages/web/src/auth/logout-audit.ts").unwrap();

        assert_eq!(
            file_hit_count_for_path(&left, "packages/web/src/auth/logout-audit.ts"),
            1
        );
        assert_eq!(left_hit.reason_paths.len(), 1);
        assert!(left_hit.explanation.is_some());
        assert_eq!(
            left_hit.explanation.as_ref().unwrap().primary_path,
            left_hit.reason_paths[0]
        );
        assert_eq!(left_hit.reason_paths[0], right_hit.reason_paths[0]);
    }

    #[test]
    fn unknown_targets_return_target_not_found() {
        let snapshot = fixture_snapshot();
        let mut symbol_workspace = SymbolWorkspace::default();
        let index = symbol_workspace.prepare_snapshot(&snapshot).unwrap();
        let error = ImpactWorkspace::default()
            .analyze_with_snapshot(
                &index.graph,
                Some(&snapshot),
                &ImpactAnalyzeParams {
                    repo_id: "repo-1".to_string(),
                    snapshot_id: "snap-impact-engine".to_string(),
                    target: ImpactTargetRef::Symbol {
                        value: "packages/auth/src/session/service.ts#missingSymbol".to_string(),
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
                },
            )
            .unwrap_err();

        assert!(matches!(error, crate::ImpactError::TargetNotFound(_)));
    }

    #[test]
    fn result_ordering_is_deterministic() {
        let params = ImpactAnalyzeParams {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-impact-engine".to_string(),
            target: ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: Some("packages/auth/src/session/service.ts".to_string()),
            },
            change_hint: ImpactChangeScenario::Delete,
            limit: 32,
            include_transitive: true,
            include_reason_paths: true,
            max_transitive_depth: None,
            max_nodes_visited: None,
            max_edges_traversed: None,
            max_candidates_considered: None,
        };

        let left = analyze(params.clone());
        let right = analyze(params);
        assert_eq!(
            all_hits(&left)
                .into_iter()
                .map(|hit| {
                    (
                        hit.rank,
                        hit.score,
                        hit.entity.clone(),
                        hit.certainty.clone(),
                        hit.primary_reason.clone(),
                        hit.depth,
                        hit.direct,
                        hit.explanation.clone(),
                        hit.reason_paths.clone(),
                    )
                })
                .collect::<Vec<_>>(),
            all_hits(&right)
                .into_iter()
                .map(|hit| {
                    (
                        hit.rank,
                        hit.score,
                        hit.entity.clone(),
                        hit.certainty.clone(),
                        hit.primary_reason.clone(),
                        hit.depth,
                        hit.direct,
                        hit.explanation.clone(),
                        hit.reason_paths.clone(),
                    )
                })
                .collect::<Vec<_>>()
        );
    }
}
