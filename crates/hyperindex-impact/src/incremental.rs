use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use hyperindex_protocol::config::ImpactConfig;
use hyperindex_protocol::impact::{
    ImpactGraphEnrichmentState, ImpactRefreshMode, ImpactRefreshStats, ImpactRefreshTrigger,
};
use hyperindex_protocol::snapshot::{ComposedSnapshot, SnapshotDiffResponse};
use hyperindex_protocol::symbols::{GraphEdgeKind, GraphNodeRef, OccurrenceRole, SymbolOccurrence};
use hyperindex_symbols::SymbolGraph;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::impact_enrichment::{
    ImpactEnrichmentPlan, ImpactPackageMembership, ImpactReverseReference, ImpactTestAssociation,
    aliases_by_canonical_symbol, audit_graph, canonical_symbol_map, enrichment_metadata,
    is_test_path, node_key, owner_symbol_for_span, package_indexes, sort_and_dedup_map_values,
    source_candidates_by_package_and_stem, source_stem_for_test_path,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpactRebuildFallbackReason {
    NoPriorSnapshot,
    MissingSnapshotDiff,
    SchemaVersionChanged,
    IncompatibleConfigChange,
    CacheOrIndexCorruption,
    UnresolvedConsistencyIssue,
}

impl ImpactRebuildFallbackReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoPriorSnapshot => "no_prior_snapshot",
            Self::MissingSnapshotDiff => "missing_snapshot_diff",
            Self::SchemaVersionChanged => "schema_version_changed",
            Self::IncompatibleConfigChange => "incompatible_config_change",
            Self::CacheOrIndexCorruption => "cache_or_index_corruption",
            Self::UnresolvedConsistencyIssue => "unresolved_consistency_issue",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StoredReverseReference {
    pub occurrence_id: String,
    pub occurrence_key: String,
    pub path: String,
    pub owner_symbol_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactFileSignature {
    pub owned_symbol_ids: Vec<String>,
    pub reference_keys: Vec<String>,
    pub file_dependency_targets: Vec<String>,
    pub symbol_import_targets: Vec<String>,
    pub symbol_export_targets: Vec<String>,
    pub package_name: Option<String>,
    pub package_root: Option<String>,
    pub is_test_file: bool,
    pub heuristic_candidate_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactFileContribution {
    pub path: String,
    pub signature: ImpactFileSignature,
    pub reverse_references_by_symbol: BTreeMap<String, Vec<StoredReverseReference>>,
    pub reverse_reference_files_by_symbol: BTreeMap<String, Vec<String>>,
    pub reverse_imports_by_symbol: BTreeMap<String, Vec<String>>,
    pub reverse_exports_by_symbol: BTreeMap<String, Vec<String>>,
    pub reverse_dependents_by_file: BTreeMap<String, Vec<String>>,
    pub tests_by_file: BTreeMap<String, Vec<ImpactTestAssociation>>,
    pub tests_by_symbol: BTreeMap<String, Vec<ImpactTestAssociation>>,
    pub is_test_file: bool,
    pub entity_count: u64,
    pub edge_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactMaterializedState {
    pub plan: ImpactEnrichmentPlan,
    pub file_contributions: BTreeMap<String, ImpactFileContribution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactRefreshResult {
    pub config_digest: String,
    pub fallback_reason: Option<ImpactRebuildFallbackReason>,
    pub stats: ImpactRefreshStats,
    pub state: ImpactMaterializedState,
}

#[derive(Debug, Clone)]
pub struct IncrementalImpactBuilder {
    config_digest: String,
}

#[derive(Debug, Clone)]
struct ImpactGlobalContext {
    symbol_display_name_by_symbol: BTreeMap<String, String>,
    canonical_symbol_by_symbol: BTreeMap<String, String>,
    aliases_by_canonical_symbol: BTreeMap<String, Vec<String>>,
    symbol_to_file: BTreeMap<String, String>,
    file_to_symbols: BTreeMap<String, Vec<String>>,
    packages_by_root: BTreeMap<String, ImpactPackageMembership>,
    package_by_file: BTreeMap<String, ImpactPackageMembership>,
    package_by_symbol: BTreeMap<String, ImpactPackageMembership>,
    source_candidates_by_package_and_stem: BTreeMap<(String, String), Vec<String>>,
    reference_occurrence_id_by_key: BTreeMap<String, String>,
}

impl IncrementalImpactBuilder {
    pub fn new(config: &ImpactConfig) -> Self {
        Self {
            config_digest: impact_config_digest(config),
        }
    }

    pub fn config_digest(&self) -> &str {
        &self.config_digest
    }

    pub fn build_full(
        &self,
        snapshot: &ComposedSnapshot,
        graph: &SymbolGraph,
        trigger: ImpactRefreshTrigger,
        fallback_reason: Option<ImpactRebuildFallbackReason>,
    ) -> ImpactRefreshResult {
        let started = Instant::now();
        let context = ImpactGlobalContext::build(graph, snapshot);
        let mut contributions = BTreeMap::new();
        for path in context.file_to_symbols.keys() {
            contributions.insert(path.clone(), build_file_contribution(path, graph, &context));
        }
        let plan = assemble_plan(graph, snapshot, &context, &contributions);
        let (entities_recomputed, edges_refreshed) = contribution_stats(contributions.values());
        ImpactRefreshResult {
            config_digest: self.config_digest.clone(),
            fallback_reason,
            stats: ImpactRefreshStats {
                mode: ImpactRefreshMode::FullRebuild,
                trigger,
                files_touched: contributions.len() as u64,
                entities_recomputed,
                edges_refreshed,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
            state: ImpactMaterializedState {
                plan,
                file_contributions: contributions,
            },
        }
    }

    pub fn build_incremental(
        &self,
        previous_snapshot: &ComposedSnapshot,
        snapshot: &ComposedSnapshot,
        diff: &SnapshotDiffResponse,
        graph: &SymbolGraph,
        previous_state: &ImpactMaterializedState,
    ) -> ImpactRefreshResult {
        let started = Instant::now();
        let context = ImpactGlobalContext::build(graph, snapshot);
        let current_signatures = build_current_signatures(graph, &context);
        let touched_files = touched_files(
            diff,
            previous_snapshot,
            snapshot,
            previous_state,
            &current_signatures,
        );

        let current_paths = current_signatures.keys().cloned().collect::<BTreeSet<_>>();
        let mut contributions = previous_state
            .file_contributions
            .iter()
            .filter(|(path, _)| current_paths.contains(*path) && !touched_files.contains(*path))
            .map(|(path, contribution)| (path.clone(), contribution.clone()))
            .collect::<BTreeMap<_, _>>();
        for path in &touched_files {
            if current_paths.contains(path) {
                contributions.insert(path.clone(), build_file_contribution(path, graph, &context));
            }
        }
        let plan = assemble_plan(graph, snapshot, &context, &contributions);
        let touched_contributions = touched_files
            .iter()
            .filter_map(|path| contributions.get(path))
            .collect::<Vec<_>>();
        let (entities_recomputed, edges_refreshed) =
            contribution_stats(touched_contributions.into_iter());

        ImpactRefreshResult {
            config_digest: self.config_digest.clone(),
            fallback_reason: None,
            stats: ImpactRefreshStats {
                mode: ImpactRefreshMode::Incremental,
                trigger: ImpactRefreshTrigger::SnapshotDiff,
                files_touched: touched_files.len() as u64,
                entities_recomputed,
                edges_refreshed,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
            state: ImpactMaterializedState {
                plan,
                file_contributions: contributions,
            },
        }
    }
}

impl ImpactGlobalContext {
    fn build(graph: &SymbolGraph, snapshot: &ComposedSnapshot) -> Self {
        let canonical_symbol_by_symbol = canonical_symbol_map(graph);
        let aliases_by_canonical_symbol =
            aliases_by_canonical_symbol(&canonical_symbol_by_symbol, graph);
        let symbol_display_name_by_symbol = graph
            .symbols
            .iter()
            .map(|(symbol_id, symbol)| (symbol_id.clone(), symbol.display_name.clone()))
            .collect::<BTreeMap<_, _>>();
        let symbol_to_file = graph
            .symbols
            .iter()
            .map(|(symbol_id, symbol)| (symbol_id.clone(), symbol.path.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut file_to_symbols = graph.symbol_ids_by_file.clone();
        sort_and_dedup_map_values(&mut file_to_symbols);
        let (packages_by_root, package_by_file, package_by_symbol) =
            package_indexes(graph, Some(snapshot));
        let source_candidates_by_package_and_stem =
            source_candidates_by_package_and_stem(&file_to_symbols, &package_by_file);
        let reference_occurrence_id_by_key = graph
            .occurrences
            .values()
            .filter(|occurrence| occurrence.role == OccurrenceRole::Reference)
            .map(|occurrence| {
                (
                    occurrence_key(occurrence),
                    occurrence.occurrence_id.0.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        Self {
            symbol_display_name_by_symbol,
            canonical_symbol_by_symbol,
            aliases_by_canonical_symbol,
            symbol_to_file,
            file_to_symbols,
            packages_by_root,
            package_by_file,
            package_by_symbol,
            source_candidates_by_package_and_stem,
            reference_occurrence_id_by_key,
        }
    }
}

fn impact_config_digest(config: &ImpactConfig) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"phase5-impact-incremental-v1\n");
    hasher.update(format!("enabled:{}\n", config.enabled));
    hasher.update(format!("default_limit:{}\n", config.default_limit));
    hasher.update(format!("max_limit:{}\n", config.max_limit));
    hasher.update(format!(
        "default_include_transitive:{}\n",
        config.default_include_transitive
    ));
    hasher.update(format!(
        "default_include_reason_paths:{}\n",
        config.default_include_reason_paths
    ));
    hasher.update(format!(
        "max_reason_paths_per_hit:{}\n",
        config.max_reason_paths_per_hit
    ));
    hasher.update(format!(
        "max_transitive_depth:{}\n",
        config.max_transitive_depth
    ));
    hasher.update(format!(
        "include_possible_results:{}\n",
        config.include_possible_results
    ));
    hasher.update(format!(
        "materialization_mode:{:?}\n",
        config.materialization_mode
    ));
    format!("{:x}", hasher.finalize())
}

fn build_current_signatures(
    graph: &SymbolGraph,
    context: &ImpactGlobalContext,
) -> BTreeMap<String, ImpactFileSignature> {
    context
        .file_to_symbols
        .keys()
        .map(|path| (path.clone(), file_signature(path, graph, context)))
        .collect()
}

fn touched_files(
    diff: &SnapshotDiffResponse,
    previous_snapshot: &ComposedSnapshot,
    snapshot: &ComposedSnapshot,
    previous_state: &ImpactMaterializedState,
    current_signatures: &BTreeMap<String, ImpactFileSignature>,
) -> BTreeSet<String> {
    let mut touched = diff
        .changed_paths
        .iter()
        .filter(|path| {
            current_signatures.contains_key(*path)
                || previous_state.file_contributions.contains_key(*path)
        })
        .cloned()
        .collect::<BTreeSet<_>>();

    if diff.left_snapshot_id != previous_snapshot.snapshot_id
        || diff.right_snapshot_id != snapshot.snapshot_id
    {
        touched.extend(current_signatures.keys().cloned());
        touched.extend(previous_state.file_contributions.keys().cloned());
        return touched;
    }

    let all_paths = current_signatures
        .keys()
        .chain(previous_state.file_contributions.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in all_paths {
        let previous = previous_state
            .file_contributions
            .get(&path)
            .map(|contribution| &contribution.signature);
        let current = current_signatures.get(&path);
        if previous != current {
            touched.insert(path);
        }
    }
    touched
}

fn build_file_contribution(
    path: &str,
    graph: &SymbolGraph,
    context: &ImpactGlobalContext,
) -> ImpactFileContribution {
    let signature = file_signature(path, graph, context);
    let owned_symbol_ids = context
        .file_to_symbols
        .get(path)
        .cloned()
        .unwrap_or_default();
    let mut reverse_references_by_symbol = BTreeMap::<String, Vec<StoredReverseReference>>::new();
    let mut reverse_reference_files_by_symbol = BTreeMap::<String, Vec<String>>::new();
    for occurrence in graph.occurrences_by_file.get(path).into_iter().flatten() {
        if occurrence.role != OccurrenceRole::Reference {
            continue;
        }
        let canonical = context
            .canonical_symbol_by_symbol
            .get(&occurrence.symbol_id.0)
            .cloned()
            .unwrap_or_else(|| occurrence.symbol_id.0.clone());
        reverse_references_by_symbol
            .entry(canonical.clone())
            .or_default()
            .push(StoredReverseReference {
                occurrence_id: occurrence.occurrence_id.0.clone(),
                occurrence_key: occurrence_key(occurrence),
                path: occurrence.path.clone(),
                owner_symbol_id: owner_symbol_for_span(
                    graph,
                    &occurrence.path,
                    occurrence.span.bytes.start,
                    occurrence.span.bytes.end,
                ),
            });
        reverse_reference_files_by_symbol
            .entry(canonical)
            .or_default()
            .push(path.to_string());
    }

    let mut reverse_dependents_by_file = BTreeMap::<String, Vec<String>>::new();
    let file_key = node_key(&GraphNodeRef::File {
        path: path.to_string(),
    });
    if let Some(edges) = graph.outgoing_edges.get(&file_key) {
        for edge in edges {
            let GraphNodeRef::File { path: target_path } = &edge.to else {
                continue;
            };
            if edge.kind != GraphEdgeKind::Imports || target_path == path {
                continue;
            }
            reverse_dependents_by_file
                .entry(target_path.clone())
                .or_default()
                .push(path.to_string());
        }
    }

    let (reverse_imports_by_symbol, reverse_exports_by_symbol) =
        symbol_edge_contributions(&owned_symbol_ids, graph, context);
    let (tests_by_file, tests_by_symbol) = test_contributions(path, graph, context, &signature);

    sort_and_dedup_map_values(&mut reverse_references_by_symbol);
    sort_and_dedup_map_values(&mut reverse_reference_files_by_symbol);
    sort_and_dedup_map_values(&mut reverse_dependents_by_file);

    let reference_count = reverse_references_by_symbol
        .values()
        .map(|values| values.len() as u64)
        .sum::<u64>();
    let import_count = reverse_imports_by_symbol
        .values()
        .map(|values| values.len() as u64)
        .sum::<u64>();
    let export_count = reverse_exports_by_symbol
        .values()
        .map(|values| values.len() as u64)
        .sum::<u64>();
    let file_edge_count = reverse_dependents_by_file
        .values()
        .map(|values| values.len() as u64)
        .sum::<u64>();
    let test_edge_count = tests_by_file
        .values()
        .map(|values| values.len() as u64)
        .sum::<u64>()
        + tests_by_symbol
            .values()
            .map(|values| values.len() as u64)
            .sum::<u64>();

    ImpactFileContribution {
        path: path.to_string(),
        signature: signature.clone(),
        reverse_references_by_symbol,
        reverse_reference_files_by_symbol,
        reverse_imports_by_symbol,
        reverse_exports_by_symbol,
        reverse_dependents_by_file,
        tests_by_file,
        tests_by_symbol,
        is_test_file: signature.is_test_file,
        entity_count: owned_symbol_ids.len() as u64 + u64::from(signature.is_test_file),
        edge_count: reference_count
            + import_count
            + export_count
            + file_edge_count
            + test_edge_count,
    }
}

fn assemble_plan(
    graph: &SymbolGraph,
    snapshot: &ComposedSnapshot,
    context: &ImpactGlobalContext,
    contributions: &BTreeMap<String, ImpactFileContribution>,
) -> ImpactEnrichmentPlan {
    let mut reverse_references_by_symbol = BTreeMap::<String, Vec<ImpactReverseReference>>::new();
    let mut reverse_reference_files_by_symbol = BTreeMap::<String, Vec<String>>::new();
    let mut reverse_imports_by_symbol = BTreeMap::<String, Vec<String>>::new();
    let mut reverse_exports_by_symbol = BTreeMap::<String, Vec<String>>::new();
    let mut reverse_dependents_by_file = BTreeMap::<String, Vec<String>>::new();
    let mut tests_by_file = BTreeMap::<String, Vec<ImpactTestAssociation>>::new();
    let mut tests_by_symbol = BTreeMap::<String, Vec<ImpactTestAssociation>>::new();
    let mut test_files = Vec::new();

    for contribution in contributions.values() {
        if contribution.is_test_file {
            test_files.push(contribution.path.clone());
        }
        for (symbol_id, references) in &contribution.reverse_references_by_symbol {
            let target = reverse_references_by_symbol
                .entry(symbol_id.clone())
                .or_default();
            for reference in references {
                target.push(ImpactReverseReference {
                    occurrence_id: context
                        .reference_occurrence_id_by_key
                        .get(&reference.occurrence_key)
                        .cloned()
                        .unwrap_or_else(|| reference.occurrence_id.clone()),
                    path: reference.path.clone(),
                    owner_symbol_id: reference.owner_symbol_id.clone(),
                });
            }
        }
        merge_string_vec_map(
            &mut reverse_reference_files_by_symbol,
            &contribution.reverse_reference_files_by_symbol,
        );
        merge_string_vec_map(
            &mut reverse_imports_by_symbol,
            &contribution.reverse_imports_by_symbol,
        );
        merge_string_vec_map(
            &mut reverse_exports_by_symbol,
            &contribution.reverse_exports_by_symbol,
        );
        merge_string_vec_map(
            &mut reverse_dependents_by_file,
            &contribution.reverse_dependents_by_file,
        );
        merge_assoc_map(&mut tests_by_file, &contribution.tests_by_file);
        merge_assoc_map(&mut tests_by_symbol, &contribution.tests_by_symbol);
    }

    sort_and_dedup_map_values(&mut reverse_references_by_symbol);
    sort_and_dedup_map_values(&mut reverse_reference_files_by_symbol);
    sort_and_dedup_map_values(&mut reverse_imports_by_symbol);
    sort_and_dedup_map_values(&mut reverse_exports_by_symbol);
    sort_and_dedup_map_values(&mut reverse_dependents_by_file);
    sort_and_dedup_map_values(&mut tests_by_file);
    sort_and_dedup_map_values(&mut tests_by_symbol);
    test_files.sort();
    test_files.dedup();

    let metadata = enrichment_metadata(
        &context.canonical_symbol_by_symbol,
        &reverse_dependents_by_file,
        &context.package_by_file,
        &tests_by_file,
        true,
    );
    let recomputed_layers = metadata
        .iter()
        .filter(|layer| layer.state == ImpactGraphEnrichmentState::Available)
        .count() as u32;
    let mut impacted_files = graph
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    impacted_files.sort();
    impacted_files.dedup();

    let _ = snapshot;
    ImpactEnrichmentPlan {
        recomputed_layers,
        impacted_files,
        audit: audit_graph(),
        metadata,
        symbol_display_name_by_symbol: context.symbol_display_name_by_symbol.clone(),
        canonical_symbol_by_symbol: context.canonical_symbol_by_symbol.clone(),
        aliases_by_canonical_symbol: context.aliases_by_canonical_symbol.clone(),
        reverse_references_by_symbol,
        reverse_reference_files_by_symbol,
        reverse_imports_by_symbol,
        reverse_exports_by_symbol,
        reverse_dependents_by_file,
        symbol_to_file: context.symbol_to_file.clone(),
        file_to_symbols: context.file_to_symbols.clone(),
        packages_by_root: context.packages_by_root.clone(),
        package_by_file: context.package_by_file.clone(),
        package_by_symbol: context.package_by_symbol.clone(),
        test_files,
        tests_by_file,
        tests_by_symbol,
    }
}

fn file_signature(
    path: &str,
    graph: &SymbolGraph,
    context: &ImpactGlobalContext,
) -> ImpactFileSignature {
    let owned_symbol_ids = context
        .file_to_symbols
        .get(path)
        .cloned()
        .unwrap_or_default();
    let package_membership = context.package_by_file.get(path);
    let is_test_file = is_test_path(path);
    let heuristic_candidate_paths = if is_test_file {
        let stem = source_stem_for_test_path(path);
        match (package_membership, stem) {
            (Some(membership), Some(stem)) => context
                .source_candidates_by_package_and_stem
                .get(&(membership.package_root.clone(), stem))
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };

    ImpactFileSignature {
        owned_symbol_ids: owned_symbol_ids.clone(),
        reference_keys: occurrence_signature_keys(path, graph, context, is_test_file),
        file_dependency_targets: file_dependency_targets(path, graph),
        symbol_import_targets: symbol_edge_signature_targets(
            &owned_symbol_ids,
            graph,
            context,
            GraphEdgeKind::Imports,
        ),
        symbol_export_targets: symbol_edge_signature_targets(
            &owned_symbol_ids,
            graph,
            context,
            GraphEdgeKind::Exports,
        ),
        package_name: package_membership.map(|membership| membership.package_name.clone()),
        package_root: package_membership.map(|membership| membership.package_root.clone()),
        is_test_file,
        heuristic_candidate_paths,
    }
}

fn occurrence_signature_keys(
    path: &str,
    graph: &SymbolGraph,
    context: &ImpactGlobalContext,
    include_import_export: bool,
) -> Vec<String> {
    let mut keys = Vec::new();
    for occurrence in graph.occurrences_by_file.get(path).into_iter().flatten() {
        let include = occurrence.role == OccurrenceRole::Reference
            || (include_import_export
                && matches!(
                    occurrence.role,
                    OccurrenceRole::Import | OccurrenceRole::Export
                ));
        if !include {
            continue;
        }
        let canonical = context
            .canonical_symbol_by_symbol
            .get(&occurrence.symbol_id.0)
            .cloned()
            .unwrap_or_else(|| occurrence.symbol_id.0.clone());
        let owner = owner_symbol_for_span(
            graph,
            &occurrence.path,
            occurrence.span.bytes.start,
            occurrence.span.bytes.end,
        );
        keys.push(format!(
            "{:?}|{}|{}|{}|{}",
            occurrence.role,
            canonical,
            occurrence.symbol_id.0,
            occurrence_key(occurrence),
            owner.unwrap_or_default()
        ));
    }
    keys.sort();
    keys.dedup();
    keys
}

fn file_dependency_targets(path: &str, graph: &SymbolGraph) -> Vec<String> {
    let file_key = node_key(&GraphNodeRef::File {
        path: path.to_string(),
    });
    let mut targets = graph
        .outgoing_edges
        .get(&file_key)
        .into_iter()
        .flat_map(|edges| edges.iter())
        .filter_map(|edge| match (&edge.kind, &edge.to) {
            (GraphEdgeKind::Imports, GraphNodeRef::File { path: target_path })
                if target_path != path =>
            {
                Some(target_path.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    targets.sort();
    targets.dedup();
    targets
}

fn symbol_edge_signature_targets(
    owned_symbol_ids: &[String],
    graph: &SymbolGraph,
    context: &ImpactGlobalContext,
    kind: GraphEdgeKind,
) -> Vec<String> {
    let mut targets = Vec::new();
    for symbol_id in owned_symbol_ids {
        let key = node_key(&GraphNodeRef::Symbol {
            symbol_id: hyperindex_protocol::symbols::SymbolId(symbol_id.clone()),
        });
        for edge in graph.outgoing_edges.get(&key).into_iter().flatten() {
            if edge.kind != kind {
                continue;
            }
            let GraphNodeRef::Symbol {
                symbol_id: target_symbol,
            } = &edge.to
            else {
                continue;
            };
            let canonical = context
                .canonical_symbol_by_symbol
                .get(&target_symbol.0)
                .cloned()
                .unwrap_or_else(|| target_symbol.0.clone());
            targets.push(format!("{symbol_id}|{canonical}"));
        }
    }
    targets.sort();
    targets.dedup();
    targets
}

fn symbol_edge_contributions(
    owned_symbol_ids: &[String],
    graph: &SymbolGraph,
    context: &ImpactGlobalContext,
) -> (BTreeMap<String, Vec<String>>, BTreeMap<String, Vec<String>>) {
    let mut reverse_imports_by_symbol = BTreeMap::<String, Vec<String>>::new();
    let mut reverse_exports_by_symbol = BTreeMap::<String, Vec<String>>::new();
    for symbol_id in owned_symbol_ids {
        let key = node_key(&GraphNodeRef::Symbol {
            symbol_id: hyperindex_protocol::symbols::SymbolId(symbol_id.clone()),
        });
        for edge in graph.outgoing_edges.get(&key).into_iter().flatten() {
            let GraphNodeRef::Symbol {
                symbol_id: target_symbol,
            } = &edge.to
            else {
                continue;
            };
            let canonical = context
                .canonical_symbol_by_symbol
                .get(&target_symbol.0)
                .cloned()
                .unwrap_or_else(|| target_symbol.0.clone());
            match edge.kind {
                GraphEdgeKind::Imports => reverse_imports_by_symbol
                    .entry(canonical)
                    .or_default()
                    .push(symbol_id.clone()),
                GraphEdgeKind::Exports => reverse_exports_by_symbol
                    .entry(canonical)
                    .or_default()
                    .push(symbol_id.clone()),
                _ => {}
            }
        }
    }
    sort_and_dedup_map_values(&mut reverse_imports_by_symbol);
    sort_and_dedup_map_values(&mut reverse_exports_by_symbol);
    (reverse_imports_by_symbol, reverse_exports_by_symbol)
}

fn test_contributions(
    path: &str,
    graph: &SymbolGraph,
    context: &ImpactGlobalContext,
    signature: &ImpactFileSignature,
) -> (
    BTreeMap<String, Vec<ImpactTestAssociation>>,
    BTreeMap<String, Vec<ImpactTestAssociation>>,
) {
    let mut tests_by_file = BTreeMap::<String, Vec<ImpactTestAssociation>>::new();
    let mut tests_by_symbol = BTreeMap::<String, Vec<ImpactTestAssociation>>::new();
    if !signature.is_test_file {
        return (tests_by_file, tests_by_symbol);
    }

    for target_path in &signature.file_dependency_targets {
        tests_by_file
            .entry(target_path.clone())
            .or_default()
            .push(ImpactTestAssociation {
                test_path: path.to_string(),
                evidence: crate::impact_enrichment::ImpactTestAssociationEvidence::ImportsFile,
                detail: "test file directly imports this file".to_string(),
                owner_symbol_id: None,
            });
    }

    for occurrence in graph.occurrences_by_file.get(path).into_iter().flatten() {
        if !matches!(
            occurrence.role,
            OccurrenceRole::Reference | OccurrenceRole::Import | OccurrenceRole::Export
        ) {
            continue;
        }
        let canonical = context
            .canonical_symbol_by_symbol
            .get(&occurrence.symbol_id.0)
            .cloned()
            .unwrap_or_else(|| occurrence.symbol_id.0.clone());
        let Some(symbol) = graph.symbols.get(&canonical) else {
            continue;
        };
        if symbol.path == path {
            continue;
        }
        let owner_symbol_id = owner_symbol_for_span(
            graph,
            path,
            occurrence.span.bytes.start,
            occurrence.span.bytes.end,
        );
        let association = ImpactTestAssociation {
            test_path: path.to_string(),
            evidence: crate::impact_enrichment::ImpactTestAssociationEvidence::ReferencesSymbol,
            detail: format!(
                "test file contains a {:?} occurrence for symbol {}",
                occurrence.role, symbol.display_name
            ),
            owner_symbol_id,
        };
        tests_by_symbol
            .entry(canonical.clone())
            .or_default()
            .push(association.clone());
        tests_by_file
            .entry(symbol.path.clone())
            .or_default()
            .push(association);
    }

    if signature.heuristic_candidate_paths.len() == 1 {
        tests_by_file
            .entry(signature.heuristic_candidate_paths[0].clone())
            .or_default()
            .push(ImpactTestAssociation {
                test_path: path.to_string(),
                evidence: crate::impact_enrichment::ImpactTestAssociationEvidence::PathHeuristic,
                detail: "test filename matches a unique same-package source filename".to_string(),
                owner_symbol_id: None,
            });
    }

    sort_and_dedup_map_values(&mut tests_by_file);
    sort_and_dedup_map_values(&mut tests_by_symbol);
    (tests_by_file, tests_by_symbol)
}

fn merge_string_vec_map(
    target: &mut BTreeMap<String, Vec<String>>,
    source: &BTreeMap<String, Vec<String>>,
) {
    for (key, values) in source {
        target
            .entry(key.clone())
            .or_default()
            .extend(values.clone());
    }
}

fn merge_assoc_map<T>(target: &mut BTreeMap<String, Vec<T>>, source: &BTreeMap<String, Vec<T>>)
where
    T: Clone,
{
    for (key, values) in source {
        target
            .entry(key.clone())
            .or_default()
            .extend(values.clone());
    }
}

fn contribution_stats<'a>(
    contributions: impl IntoIterator<Item = &'a ImpactFileContribution>,
) -> (u64, u64) {
    contributions
        .into_iter()
        .fold((0, 0), |(entities, edges), contribution| {
            (
                entities + contribution.entity_count,
                edges + contribution.edge_count,
            )
        })
}

fn occurrence_key(occurrence: &SymbolOccurrence) -> String {
    format!(
        "{}|{}|{:?}|{}:{}-{}:{}",
        occurrence.path,
        occurrence.symbol_id.0,
        occurrence.role,
        occurrence.span.start.line,
        occurrence.span.start.column,
        occurrence.span.end.line,
        occurrence.span.end.column
    )
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::impact::{ImpactAnalyzeParams, ImpactChangeScenario, ImpactTargetRef};
    use hyperindex_protocol::impact::{ImpactRefreshMode, ImpactRefreshTrigger};
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, BufferOverlay, ComposedSnapshot, OverlayEntryKind,
        SnapshotFile, WorkingTreeEntry, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use hyperindex_snapshot::SnapshotAssembler;
    use hyperindex_symbols::SymbolWorkspace;

    use crate::ImpactWorkspace;

    use super::{ImpactRebuildFallbackReason, IncrementalImpactBuilder};

    fn snapshot(
        snapshot_id: &str,
        base_files: Vec<(&str, &str)>,
        working_entries: Vec<WorkingTreeEntry>,
        buffers: Vec<BufferOverlay>,
    ) -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: snapshot_id.to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "abc123".to_string(),
                digest: format!("base-{snapshot_id}"),
                file_count: base_files.len(),
                files: base_files
                    .into_iter()
                    .map(|(path, contents)| SnapshotFile {
                        path: path.to_string(),
                        content_sha256: format!("sha-{snapshot_id}-{path}"),
                        content_bytes: contents.len(),
                        contents: contents.to_string(),
                    })
                    .collect(),
            },
            working_tree: WorkingTreeOverlay {
                digest: format!("work-{snapshot_id}"),
                entries: working_entries,
            },
            buffers,
        }
    }

    fn base_snapshot() -> ComposedSnapshot {
        snapshot(
            "snap-base",
            vec![
                ("package.json", r#"{ "name": "repo-root" }"#),
                (
                    "src/service.ts",
                    r#"
                    export function invalidateSession() {
                      return 1;
                    }
                    "#,
                ),
                (
                    "src/routes/logout.ts",
                    r#"
                    import { invalidateSession } from "../service";
                    export function logout() {
                      return invalidateSession();
                    }
                    "#,
                ),
                (
                    "tests/logout.test.ts",
                    r#"
                    import { logout } from "../src/routes/logout";
                    test("logout", () => {
                      logout();
                    });
                    "#,
                ),
            ],
            Vec::new(),
            Vec::new(),
        )
    }

    fn analyze_output(
        snapshot: &ComposedSnapshot,
        plan: crate::ImpactEnrichmentPlan,
    ) -> serde_json::Value {
        let mut workspace = SymbolWorkspace::default();
        let index = workspace.prepare_snapshot(snapshot).unwrap();
        let params = ImpactAnalyzeParams {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            target: ImpactTargetRef::File {
                path: "src/service.ts".to_string(),
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
        let mut response = ImpactWorkspace::default()
            .analyze_with_enrichment(&index.graph, Some(snapshot), &plan, &params)
            .unwrap();
        response.stats.elapsed_ms = 0;
        serde_json::to_value(response).unwrap()
    }

    fn index(snapshot: &ComposedSnapshot) -> hyperindex_symbols::SymbolScaffoldIndex {
        let mut workspace = SymbolWorkspace::default();
        workspace.prepare_snapshot(snapshot).unwrap()
    }

    #[test]
    fn incremental_single_file_edit_avoids_full_rebuild_and_matches_full_output() {
        let config = hyperindex_protocol::config::ImpactConfig::default();
        let builder = IncrementalImpactBuilder::new(&config);
        let base = base_snapshot();
        let edited = snapshot(
            "snap-edited",
            vec![
                ("package.json", r#"{ "name": "repo-root" }"#),
                (
                    "src/service.ts",
                    r#"
                    export function invalidateSession() {
                      return 2;
                    }
                    "#,
                ),
                (
                    "src/routes/logout.ts",
                    r#"
                    import { invalidateSession } from "../service";
                    export function logout() {
                      return invalidateSession();
                    }
                    "#,
                ),
                (
                    "tests/logout.test.ts",
                    r#"
                    import { logout } from "../src/routes/logout";
                    test("logout", () => {
                      logout();
                    });
                    "#,
                ),
            ],
            Vec::new(),
            Vec::new(),
        );
        let base_index = index(&base);
        let edited_index = index(&edited);
        let base_build = builder.build_full(
            &base,
            &base_index.graph,
            ImpactRefreshTrigger::Bootstrap,
            Some(ImpactRebuildFallbackReason::NoPriorSnapshot),
        );
        let diff = SnapshotAssembler.diff(&base, &edited);
        let refreshed = builder.build_incremental(
            &base,
            &edited,
            &diff,
            &edited_index.graph,
            &base_build.state,
        );
        let full = builder.build_full(
            &edited,
            &edited_index.graph,
            ImpactRefreshTrigger::SnapshotDiff,
            None,
        );

        assert_eq!(refreshed.stats.mode, ImpactRefreshMode::Incremental);
        assert!(refreshed.stats.files_touched < full.stats.files_touched);
        assert_eq!(
            analyze_output(&edited, refreshed.state.plan.clone()),
            analyze_output(&edited, full.state.plan.clone())
        );
    }

    #[test]
    fn buffer_overlay_changes_impact_before_save_and_matches_full_output() {
        let config = hyperindex_protocol::config::ImpactConfig::default();
        let builder = IncrementalImpactBuilder::new(&config);
        let base = base_snapshot();
        let buffered = snapshot(
            "snap-buffer",
            vec![
                ("package.json", r#"{ "name": "repo-root" }"#),
                (
                    "src/service.ts",
                    r#"
                    export function invalidateSession() {
                      return 1;
                    }
                    "#,
                ),
                (
                    "src/routes/logout.ts",
                    r#"
                    import { invalidateSessionBuffered } from "../service";
                    export function logout() {
                      return invalidateSessionBuffered();
                    }
                    "#,
                ),
                (
                    "tests/logout.test.ts",
                    r#"
                    import { logout } from "../src/routes/logout";
                    test("logout", () => {
                      logout();
                    });
                    "#,
                ),
            ],
            Vec::new(),
            vec![BufferOverlay {
                buffer_id: "buffer-1".to_string(),
                path: "src/service.ts".to_string(),
                version: 1,
                content_sha256: "sha-buffer".to_string(),
                content_bytes: 93,
                contents: r#"
                export function invalidateSessionBuffered() {
                  return 3;
                }
                "#
                .to_string(),
            }],
        );
        let base_index = index(&base);
        let buffered_index = index(&buffered);
        let base_build = builder.build_full(
            &base,
            &base_index.graph,
            ImpactRefreshTrigger::Bootstrap,
            Some(ImpactRebuildFallbackReason::NoPriorSnapshot),
        );
        let diff = SnapshotAssembler.diff(&base, &buffered);
        let refreshed = builder.build_incremental(
            &base,
            &buffered,
            &diff,
            &buffered_index.graph,
            &base_build.state,
        );
        let full = builder.build_full(
            &buffered,
            &buffered_index.graph,
            ImpactRefreshTrigger::SnapshotDiff,
            None,
        );

        assert_eq!(
            diff.buffer_only_changed_paths,
            vec!["src/service.ts".to_string()]
        );
        assert_eq!(
            analyze_output(&buffered, refreshed.state.plan.clone()),
            analyze_output(&buffered, full.state.plan.clone())
        );
    }

    #[test]
    fn add_delete_and_modify_flows_match_full_output() {
        let config = hyperindex_protocol::config::ImpactConfig::default();
        let builder = IncrementalImpactBuilder::new(&config);
        let base = base_snapshot();
        let next = snapshot(
            "snap-next",
            vec![
                ("package.json", r#"{ "name": "repo-root" }"#),
                (
                    "src/service.ts",
                    r#"
                    export function invalidateSession() {
                      return 7;
                    }
                    "#,
                ),
                (
                    "src/new.ts",
                    r#"
                    import { invalidateSession } from "./service";
                    export function useService() {
                      return invalidateSession();
                    }
                    "#,
                ),
                (
                    "tests/logout.test.ts",
                    r#"
                    import { useService } from "../src/new";
                    test("logout", () => {
                      useService();
                    });
                    "#,
                ),
            ],
            vec![WorkingTreeEntry {
                path: "src/routes/logout.ts".to_string(),
                kind: OverlayEntryKind::Delete,
                content_sha256: None,
                content_bytes: None,
                contents: None,
            }],
            Vec::new(),
        );
        let base_index = index(&base);
        let next_index = index(&next);
        let base_build = builder.build_full(
            &base,
            &base_index.graph,
            ImpactRefreshTrigger::Bootstrap,
            Some(ImpactRebuildFallbackReason::NoPriorSnapshot),
        );
        let diff = SnapshotAssembler.diff(&base, &next);
        let refreshed =
            builder.build_incremental(&base, &next, &diff, &next_index.graph, &base_build.state);
        let full = builder.build_full(
            &next,
            &next_index.graph,
            ImpactRefreshTrigger::SnapshotDiff,
            None,
        );

        assert_eq!(
            analyze_output(&next, refreshed.state.plan.clone()),
            analyze_output(&next, full.state.plan.clone())
        );
    }
}
