use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::symbols::{
    FileFacts as ProtocolFileFacts, GraphEdge, GraphEdgeKind, GraphNodeRef,
    SymbolOccurrence as ProtocolSymbolOccurrence, SymbolRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::facts::{FactsBatch, SymbolFactRecord};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolSpanIndex {
    pub symbol_id: String,
    pub start_byte: u32,
    pub end_byte: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolGraph {
    pub indexed_files: usize,
    pub symbol_count: usize,
    pub occurrence_count: usize,
    pub node_count: usize,
    pub edge_count: usize,
    pub stage: String,
    pub files: Vec<ProtocolFileFacts>,
    pub symbols: BTreeMap<String, SymbolRecord>,
    pub symbol_facts: BTreeMap<String, SymbolFactRecord>,
    pub occurrences: BTreeMap<String, ProtocolSymbolOccurrence>,
    pub occurrences_by_symbol: BTreeMap<String, Vec<ProtocolSymbolOccurrence>>,
    pub occurrences_by_file: BTreeMap<String, Vec<ProtocolSymbolOccurrence>>,
    pub symbol_ids_by_name: BTreeMap<String, Vec<String>>,
    pub symbol_ids_by_lower_name: BTreeMap<String, Vec<String>>,
    pub symbol_ids_by_file: BTreeMap<String, Vec<String>>,
    pub symbol_ids_by_span: BTreeMap<String, Vec<SymbolSpanIndex>>,
    pub outgoing_edges: BTreeMap<String, Vec<GraphEdge>>,
    pub incoming_edges: BTreeMap<String, Vec<GraphEdge>>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Default, Clone)]
pub struct SymbolGraphBuilder;

impl SymbolGraphBuilder {
    pub fn build(&self, facts: &FactsBatch) -> SymbolGraph {
        self.build_internal(facts, None)
    }

    pub fn build_with_snapshot(
        &self,
        facts: &FactsBatch,
        snapshot: &ComposedSnapshot,
    ) -> SymbolGraph {
        self.build_internal(facts, Some(snapshot))
    }

    fn build_internal(
        &self,
        facts: &FactsBatch,
        snapshot: Option<&ComposedSnapshot>,
    ) -> SymbolGraph {
        let mut files = Vec::new();
        let mut symbols = BTreeMap::new();
        let mut symbol_facts = BTreeMap::new();
        let mut occurrences = BTreeMap::new();
        let mut occurrences_by_symbol = BTreeMap::new();
        let mut occurrences_by_file = BTreeMap::new();
        let mut symbol_ids_by_name = BTreeMap::new();
        let mut symbol_ids_by_lower_name = BTreeMap::new();
        let mut symbol_ids_by_file = BTreeMap::new();
        let mut symbol_ids_by_span = BTreeMap::new();
        let mut module_symbols_by_file = BTreeMap::new();
        let mut edge_map = BTreeMap::new();
        let mut exported_symbols_by_file = BTreeMap::<String, BTreeMap<String, Vec<String>>>::new();

        for file in &facts.files {
            files.push(file.facts.clone());
            for symbol in &file.facts.symbols {
                let symbol_id = symbol.symbol_id.0.clone();
                symbols.insert(symbol_id.clone(), symbol.clone());
                symbol_ids_by_name
                    .entry(symbol.display_name.clone())
                    .or_insert_with(Vec::new)
                    .push(symbol_id.clone());
                symbol_ids_by_lower_name
                    .entry(symbol.display_name.to_ascii_lowercase())
                    .or_insert_with(Vec::new)
                    .push(symbol_id.clone());
                symbol_ids_by_file
                    .entry(symbol.path.clone())
                    .or_insert_with(Vec::new)
                    .push(symbol_id.clone());
                symbol_ids_by_span
                    .entry(symbol.path.clone())
                    .or_insert_with(Vec::new)
                    .push(SymbolSpanIndex {
                        symbol_id: symbol_id.clone(),
                        start_byte: symbol.span.bytes.start,
                        end_byte: symbol.span.bytes.end,
                    });
            }
            for symbol_fact in &file.symbol_facts {
                if symbol_fact.symbol.kind == hyperindex_protocol::symbols::SymbolKind::Module {
                    module_symbols_by_file.insert(
                        symbol_fact.symbol.path.clone(),
                        symbol_fact.symbol.symbol_id.0.clone(),
                    );
                }
                symbol_facts.insert(symbol_fact.symbol.symbol_id.0.clone(), symbol_fact.clone());
            }
            for occurrence in &file.facts.occurrences {
                occurrences.insert(occurrence.occurrence_id.0.clone(), occurrence.clone());
                occurrences_by_symbol
                    .entry(occurrence.symbol_id.0.clone())
                    .or_insert_with(Vec::new)
                    .push(occurrence.clone());
                occurrences_by_file
                    .entry(occurrence.path.clone())
                    .or_insert_with(Vec::new)
                    .push(occurrence.clone());
            }
            for edge in &file.facts.edges {
                edge_map
                    .entry(edge.edge_id.clone())
                    .or_insert_with(|| edge.clone());
            }
            for export_fact in &file.export_facts {
                exported_symbols_by_file
                    .entry(export_fact.path.clone())
                    .or_default()
                    .entry(export_fact.exported_name.clone())
                    .or_default()
                    .push(export_fact.symbol_id.0.clone());
            }
        }

        for values in symbol_ids_by_name.values_mut() {
            values.sort();
            values.dedup();
        }
        for values in symbol_ids_by_lower_name.values_mut() {
            values.sort();
            values.dedup();
        }
        for values in symbol_ids_by_file.values_mut() {
            values.sort();
            values.dedup();
        }
        for values in symbol_ids_by_span.values_mut() {
            values.sort_by(|left, right| {
                left.start_byte
                    .cmp(&right.start_byte)
                    .then_with(|| left.end_byte.cmp(&right.end_byte))
                    .then_with(|| left.symbol_id.cmp(&right.symbol_id))
            });
        }
        for values in occurrences_by_symbol.values_mut() {
            values.sort_by(|left, right| {
                left.path
                    .cmp(&right.path)
                    .then_with(|| left.span.bytes.start.cmp(&right.span.bytes.start))
                    .then_with(|| left.occurrence_id.0.cmp(&right.occurrence_id.0))
            });
        }
        for values in occurrences_by_file.values_mut() {
            values.sort_by(|left, right| {
                left.span
                    .bytes
                    .start
                    .cmp(&right.span.bytes.start)
                    .then_with(|| left.span.bytes.end.cmp(&right.span.bytes.end))
                    .then_with(|| left.occurrence_id.0.cmp(&right.occurrence_id.0))
            });
        }
        for exported_names in exported_symbols_by_file.values_mut() {
            for values in exported_names.values_mut() {
                values.sort();
                values.dedup();
            }
        }

        let resolver = ModuleResolver::new(symbol_ids_by_file.keys().cloned().collect(), snapshot);
        for file in &facts.files {
            for import_fact in &file.import_facts {
                let Some(target_path) =
                    resolver.resolve(&import_fact.path, &import_fact.module_specifier)
                else {
                    continue;
                };
                insert_edge(
                    &mut edge_map,
                    make_edge(
                        GraphEdgeKind::Imports,
                        GraphNodeRef::File {
                            path: import_fact.path.clone(),
                        },
                        GraphNodeRef::File {
                            path: target_path.clone(),
                        },
                    ),
                );

                let Some(imported_name) = &import_fact.imported_name else {
                    continue;
                };
                if imported_name == "*" {
                    continue;
                }
                let Some(target_symbol_id) =
                    unique_exported_symbol(&exported_symbols_by_file, &target_path, imported_name)
                else {
                    continue;
                };
                insert_edge(
                    &mut edge_map,
                    make_edge(
                        GraphEdgeKind::Imports,
                        GraphNodeRef::Symbol {
                            symbol_id: import_fact.symbol_id.clone(),
                        },
                        GraphNodeRef::Symbol {
                            symbol_id: hyperindex_protocol::symbols::SymbolId(target_symbol_id),
                        },
                    ),
                );
            }

            for export_fact in &file.export_facts {
                let Some(module_specifier) = &export_fact.module_specifier else {
                    continue;
                };
                let Some(target_path) = resolver.resolve(&export_fact.path, module_specifier)
                else {
                    continue;
                };
                insert_edge(
                    &mut edge_map,
                    make_edge(
                        GraphEdgeKind::Imports,
                        GraphNodeRef::File {
                            path: export_fact.path.clone(),
                        },
                        GraphNodeRef::File {
                            path: target_path.clone(),
                        },
                    ),
                );

                let source_name = export_fact
                    .local_name
                    .clone()
                    .unwrap_or_else(|| export_fact.exported_name.clone());
                let Some(target_symbol_id) =
                    unique_exported_symbol(&exported_symbols_by_file, &target_path, &source_name)
                else {
                    continue;
                };
                insert_edge(
                    &mut edge_map,
                    make_edge(
                        GraphEdgeKind::Exports,
                        GraphNodeRef::Symbol {
                            symbol_id: export_fact.symbol_id.clone(),
                        },
                        GraphNodeRef::Symbol {
                            symbol_id: hyperindex_protocol::symbols::SymbolId(target_symbol_id),
                        },
                    ),
                );
            }
        }

        let edges = edge_map.into_values().collect::<Vec<_>>();
        let mut outgoing_edges = BTreeMap::<String, Vec<GraphEdge>>::new();
        let mut incoming_edges = BTreeMap::<String, Vec<GraphEdge>>::new();
        for edge in &edges {
            outgoing_edges
                .entry(node_key(&edge.from))
                .or_insert_with(Vec::new)
                .push(edge.clone());
            incoming_edges
                .entry(node_key(&edge.to))
                .or_insert_with(Vec::new)
                .push(edge.clone());
        }
        for values in outgoing_edges.values_mut() {
            values.sort_by(|left, right| left.edge_id.cmp(&right.edge_id));
        }
        for values in incoming_edges.values_mut() {
            values.sort_by(|left, right| left.edge_id.cmp(&right.edge_id));
        }

        SymbolGraph {
            indexed_files: facts.files.len(),
            symbol_count: symbols.len(),
            occurrence_count: occurrences.len(),
            node_count: files.len() + symbols.len() + occurrences.len(),
            edge_count: edges.len(),
            stage: "resolved_graph".to_string(),
            files,
            symbols,
            symbol_facts,
            occurrences,
            occurrences_by_symbol,
            occurrences_by_file,
            symbol_ids_by_name,
            symbol_ids_by_lower_name,
            symbol_ids_by_file,
            symbol_ids_by_span,
            outgoing_edges,
            incoming_edges,
            edges,
        }
    }
}

#[derive(Debug, Clone)]
struct WorkspacePackage {
    name: String,
    dir: String,
}

#[derive(Debug, Clone)]
struct ModuleResolver {
    source_paths: BTreeSet<String>,
    workspace_packages: Vec<WorkspacePackage>,
}

impl ModuleResolver {
    fn new(source_paths: BTreeSet<String>, snapshot: Option<&ComposedSnapshot>) -> Self {
        let workspace_packages = snapshot.map(read_workspace_packages).unwrap_or_default();
        Self {
            source_paths,
            workspace_packages,
        }
    }

    fn resolve(&self, from_path: &str, specifier: &str) -> Option<String> {
        if specifier.starts_with("./") || specifier.starts_with("../") {
            let base_dir = Path::new(from_path)
                .parent()
                .unwrap_or_else(|| Path::new(""));
            let joined = base_dir.join(specifier);
            let normalized = normalize_repo_path(&joined)?;
            return self.resolve_candidate(&normalized);
        }

        self.resolve_workspace_package(specifier)
    }

    fn resolve_workspace_package(&self, specifier: &str) -> Option<String> {
        let package = self
            .workspace_packages
            .iter()
            .filter(|package| {
                specifier == package.name
                    || specifier
                        .strip_prefix(&format!("{}/", package.name))
                        .is_some()
            })
            .max_by_key(|package| package.name.len())?;
        let subpath = specifier
            .strip_prefix(&package.name)
            .unwrap_or("")
            .trim_start_matches('/');
        let mut candidates = Vec::new();
        if subpath.is_empty() {
            candidates.push(format!("{}/src/index", package.dir));
            candidates.push(format!("{}/index", package.dir));
        } else {
            candidates.push(format!("{}/{}", package.dir, subpath));
            candidates.push(format!("{}/{}/index", package.dir, subpath));
            candidates.push(format!("{}/src/{}", package.dir, subpath));
            candidates.push(format!("{}/src/{}/index", package.dir, subpath));
        }
        candidates
            .into_iter()
            .find_map(|candidate| self.resolve_candidate(&candidate))
    }

    fn resolve_candidate(&self, candidate: &str) -> Option<String> {
        let candidate = candidate.trim_start_matches("./");
        if self.source_paths.contains(candidate) {
            return Some(candidate.to_string());
        }

        let path = Path::new(candidate);
        if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
            for swapped in source_extension_variants(candidate, extension) {
                if self.source_paths.contains(&swapped) {
                    return Some(swapped);
                }
            }
        } else {
            for extension in SOURCE_EXTENSIONS {
                let file_candidate = format!("{candidate}.{extension}");
                if self.source_paths.contains(&file_candidate) {
                    return Some(file_candidate);
                }
            }
        }

        for extension in SOURCE_EXTENSIONS {
            let index_candidate = format!("{candidate}/index.{extension}");
            if self.source_paths.contains(&index_candidate) {
                return Some(index_candidate);
            }
        }
        None
    }
}

