use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use hyperindex_protocol::impact::{
    ImpactGraphEnrichmentKind, ImpactGraphEnrichmentMetadata, ImpactGraphEnrichmentState,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::symbols::{GraphEdgeKind, GraphNodeRef, OccurrenceRole, SymbolKind};
use hyperindex_snapshot::SnapshotAssembler;
use hyperindex_symbols::SymbolGraph;
use serde::{Deserialize, Serialize};

use crate::common::{ImpactComponentStatus, implemented_status};
use crate::impact_model::ImpactModelSeed;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactDeferredFeatureKind {
    ConfigKey,
    Route,
    Api,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ImpactDeferredFeature {
    pub kind: ImpactDeferredFeatureKind,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactGraphAudit {
    pub available_edge_kinds: Vec<String>,
    pub directly_usable_indexes: Vec<String>,
    pub deferred_features: Vec<ImpactDeferredFeature>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactPackageEvidence {
    SnapshotPackageJson,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ImpactPackageMembership {
    pub package_name: String,
    pub package_root: String,
    pub evidence: ImpactPackageEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ImpactReverseReference {
    pub occurrence_id: String,
    pub path: String,
    pub owner_symbol_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactTestAssociationEvidence {
    ImportsFile,
    ReferencesSymbol,
    PathHeuristic,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ImpactTestAssociation {
    pub test_path: String,
    pub evidence: ImpactTestAssociationEvidence,
    pub detail: String,
    pub owner_symbol_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactEnrichmentPlan {
    pub recomputed_layers: u32,
    pub impacted_files: Vec<String>,
    pub audit: ImpactGraphAudit,
    pub metadata: Vec<ImpactGraphEnrichmentMetadata>,
    pub symbol_display_name_by_symbol: BTreeMap<String, String>,
    pub canonical_symbol_by_symbol: BTreeMap<String, String>,
    pub aliases_by_canonical_symbol: BTreeMap<String, Vec<String>>,
    pub reverse_references_by_symbol: BTreeMap<String, Vec<ImpactReverseReference>>,
    pub reverse_reference_files_by_symbol: BTreeMap<String, Vec<String>>,
    pub reverse_imports_by_symbol: BTreeMap<String, Vec<String>>,
    pub reverse_exports_by_symbol: BTreeMap<String, Vec<String>>,
    pub reverse_dependents_by_file: BTreeMap<String, Vec<String>>,
    pub symbol_to_file: BTreeMap<String, String>,
    pub file_to_symbols: BTreeMap<String, Vec<String>>,
    pub packages_by_root: BTreeMap<String, ImpactPackageMembership>,
    pub package_by_file: BTreeMap<String, ImpactPackageMembership>,
    pub package_by_symbol: BTreeMap<String, ImpactPackageMembership>,
    pub test_files: Vec<String>,
    pub tests_by_file: BTreeMap<String, Vec<ImpactTestAssociation>>,
    pub tests_by_symbol: BTreeMap<String, Vec<ImpactTestAssociation>>,
}

#[derive(Debug, Default, Clone)]
pub struct ImpactEnrichmentPlanner;

impl ImpactEnrichmentPlanner {
    pub fn plan(
        &self,
        graph: &SymbolGraph,
        snapshot: Option<&ComposedSnapshot>,
        _seed: &ImpactModelSeed,
    ) -> ImpactEnrichmentPlan {
        self.build(graph, snapshot)
    }

    pub fn build(
        &self,
        graph: &SymbolGraph,
        snapshot: Option<&ComposedSnapshot>,
    ) -> ImpactEnrichmentPlan {
        let audit = audit_graph();
        let canonical_symbol_by_symbol = canonical_symbol_map(graph);
        let aliases_by_canonical_symbol =
            aliases_by_canonical_symbol(&canonical_symbol_by_symbol, graph);
        let symbol_to_file = graph
            .symbols
            .iter()
            .map(|(symbol_id, symbol)| (symbol_id.clone(), symbol.path.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut file_to_symbols = graph.symbol_ids_by_file.clone();
        sort_and_dedup_map_values(&mut file_to_symbols);
        let symbol_display_name_by_symbol = graph
            .symbols
            .iter()
            .map(|(symbol_id, symbol)| (symbol_id.clone(), symbol.display_name.clone()))
            .collect::<BTreeMap<_, _>>();

        let (reverse_imports_by_symbol, reverse_exports_by_symbol, reverse_dependents_by_file) =
            reverse_edge_indexes(graph, &canonical_symbol_by_symbol);
        let (reverse_references_by_symbol, reverse_reference_files_by_symbol) =
            reverse_reference_indexes(graph, &canonical_symbol_by_symbol);
        let (packages_by_root, package_by_file, package_by_symbol) =
            package_indexes(graph, snapshot);
        let source_candidates_by_package_and_stem =
            source_candidates_by_package_and_stem(&file_to_symbols, &package_by_file);
        let (test_files, tests_by_file, tests_by_symbol) = test_associations(
            graph,
            &canonical_symbol_by_symbol,
            &package_by_file,
            &file_to_symbols,
            &source_candidates_by_package_and_stem,
        );

        let metadata = enrichment_metadata(
            &canonical_symbol_by_symbol,
            &reverse_dependents_by_file,
            &package_by_file,
            &tests_by_file,
            snapshot.is_some(),
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

        ImpactEnrichmentPlan {
            recomputed_layers,
            impacted_files,
            audit,
            metadata,
            symbol_display_name_by_symbol,
            canonical_symbol_by_symbol,
            aliases_by_canonical_symbol,
            reverse_references_by_symbol,
            reverse_reference_files_by_symbol,
            reverse_imports_by_symbol,
            reverse_exports_by_symbol,
            reverse_dependents_by_file,
            symbol_to_file,
            file_to_symbols,
            packages_by_root,
            package_by_file,
            package_by_symbol,
            test_files,
            tests_by_file,
            tests_by_symbol,
        }
    }

    pub fn status(&self) -> ImpactComponentStatus {
        implemented_status(
            "impact_enrichment",
            "deterministic reverse indexes, ownership maps, package membership, test affinity, and explicit route/config/api deferrals are available for impact planning",
        )
    }
}

pub(crate) fn audit_graph() -> ImpactGraphAudit {
    ImpactGraphAudit {
        available_edge_kinds: vec![
            "contains".to_string(),
            "defines".to_string(),
            "references".to_string(),
            "imports".to_string(),
            "exports".to_string(),
        ],
        directly_usable_indexes: vec![
            "symbols".to_string(),
            "symbol_facts".to_string(),
            "occurrences".to_string(),
            "occurrences_by_symbol".to_string(),
            "occurrences_by_file".to_string(),
            "symbol_ids_by_file".to_string(),
            "symbol_ids_by_span".to_string(),
            "incoming_edges".to_string(),
            "outgoing_edges".to_string(),
        ],
        deferred_features: vec![
            ImpactDeferredFeature {
                kind: ImpactDeferredFeatureKind::ConfigKey,
                reason: "the checked-in Phase 4 graph has no first-class config-key declarations or usage edges".to_string(),
            },
            ImpactDeferredFeature {
                kind: ImpactDeferredFeatureKind::Route,
                reason: "route-like files can be file-backed, but the graph has no dedicated route registry or route edges".to_string(),
            },
            ImpactDeferredFeature {
                kind: ImpactDeferredFeatureKind::Api,
                reason: "the checked-in fact model does not expose endpoint or API contract anchors beyond normal files and symbols".to_string(),
            },
        ],
    }
}

pub(crate) fn canonical_symbol_map(graph: &SymbolGraph) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for symbol_id in graph.symbols.keys() {
        let canonical =
            canonical_symbol_id_memoized(graph, symbol_id, &mut result, &mut BTreeSet::new());
        result.insert(symbol_id.clone(), canonical);
    }
    result
}

pub(crate) fn aliases_by_canonical_symbol(
    canonical_symbol_by_symbol: &BTreeMap<String, String>,
    graph: &SymbolGraph,
) -> BTreeMap<String, Vec<String>> {
    let mut aliases = BTreeMap::<String, Vec<String>>::new();
    for symbol_id in graph.symbols.keys() {
        let canonical = canonical_symbol_by_symbol
            .get(symbol_id)
            .cloned()
            .unwrap_or_else(|| symbol_id.clone());
        aliases
            .entry(canonical)
            .or_default()
            .push(symbol_id.clone());
    }
    sort_and_dedup_map_values(&mut aliases);
    aliases
}

fn canonical_symbol_id_memoized(
    graph: &SymbolGraph,
    symbol_id: &str,
    cache: &mut BTreeMap<String, String>,
    visiting: &mut BTreeSet<String>,
) -> String {
    if let Some(canonical) = cache.get(symbol_id) {
        return canonical.clone();
    }
    if !visiting.insert(symbol_id.to_string()) {
        return symbol_id.to_string();
    }

    let canonical = match unique_alias_target(graph, symbol_id) {
        Some(next_symbol_id) if next_symbol_id != symbol_id => {
            canonical_symbol_id_memoized(graph, &next_symbol_id, cache, visiting)
        }
        _ => symbol_id.to_string(),
    };

    visiting.remove(symbol_id);
    cache.insert(symbol_id.to_string(), canonical.clone());
    canonical
}

fn unique_alias_target(graph: &SymbolGraph, symbol_id: &str) -> Option<String> {
    let key = node_key(&GraphNodeRef::Symbol {
        symbol_id: hyperindex_protocol::symbols::SymbolId(symbol_id.to_string()),
    });
    let mut unique = None::<String>;
    for edge in graph.outgoing_edges.get(&key).into_iter().flatten() {
        if !matches!(edge.kind, GraphEdgeKind::Imports | GraphEdgeKind::Exports) {
            continue;
        }
        let GraphNodeRef::Symbol {
            symbol_id: target_symbol_id,
        } = &edge.to
        else {
            continue;
        };
        match unique.as_deref() {
            None => unique = Some(target_symbol_id.0.clone()),
            Some(existing) if existing == target_symbol_id.0 => {}
            Some(_) => return None,
        }
    }
    unique
}

fn reverse_edge_indexes(
    graph: &SymbolGraph,
    canonical_symbol_by_symbol: &BTreeMap<String, String>,
) -> (
    BTreeMap<String, Vec<String>>,
    BTreeMap<String, Vec<String>>,
    BTreeMap<String, Vec<String>>,
) {
    let mut reverse_imports_by_symbol = BTreeMap::<String, Vec<String>>::new();
    let mut reverse_exports_by_symbol = BTreeMap::<String, Vec<String>>::new();
    let mut reverse_dependents_by_file = BTreeMap::<String, Vec<String>>::new();

    for edge in &graph.edges {
        match (&edge.kind, &edge.from, &edge.to) {
            (
                GraphEdgeKind::Imports,
                GraphNodeRef::File { path: from_path },
                GraphNodeRef::File { path: to_path },
            ) => {
                reverse_dependents_by_file
                    .entry(to_path.clone())
                    .or_default()
                    .push(from_path.clone());
            }
            (
                GraphEdgeKind::Imports,
                GraphNodeRef::Symbol {
                    symbol_id: from_symbol_id,
                },
                GraphNodeRef::Symbol {
                    symbol_id: to_symbol_id,
                },
            ) => {
                let canonical = canonical_symbol_by_symbol
                    .get(&to_symbol_id.0)
                    .cloned()
                    .unwrap_or_else(|| to_symbol_id.0.clone());
                reverse_imports_by_symbol
                    .entry(canonical)
                    .or_default()
                    .push(from_symbol_id.0.clone());
            }
            (
                GraphEdgeKind::Exports,
                GraphNodeRef::Symbol {
                    symbol_id: from_symbol_id,
                },
                GraphNodeRef::Symbol {
                    symbol_id: to_symbol_id,
                },
            ) => {
                let canonical = canonical_symbol_by_symbol
                    .get(&to_symbol_id.0)
                    .cloned()
                    .unwrap_or_else(|| to_symbol_id.0.clone());
                reverse_exports_by_symbol
                    .entry(canonical)
                    .or_default()
                    .push(from_symbol_id.0.clone());
            }
            _ => {}
        }
    }

    sort_and_dedup_map_values(&mut reverse_imports_by_symbol);
    sort_and_dedup_map_values(&mut reverse_exports_by_symbol);
    sort_and_dedup_map_values(&mut reverse_dependents_by_file);
    (
        reverse_imports_by_symbol,
        reverse_exports_by_symbol,
        reverse_dependents_by_file,
    )
}

fn reverse_reference_indexes(
    graph: &SymbolGraph,
    canonical_symbol_by_symbol: &BTreeMap<String, String>,
) -> (
    BTreeMap<String, Vec<ImpactReverseReference>>,
    BTreeMap<String, Vec<String>>,
) {
    let mut reverse_references_by_symbol = BTreeMap::<String, Vec<ImpactReverseReference>>::new();
    let mut reverse_reference_files_by_symbol = BTreeMap::<String, Vec<String>>::new();

    for occurrence in graph.occurrences.values() {
        if occurrence.role != OccurrenceRole::Reference {
            continue;
        }
        let canonical = canonical_symbol_by_symbol
            .get(&occurrence.symbol_id.0)
            .cloned()
            .unwrap_or_else(|| occurrence.symbol_id.0.clone());
        reverse_references_by_symbol
            .entry(canonical.clone())
            .or_default()
            .push(ImpactReverseReference {
                occurrence_id: occurrence.occurrence_id.0.clone(),
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
            .push(occurrence.path.clone());
    }

    sort_and_dedup_map_values(&mut reverse_references_by_symbol);
    sort_and_dedup_map_values(&mut reverse_reference_files_by_symbol);
    (
        reverse_references_by_symbol,
        reverse_reference_files_by_symbol,
    )
}

pub(crate) fn owner_symbol_for_span(
    graph: &SymbolGraph,
    path: &str,
    start: u32,
    end: u32,
) -> Option<String> {
    graph.symbol_ids_by_span.get(path).and_then(|spans| {
        spans
            .iter()
            .filter_map(|span_index| {
                let symbol = graph.symbols.get(&span_index.symbol_id)?;
                (span_index.start_byte <= start && span_index.end_byte >= end).then_some(symbol)
            })
            .filter(|symbol| symbol.kind != SymbolKind::Module)
            .min_by(|left, right| {
                let left_width = left.span.bytes.end - left.span.bytes.start;
                let right_width = right.span.bytes.end - right.span.bytes.start;
                left_width
                    .cmp(&right_width)
                    .then_with(|| left.span.bytes.start.cmp(&right.span.bytes.start))
                    .then_with(|| left.symbol_id.0.cmp(&right.symbol_id.0))
            })
            .map(|symbol| symbol.symbol_id.0.clone())
    })
}

pub(crate) fn package_indexes(
    graph: &SymbolGraph,
    snapshot: Option<&ComposedSnapshot>,
) -> (
    BTreeMap<String, ImpactPackageMembership>,
    BTreeMap<String, ImpactPackageMembership>,
    BTreeMap<String, ImpactPackageMembership>,
) {
    let Some(snapshot) = snapshot else {
        return (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    };

    let packages = discover_packages(snapshot);
    let packages_by_root = packages
        .iter()
        .map(|package| {
            (
                package.package_root.clone(),
                ImpactPackageMembership {
                    package_name: package.package_name.clone(),
                    package_root: package.package_root.clone(),
                    evidence: ImpactPackageEvidence::SnapshotPackageJson,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut package_by_file = BTreeMap::new();
    for file in &graph.files {
        let Some(package) = packages
            .iter()
            .filter(|package| path_is_within_package(&file.path, &package.package_root))
            .max_by(|left, right| {
                left.package_root
                    .len()
                    .cmp(&right.package_root.len())
                    .then_with(|| left.package_name.cmp(&right.package_name))
            })
        else {
            continue;
        };
        package_by_file.insert(
            file.path.clone(),
            ImpactPackageMembership {
                package_name: package.package_name.clone(),
                package_root: package.package_root.clone(),
                evidence: ImpactPackageEvidence::SnapshotPackageJson,
            },
        );
    }

    let mut package_by_symbol = BTreeMap::new();
    for (symbol_id, symbol) in &graph.symbols {
        if let Some(membership) = package_by_file.get(&symbol.path) {
            package_by_symbol.insert(symbol_id.clone(), membership.clone());
        }
    }

    (packages_by_root, package_by_file, package_by_symbol)
}

fn test_associations(
    graph: &SymbolGraph,
    canonical_symbol_by_symbol: &BTreeMap<String, String>,
    package_by_file: &BTreeMap<String, ImpactPackageMembership>,
    file_to_symbols: &BTreeMap<String, Vec<String>>,
    source_candidates_by_package_and_stem: &BTreeMap<(String, String), Vec<String>>,
) -> (
    Vec<String>,
    BTreeMap<String, Vec<ImpactTestAssociation>>,
    BTreeMap<String, Vec<ImpactTestAssociation>>,
) {
    let test_files = file_to_symbols
        .keys()
        .filter(|path| is_test_path(path))
        .cloned()
        .collect::<Vec<_>>();

    let mut tests_by_file = BTreeMap::<String, BTreeSet<ImpactTestAssociation>>::new();
    let mut tests_by_symbol = BTreeMap::<String, BTreeSet<ImpactTestAssociation>>::new();

    for test_path in &test_files {
        let file_key = node_key(&GraphNodeRef::File {
            path: test_path.clone(),
        });
        if let Some(edges) = graph.outgoing_edges.get(&file_key) {
            for edge in edges {
                let GraphNodeRef::File { path: target_path } = &edge.to else {
                    continue;
                };
                if edge.kind != GraphEdgeKind::Imports || target_path == test_path {
                    continue;
                }
                tests_by_file
                    .entry(target_path.clone())
                    .or_default()
                    .insert(ImpactTestAssociation {
                        test_path: test_path.clone(),
                        evidence: ImpactTestAssociationEvidence::ImportsFile,
                        detail: "test file directly imports this file".to_string(),
                        owner_symbol_id: None,
                    });
            }
        }

        if let Some(occurrences) = graph.occurrences_by_file.get(test_path) {
            for occurrence in occurrences {
                if !matches!(
                    occurrence.role,
                    OccurrenceRole::Reference | OccurrenceRole::Import | OccurrenceRole::Export
                ) {
                    continue;
                }
                let canonical = canonical_symbol_by_symbol
                    .get(&occurrence.symbol_id.0)
                    .cloned()
                    .unwrap_or_else(|| occurrence.symbol_id.0.clone());
                let Some(symbol) = graph.symbols.get(&canonical) else {
                    continue;
                };
                if symbol.path == *test_path {
                    continue;
                }
                let owner_symbol_id = owner_symbol_for_span(
                    graph,
                    test_path,
                    occurrence.span.bytes.start,
                    occurrence.span.bytes.end,
                );
                let detail = format!(
                    "test file contains a {:?} occurrence for symbol {}",
                    occurrence.role, symbol.display_name
                );
                let association = ImpactTestAssociation {
                    test_path: test_path.clone(),
                    evidence: ImpactTestAssociationEvidence::ReferencesSymbol,
                    detail,
                    owner_symbol_id,
                };
                tests_by_symbol
                    .entry(canonical.clone())
                    .or_default()
                    .insert(association.clone());
                tests_by_file
                    .entry(symbol.path.clone())
                    .or_default()
                    .insert(association);
            }
        }

        let Some(test_membership) = package_by_file.get(test_path) else {
            continue;
        };
        let Some(test_stem) = source_stem_for_test_path(test_path) else {
            continue;
        };
        let candidates = source_candidates_by_package_and_stem
            .get(&(test_membership.package_root.clone(), test_stem))
            .cloned()
            .unwrap_or_default();
        if candidates.len() != 1 {
            continue;
        }
        tests_by_file
            .entry(candidates[0].clone())
            .or_default()
            .insert(ImpactTestAssociation {
                test_path: test_path.clone(),
                evidence: ImpactTestAssociationEvidence::PathHeuristic,
                detail: "test filename matches a unique same-package source filename".to_string(),
                owner_symbol_id: None,
            });
    }

    let mut tests_by_file = tests_by_file
        .into_iter()
        .map(|(path, associations)| (path, associations.into_iter().collect::<Vec<_>>()))
        .collect::<BTreeMap<_, _>>();
    let mut tests_by_symbol = tests_by_symbol
        .into_iter()
        .map(|(symbol_id, associations)| (symbol_id, associations.into_iter().collect::<Vec<_>>()))
        .collect::<BTreeMap<_, _>>();
    sort_and_dedup_map_values(&mut tests_by_file);
    sort_and_dedup_map_values(&mut tests_by_symbol);
    (test_files, tests_by_file, tests_by_symbol)
}

pub(crate) fn source_candidates_by_package_and_stem(
    file_to_symbols: &BTreeMap<String, Vec<String>>,
    package_by_file: &BTreeMap<String, ImpactPackageMembership>,
) -> BTreeMap<(String, String), Vec<String>> {
    let mut result = BTreeMap::<(String, String), Vec<String>>::new();
    for path in file_to_symbols.keys() {
        if is_test_path(path) {
            continue;
        }
        let Some(membership) = package_by_file.get(path) else {
            continue;
        };
        let Some(stem) = file_source_stem(path) else {
            continue;
        };
        result
            .entry((membership.package_root.clone(), stem))
            .or_default()
            .push(path.clone());
    }
    for values in result.values_mut() {
        values.sort();
        values.dedup();
    }
    result
}

pub(crate) fn enrichment_metadata(
    canonical_symbol_by_symbol: &BTreeMap<String, String>,
    reverse_dependents_by_file: &BTreeMap<String, Vec<String>>,
    package_by_file: &BTreeMap<String, ImpactPackageMembership>,
    tests_by_file: &BTreeMap<String, Vec<ImpactTestAssociation>>,
    snapshot_available: bool,
) -> Vec<ImpactGraphEnrichmentMetadata> {
    vec![
        ImpactGraphEnrichmentMetadata {
            kind: ImpactGraphEnrichmentKind::CanonicalAlias,
            state: ImpactGraphEnrichmentState::Available,
            evidence_count: Some(canonical_symbol_by_symbol.len() as u64),
        },
        ImpactGraphEnrichmentMetadata {
            kind: ImpactGraphEnrichmentKind::FileAdjacency,
            state: ImpactGraphEnrichmentState::Available,
            evidence_count: Some(
                reverse_dependents_by_file
                    .values()
                    .map(|values| values.len() as u64)
                    .sum(),
            ),
        },
        ImpactGraphEnrichmentMetadata {
            kind: ImpactGraphEnrichmentKind::PackageMembership,
            state: if snapshot_available {
                ImpactGraphEnrichmentState::Available
            } else {
                ImpactGraphEnrichmentState::Deferred
            },
            evidence_count: snapshot_available.then_some(package_by_file.len() as u64),
        },
        ImpactGraphEnrichmentMetadata {
            kind: ImpactGraphEnrichmentKind::TestAffinity,
            state: ImpactGraphEnrichmentState::Available,
            evidence_count: Some(
                tests_by_file
                    .values()
                    .map(|values| values.len() as u64)
                    .sum(),
            ),
        },
    ]
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspacePackage {
    pub package_name: String,
    pub package_root: String,
}

pub(crate) fn discover_packages(snapshot: &ComposedSnapshot) -> Vec<WorkspacePackage> {
    let assembler = SnapshotAssembler;
    let mut package_paths = BTreeSet::new();
    for file in &snapshot.base.files {
        if file.path == "package.json" || file.path.ends_with("/package.json") {
            package_paths.insert(file.path.clone());
        }
    }
    for entry in &snapshot.working_tree.entries {
        if entry.path == "package.json" || entry.path.ends_with("/package.json") {
            package_paths.insert(entry.path.clone());
        }
    }
    for buffer in &snapshot.buffers {
        if buffer.path == "package.json" || buffer.path.ends_with("/package.json") {
            package_paths.insert(buffer.path.clone());
        }
    }

    let mut packages = package_paths
        .into_iter()
        .filter_map(|path| {
            let contents = assembler.resolve_file(snapshot, &path)?.contents;
            let package_name = serde_json::from_str::<serde_json::Value>(&contents)
                .ok()?
                .get("name")?
                .as_str()?
                .to_string();
            let root = Path::new(&path).parent().and_then(normalize_repo_path)?;
            Some(WorkspacePackage {
                package_name,
                package_root: root,
            })
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| {
        left.package_root
            .len()
            .cmp(&right.package_root.len())
            .reverse()
            .then_with(|| left.package_root.cmp(&right.package_root))
            .then_with(|| left.package_name.cmp(&right.package_name))
    });
    packages
}

pub(crate) fn path_is_within_package(path: &str, package_root: &str) -> bool {
    package_root.is_empty() || path == package_root || path.starts_with(&format!("{package_root}/"))
}

pub(crate) fn normalize_repo_path(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_str()?.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.join("/"))
}

pub(crate) fn is_test_path(path: &str) -> bool {
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

pub(crate) fn source_stem_for_test_path(path: &str) -> Option<String> {
    let file_name = Path::new(path).file_name()?.to_str()?.to_ascii_lowercase();
    let stem = [
        ".test.tsx",
        ".test.ts",
        ".test.jsx",
        ".test.js",
        ".spec.tsx",
        ".spec.ts",
        ".spec.jsx",
        ".spec.js",
    ]
    .iter()
    .find_map(|suffix| file_name.strip_suffix(suffix))
    .or_else(|| Path::new(&file_name).file_stem()?.to_str())?;
    Some(stem.to_string())
}

pub(crate) fn file_source_stem(path: &str) -> Option<String> {
    let file_name = Path::new(path).file_stem()?.to_str()?.to_ascii_lowercase();
    Some(file_name)
}

pub(crate) fn node_key(node: &GraphNodeRef) -> String {
    match node {
        GraphNodeRef::Symbol { symbol_id } => format!("symbol:{}", symbol_id.0),
        GraphNodeRef::Occurrence { occurrence_id } => format!("occurrence:{}", occurrence_id.0),
        GraphNodeRef::File { path } => format!("file:{path}"),
    }
}

pub(crate) fn sort_and_dedup_map_values<T>(map: &mut BTreeMap<String, Vec<T>>)
where
    T: Ord,
{
    for values in map.values_mut() {
        values.sort();
        values.dedup();
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, SnapshotFile, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use hyperindex_symbols::SymbolWorkspace;

    use super::{
        ImpactDeferredFeatureKind, ImpactEnrichmentPlanner, ImpactTestAssociationEvidence,
    };

    fn snapshot(files: Vec<(&str, &str)>) -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: "snap-impact-enrichment".to_string(),
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

    fn build_fixture_plan() -> super::ImpactEnrichmentPlan {
        let snapshot = snapshot(vec![
            ("package.json", r#"{ "name": "repo-root" }"#),
            (
                "packages/auth/package.json",
                r#"{ "name": "@hyperindex/auth" }"#,
            ),
            (
                "packages/api/package.json",
                r#"{ "name": "@hyperindex/api" }"#,
            ),
            (
                "packages/auth/src/session/service.ts",
                r#"
                export function createSession() {
                  return 1;
                }

                export function invalidateSession() {
                  return createSession();
                }
                "#,
            ),
            (
                "packages/auth/src/index.ts",
                r#"
                export { createSession, invalidateSession } from "./session/service";
                "#,
            ),
            (
                "packages/api/src/routes/logout.ts",
                r#"
                import { invalidateSession } from "@hyperindex/auth";

                export function logout() {
                  return invalidateSession();
                }
                "#,
            ),
            (
                "packages/auth/tests/session/service.test.ts",
                r#"
                describe("service", () => {
                  it("keeps the stem heuristic deterministic", () => {
                    expect(true).toBe(true);
                  });
                });
                "#,
            ),
            (
                "packages/auth/tests/session/session-callers.test.ts",
                r#"
                import { createSession, invalidateSession } from "../../src/session/service";

                test("session callers", () => {
                  createSession();
                  invalidateSession();
                });
                "#,
            ),
            (
                "packages/api/tests/logout-route.test.ts",
                r#"
                import { logout } from "../src/routes/logout";

                test("logout route", () => {
                  logout();
                });
                "#,
            ),
        ]);
        let mut workspace = SymbolWorkspace::default();
        let index = workspace.prepare_snapshot(&snapshot).unwrap();
        ImpactEnrichmentPlanner::default().build(&index.graph, Some(&snapshot))
    }

    fn canonical_symbol_id(
        plan: &super::ImpactEnrichmentPlan,
        path: &str,
        display_name: &str,
    ) -> String {
        let symbol_id = plan
            .symbol_to_file
            .iter()
            .find_map(|(symbol_id, owner_path)| {
                (owner_path == path
                    && plan
                        .symbol_display_name_by_symbol
                        .get(symbol_id)
                        .map(|name| name.as_str())
                        == Some(display_name))
                .then_some(symbol_id.clone())
            })
            .unwrap();
        plan.canonical_symbol_by_symbol
            .get(&symbol_id)
            .cloned()
            .unwrap()
    }

    #[test]
    fn enrichment_outputs_are_deterministic() {
        let left = build_fixture_plan();
        let right = build_fixture_plan();

        assert_eq!(left, right);
        assert_eq!(left.recomputed_layers, 4);
    }

    #[test]
    fn reverse_lookup_indexes_are_correct_on_fixture() {
        let plan = build_fixture_plan();
        let invalidate_session = canonical_symbol_id(
            &plan,
            "packages/auth/src/session/service.ts",
            "invalidateSession",
        );

        assert_eq!(
            plan.reverse_dependents_by_file
                .get("packages/auth/src/session/service.ts")
                .unwrap(),
            &vec![
                "packages/auth/src/index.ts".to_string(),
                "packages/auth/tests/session/session-callers.test.ts".to_string(),
            ]
        );

        let reverse_import_paths = plan
            .reverse_imports_by_symbol
            .get(&invalidate_session)
            .unwrap()
            .iter()
            .filter_map(|symbol_id| plan.symbol_to_file.get(symbol_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut reverse_import_paths = reverse_import_paths;
        reverse_import_paths.sort();
        assert_eq!(
            reverse_import_paths,
            vec![
                "packages/api/src/routes/logout.ts".to_string(),
                "packages/auth/tests/session/session-callers.test.ts".to_string(),
            ]
        );

        let reverse_export_paths = plan
            .reverse_exports_by_symbol
            .get(&invalidate_session)
            .unwrap()
            .iter()
            .filter_map(|symbol_id| plan.symbol_to_file.get(symbol_id))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            reverse_export_paths,
            vec!["packages/auth/src/index.ts".to_string()]
        );

        assert_eq!(
            plan.package_by_file
                .get("packages/api/src/routes/logout.ts")
                .unwrap()
                .package_name,
            "@hyperindex/api"
        );
        assert_eq!(
            plan.package_by_symbol
                .get(&invalidate_session)
                .unwrap()
                .package_name,
            "@hyperindex/auth"
        );
    }

    #[test]
    fn test_associations_are_conservative_and_explainable() {
        let plan = build_fixture_plan();
        let invalidate_session = canonical_symbol_id(
            &plan,
            "packages/auth/src/session/service.ts",
            "invalidateSession",
        );

        let service_tests = plan
            .tests_by_file
            .get("packages/auth/src/session/service.ts")
            .unwrap();
        assert!(service_tests.iter().any(|association| {
            association.test_path == "packages/auth/tests/session/service.test.ts"
                && association.evidence == ImpactTestAssociationEvidence::PathHeuristic
                && association.detail.contains("same-package source filename")
        }));
        assert!(service_tests.iter().any(|association| {
            association.test_path == "packages/auth/tests/session/session-callers.test.ts"
                && association.evidence == ImpactTestAssociationEvidence::ImportsFile
        }));
        assert!(!service_tests.iter().any(|association| {
            association.test_path == "packages/api/tests/logout-route.test.ts"
        }));

        let symbol_tests = plan.tests_by_symbol.get(&invalidate_session).unwrap();
        assert!(symbol_tests.iter().any(|association| {
            association.test_path == "packages/auth/tests/session/session-callers.test.ts"
                && association.evidence == ImpactTestAssociationEvidence::ReferencesSymbol
                && association.detail.contains("invalidateSession")
        }));
        assert!(!symbol_tests.iter().any(|association| {
            association.test_path == "packages/auth/tests/session/service.test.ts"
        }));
    }

    #[test]
    fn unsupported_edge_types_are_explicitly_deferred() {
        let plan = build_fixture_plan();
        let deferred = plan
            .audit
            .deferred_features
            .iter()
            .map(|feature| feature.kind.clone())
            .collect::<Vec<_>>();

        assert_eq!(
            deferred,
            vec![
                ImpactDeferredFeatureKind::ConfigKey,
                ImpactDeferredFeatureKind::Route,
                ImpactDeferredFeatureKind::Api,
            ]
        );
    }
}