const SOURCE_EXTENSIONS: [&str; 6] = ["ts", "tsx", "js", "jsx", "mts", "cts"];

fn unique_exported_symbol(
    exported_symbols_by_file: &BTreeMap<String, BTreeMap<String, Vec<String>>>,
    path: &str,
    exported_name: &str,
) -> Option<String> {
    let values = exported_symbols_by_file.get(path)?.get(exported_name)?;
    (values.len() == 1).then(|| values[0].clone())
}

fn insert_edge(edges: &mut BTreeMap<String, GraphEdge>, edge: GraphEdge) {
    edges.entry(edge.edge_id.clone()).or_insert(edge);
}

pub(crate) fn node_key(node: &GraphNodeRef) -> String {
    match node {
        GraphNodeRef::Symbol { symbol_id } => format!("symbol:{}", symbol_id.0),
        GraphNodeRef::Occurrence { occurrence_id } => format!("occurrence:{}", occurrence_id.0),
        GraphNodeRef::File { path } => format!("file:{path}"),
    }
}

fn make_edge(kind: GraphEdgeKind, from: GraphNodeRef, to: GraphNodeRef) -> GraphEdge {
    let digest = format!(
        "{:x}",
        Sha256::digest(format!("{kind:?}\n{from:?}\n{to:?}").as_bytes())
    );
    let edge_name = match kind {
        GraphEdgeKind::Contains => "contains",
        GraphEdgeKind::Defines => "defines",
        GraphEdgeKind::References => "references",
        GraphEdgeKind::Imports => "imports",
        GraphEdgeKind::Exports => "exports",
    };
    GraphEdge {
        edge_id: format!("edge.{edge_name}.{}", &digest[..12]),
        kind,
        from,
        to,
    }
}

fn read_workspace_packages(snapshot: &ComposedSnapshot) -> Vec<WorkspacePackage> {
    let assembler = hyperindex_snapshot::SnapshotAssembler;
    let mut candidate_paths = BTreeSet::new();
    for file in &snapshot.base.files {
        if file.path == "package.json" || file.path.ends_with("/package.json") {
            candidate_paths.insert(file.path.clone());
        }
    }
    for entry in &snapshot.working_tree.entries {
        if entry.path == "package.json" || entry.path.ends_with("/package.json") {
            candidate_paths.insert(entry.path.clone());
        }
    }
    for buffer in &snapshot.buffers {
        if buffer.path == "package.json" || buffer.path.ends_with("/package.json") {
            candidate_paths.insert(buffer.path.clone());
        }
    }

    let mut packages = candidate_paths
        .into_iter()
        .filter_map(|path| {
            let contents = assembler.resolve_file(snapshot, &path)?.contents;
            let package_name = serde_json::from_str::<Value>(&contents)
                .ok()?
                .get("name")?
                .as_str()?
                .to_string();
            let dir = Path::new(&path)
                .parent()
                .map(PathBuf::from)
                .unwrap_or_default();
            Some(WorkspacePackage {
                name: package_name,
                dir: normalize_repo_path(&dir).unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.dir.cmp(&right.dir))
    });
    packages
}

fn normalize_repo_path(path: &Path) -> Option<String> {
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

fn source_extension_variants(candidate: &str, extension: &str) -> Vec<String> {
    let Some(stem) = candidate.strip_suffix(&format!(".{extension}")) else {
        return Vec::new();
    };
    match extension {
        "js" => vec![format!("{stem}.ts"), format!("{stem}.tsx")],
        "jsx" => vec![format!("{stem}.tsx"), format!("{stem}.ts")],
        "mjs" => vec![format!("{stem}.mts"), format!("{stem}.ts")],
        "cjs" => vec![format!("{stem}.cts"), format!("{stem}.ts")],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::symbols::GraphEdgeKind;

    use crate::SymbolWorkspace;

    use super::{SymbolGraphBuilder, node_key};
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, SnapshotFile, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

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
    fn resolves_relative_imports_and_cross_file_references() {
        let snapshot = snapshot(vec![
            (
                "src/api.ts",
                r#"
                export function createSession() {
                  return 1;
                }

                export default function invalidateSession() {
                  return createSession();
                }
                "#,
            ),
            (
                "src/consumer.ts",
                r#"
                import invalidateSession, { createSession } from "./api";

                export function run() {
                  createSession();
                  return invalidateSession();
                }
                "#,
            ),
        ]);
        let mut workspace = SymbolWorkspace::default();
        let index = workspace.prepare_snapshot(&snapshot).unwrap();
        let graph = &index.graph;

        let file_edges = graph
            .outgoing_edges
            .get("file:src/consumer.ts")
            .unwrap()
            .iter()
            .filter(|edge| edge.kind == GraphEdgeKind::Imports)
            .count();
        assert_eq!(file_edges, 1);

        let create_session_source = graph
            .symbol_ids_by_file
            .get("src/api.ts")
            .unwrap()
            .iter()
            .filter_map(|symbol_id| graph.symbols.get(symbol_id))
            .find(|symbol| symbol.display_name == "createSession")
            .unwrap();
        let source_key = node_key(&hyperindex_protocol::symbols::GraphNodeRef::Symbol {
            symbol_id: create_session_source.symbol_id.clone(),
        });
        let imported_bindings = graph
            .incoming_edges
            .get(&source_key)
            .unwrap()
            .iter()
            .filter(|edge| edge.kind == GraphEdgeKind::Imports)
            .count();
        assert_eq!(imported_bindings, 1);

        let query = workspace.query_engine();
        let references = query.references(graph, &create_session_source.symbol_id.0);
        assert!(references.iter().any(|occurrence| {
            occurrence.path == "src/consumer.ts" && occurrence.role == "Import"
        }));
        assert!(references.iter().any(|occurrence| {
            occurrence.path == "src/consumer.ts" && occurrence.role == "Reference"
        }));
        assert!(references.iter().any(|occurrence| {
            occurrence.path == "src/api.ts" && occurrence.role == "Reference"
        }));
    }

    #[test]
    fn resolves_workspace_package_names_from_package_metadata() {
        let snapshot = snapshot(vec![
            ("package.json", r#"{ "name": "repo-root" }"#),
            (
                "packages/auth/package.json",
                r#"{ "name": "@hyperindex/auth" }"#,
            ),
            (
                "packages/auth/src/index.ts",
                r#"
                export function createSession() {
                  return 1;
                }
                "#,
            ),
            (
                "apps/web/src/main.ts",
                r#"
                import { createSession } from "@hyperindex/auth";

                export function run() {
                  return createSession();
                }
                "#,
            ),
        ]);
        let mut workspace = SymbolWorkspace::default();
        let index = workspace.prepare_snapshot(&snapshot).unwrap();

        let imports = index
            .graph
            .outgoing_edges
            .get("file:apps/web/src/main.ts")
            .unwrap()
            .iter()
            .filter(|edge| edge.kind == GraphEdgeKind::Imports)
            .count();
        assert_eq!(imports, 1);
    }

    #[test]
    fn graph_builder_rebuilds_identically_from_facts() {
        let snapshot = snapshot(vec![
            (
                "src/lib.ts",
                r#"
                export function createSession() {
                  return 1;
                }
                "#,
            ),
            (
                "src/main.ts",
                r#"
                import { createSession } from "./lib";
                export function run() {
                  return createSession();
                }
                "#,
            ),
        ]);
        let mut workspace = SymbolWorkspace::default();
        let index = workspace.prepare_snapshot(&snapshot).unwrap();
        let rebuilt = SymbolGraphBuilder::default().build_with_snapshot(&index.facts, &snapshot);

        assert_eq!(rebuilt, index.graph);
    }
}
