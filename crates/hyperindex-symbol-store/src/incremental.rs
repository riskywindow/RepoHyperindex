use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use hyperindex_parser::{ParseCore, ParseCoreSettings, ParseEligibilityRules, SnapshotFileCatalog};
use hyperindex_protocol::snapshot::{ComposedSnapshot, SnapshotDiffResponse};
use hyperindex_protocol::symbols::{GraphEdge, GraphNodeRef, SymbolOccurrence};
use hyperindex_symbols::{
    ExtractedFileFacts, FactWorkspace, FactsBatch, SymbolGraph, SymbolGraphBuilder,
};
use sha2::{Digest, Sha256};

use crate::SymbolStoreResult;
use crate::migrations::SYMBOL_STORE_SCHEMA_VERSION;
use crate::symbol_store::{IndexedSnapshotState, SymbolStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalIndexOptions {
    pub store_root: PathBuf,
    pub rules: ParseEligibilityRules,
    pub diagnostics_max_per_file: usize,
}

impl Default for IncrementalIndexOptions {
    fn default() -> Self {
        Self {
            store_root: PathBuf::from(".hyperindex/data/symbols"),
            rules: ParseEligibilityRules::default(),
            diagnostics_max_per_file: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncrementalRefreshMode {
    FullRebuild,
    Incremental,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncrementalRefreshTrigger {
    Bootstrap,
    SnapshotDiff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildFallbackReason {
    NoPriorSnapshot,
    MissingSnapshotDiff,
    SchemaVersionChanged,
    IncompatibleConfigChange,
    CacheOrIndexCorruption,
    UnresolvedConsistencyIssue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalRefreshStats {
    pub mode: IncrementalRefreshMode,
    pub trigger: IncrementalRefreshTrigger,
    pub files_reparsed: u64,
    pub files_reused: u64,
    pub symbols_updated: u64,
    pub edges_updated: u64,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct IncrementalRefreshResult {
    pub repo_id: String,
    pub snapshot_id: String,
    pub parser_config_digest: String,
    pub fallback_reason: Option<RebuildFallbackReason>,
    pub stats: IncrementalRefreshStats,
    pub facts: FactsBatch,
    pub graph: SymbolGraph,
}

#[derive(Debug, Clone)]
pub struct IncrementalSymbolIndexer {
    store: SymbolStore,
    parser: ParseCore,
    facts: FactWorkspace,
    graph: SymbolGraphBuilder,
    options: IncrementalIndexOptions,
    parser_config_digest: String,
}

impl IncrementalSymbolIndexer {
    pub fn open(
        root: &Path,
        repo_id: &str,
        options: IncrementalIndexOptions,
    ) -> SymbolStoreResult<Self> {
        let store_root = if options.store_root.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            options.store_root.clone()
        };
        Ok(Self {
            store: SymbolStore::open(&store_root, repo_id)?,
            parser: ParseCore::with_settings(ParseCoreSettings {
                diagnostics_max_per_file: options.diagnostics_max_per_file,
            }),
            facts: FactWorkspace,
            graph: SymbolGraphBuilder,
            parser_config_digest: parser_config_digest(&options),
            options,
        })
    }

    pub fn refresh(
        &mut self,
        previous_snapshot: Option<&ComposedSnapshot>,
        snapshot: &ComposedSnapshot,
        diff: Option<&SnapshotDiffResponse>,
    ) -> SymbolStoreResult<IncrementalRefreshResult> {
        let started = Instant::now();
        let trigger = if previous_snapshot.is_some() || diff.is_some() {
            IncrementalRefreshTrigger::SnapshotDiff
        } else {
            IncrementalRefreshTrigger::Bootstrap
        };

        let plan = self.plan_refresh(previous_snapshot, snapshot, diff)?;
        match plan {
            RefreshPlan::Incremental { diff, prior_batch } => {
                self.run_incremental(&started, trigger, snapshot, diff, prior_batch)
            }
            RefreshPlan::Full(fallback_reason) => {
                self.run_full_rebuild(&started, trigger, snapshot, Some(fallback_reason))
            }
        }
    }

    fn plan_refresh(
        &self,
        previous_snapshot: Option<&ComposedSnapshot>,
        snapshot: &ComposedSnapshot,
        diff: Option<&SnapshotDiffResponse>,
    ) -> SymbolStoreResult<RefreshPlan> {
        let Some(previous_snapshot) = previous_snapshot else {
            return Ok(RefreshPlan::Full(RebuildFallbackReason::NoPriorSnapshot));
        };
        let Some(diff) = diff.cloned() else {
            return Ok(RefreshPlan::Full(
                RebuildFallbackReason::MissingSnapshotDiff,
            ));
        };
        if diff.left_snapshot_id != previous_snapshot.snapshot_id
            || diff.right_snapshot_id != snapshot.snapshot_id
            || previous_snapshot.repo_id != snapshot.repo_id
        {
            return Ok(RefreshPlan::Full(
                RebuildFallbackReason::UnresolvedConsistencyIssue,
            ));
        }

        let Some(indexed_state) = self
            .store
            .load_indexed_snapshot_state(&previous_snapshot.snapshot_id)?
        else {
            return Ok(RefreshPlan::Full(RebuildFallbackReason::NoPriorSnapshot));
        };
        if indexed_state.repo_id != snapshot.repo_id {
            return Ok(RefreshPlan::Full(
                RebuildFallbackReason::UnresolvedConsistencyIssue,
            ));
        }
        if indexed_state.schema_version != SYMBOL_STORE_SCHEMA_VERSION as u32 {
            return Ok(RefreshPlan::Full(
                RebuildFallbackReason::SchemaVersionChanged,
            ));
        }
        if indexed_state.parser_config_digest != self.parser_config_digest {
            return Ok(RefreshPlan::Full(
                RebuildFallbackReason::IncompatibleConfigChange,
            ));
        }

        let prior_batch = match self
            .store
            .load_snapshot_facts(&previous_snapshot.snapshot_id)
        {
            Ok(snapshot) => FactsBatch {
                files: snapshot.files,
            },
            Err(_) => {
                return Ok(RefreshPlan::Full(
                    RebuildFallbackReason::CacheOrIndexCorruption,
                ));
            }
        };

        let previous_catalog = SnapshotFileCatalog::build(previous_snapshot, &self.options.rules);
        let previous_expected = previous_catalog
            .eligible_files
            .iter()
            .map(|file| {
                (
                    file.path.clone(),
                    (file.content_sha256.clone(), file.content_bytes as u64),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let previous_indexed = prior_batch
            .files
            .iter()
            .map(|file| {
                (
                    file.facts.path.clone(),
                    (
                        file.artifact.content_sha256.clone(),
                        file.artifact.content_bytes,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if previous_expected != previous_indexed
            || indexed_state.indexed_file_count != prior_batch.files.len()
        {
            return Ok(RefreshPlan::Full(
                RebuildFallbackReason::UnresolvedConsistencyIssue,
            ));
        }

        Ok(RefreshPlan::Incremental { diff, prior_batch })
    }

    fn run_full_rebuild(
        &mut self,
        started: &Instant,
        trigger: IncrementalRefreshTrigger,
        snapshot: &ComposedSnapshot,
        fallback_reason: Option<RebuildFallbackReason>,
    ) -> SymbolStoreResult<IncrementalRefreshResult> {
        let catalog = SnapshotFileCatalog::build(snapshot, &self.options.rules);
        let mut artifacts = Vec::new();
        for file in &catalog.eligible_files {
            artifacts.push(
                self.parser
                    .parse_contents(file.to_candidate(), file.contents.clone())?,
            );
        }
        let facts = self
            .facts
            .extract(&snapshot.repo_id, &snapshot.snapshot_id, &artifacts);
        let graph = self.graph.build_with_snapshot(&facts, snapshot);
        self.store
            .persist_facts(&snapshot.repo_id, &snapshot.snapshot_id, &facts)?;
        self.store
            .record_indexed_snapshot_state(&IndexedSnapshotState {
                repo_id: snapshot.repo_id.clone(),
                snapshot_id: snapshot.snapshot_id.clone(),
                parser_config_digest: self.parser_config_digest.clone(),
                schema_version: SYMBOL_STORE_SCHEMA_VERSION as u32,
                indexed_file_count: facts.files.len(),
                refresh_mode: "full_rebuild".to_string(),
            })?;

        Ok(IncrementalRefreshResult {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            parser_config_digest: self.parser_config_digest.clone(),
            fallback_reason,
            stats: IncrementalRefreshStats {
                mode: IncrementalRefreshMode::FullRebuild,
                trigger,
                files_reparsed: facts.files.len() as u64,
                files_reused: 0,
                symbols_updated: facts.symbol_count() as u64,
                edges_updated: graph.edge_count as u64,
                elapsed_ms: started.elapsed().as_millis(),
            },
            facts,
            graph,
        })
    }

    fn run_incremental(
        &mut self,
        started: &Instant,
        trigger: IncrementalRefreshTrigger,
        snapshot: &ComposedSnapshot,
        diff: SnapshotDiffResponse,
        prior_batch: FactsBatch,
    ) -> SymbolStoreResult<IncrementalRefreshResult> {
        let current_catalog = SnapshotFileCatalog::build(snapshot, &self.options.rules);
        let current_files = current_catalog
            .eligible_files
            .iter()
            .map(|file| (file.path.clone(), file))
            .collect::<BTreeMap<_, _>>();
        let prior_files = prior_batch
            .files
            .iter()
            .map(|file| (file.facts.path.clone(), file))
            .collect::<BTreeMap<_, _>>();
        let changed_paths = diff
            .changed_paths
            .into_iter()
            .filter(|path| current_files.contains_key(path) || prior_files.contains_key(path))
            .collect::<BTreeSet<_>>();

        let mut next_files = BTreeMap::<String, ExtractedFileFacts>::new();
        let mut files_reparsed = 0u64;
        let mut files_reused = 0u64;

        for (path, file) in &current_files {
            if changed_paths.contains(path) {
                let artifact = self
                    .parser
                    .parse_contents(file.to_candidate(), file.contents.clone())?;
                let extracted =
                    self.facts
                        .extract(&snapshot.repo_id, &snapshot.snapshot_id, &[artifact]);
                next_files.insert(path.clone(), extracted.files.into_iter().next().unwrap());
                files_reparsed += 1;
                continue;
            }

            let Some(previous) = prior_files.get(path) else {
                return self.run_full_rebuild(
                    started,
                    trigger,
                    snapshot,
                    Some(RebuildFallbackReason::UnresolvedConsistencyIssue),
                );
            };
            if previous.artifact.content_sha256 != file.content_sha256
                || previous.artifact.content_bytes != file.content_bytes as u64
            {
                return self.run_full_rebuild(
                    started,
                    trigger,
                    snapshot,
                    Some(RebuildFallbackReason::UnresolvedConsistencyIssue),
                );
            }
            next_files.insert(
                path.clone(),
                previous.rebind_snapshot(&snapshot.snapshot_id),
            );
            files_reused += 1;
        }

        let facts = FactsBatch {
            files: next_files.into_values().collect(),
        };
        let graph = self.graph.build_with_snapshot(&facts, snapshot);
        self.store
            .persist_facts(&snapshot.repo_id, &snapshot.snapshot_id, &facts)?;
        self.store
            .record_indexed_snapshot_state(&IndexedSnapshotState {
                repo_id: snapshot.repo_id.clone(),
                snapshot_id: snapshot.snapshot_id.clone(),
                parser_config_digest: self.parser_config_digest.clone(),
                schema_version: SYMBOL_STORE_SCHEMA_VERSION as u32,
                indexed_file_count: facts.files.len(),
                refresh_mode: "incremental".to_string(),
            })?;

        Ok(IncrementalRefreshResult {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            parser_config_digest: self.parser_config_digest.clone(),
            fallback_reason: None,
            stats: IncrementalRefreshStats {
                mode: IncrementalRefreshMode::Incremental,
                trigger,
                files_reparsed,
                files_reused,
                symbols_updated: updated_symbol_count(&prior_batch, &facts) as u64,
                edges_updated: updated_edge_count(&prior_batch, &facts) as u64,
                elapsed_ms: started.elapsed().as_millis(),
            },
            facts,
            graph,
        })
    }
}

enum RefreshPlan {
    Full(RebuildFallbackReason),
    Incremental {
        diff: SnapshotDiffResponse,
        prior_batch: FactsBatch,
    },
}

trait ResolvedParseFileExt {
    fn to_candidate(&self) -> hyperindex_parser::ParseCandidate;
}

impl ResolvedParseFileExt for hyperindex_parser::ResolvedParseFile {
    fn to_candidate(&self) -> hyperindex_parser::ParseCandidate {
        hyperindex_parser::ParseCandidate {
            path: self.path.clone(),
            language: self.language,
            source_kind: self.source_kind.clone(),
            content_sha256: self.content_sha256.clone(),
            content_bytes: self.content_bytes,
        }
    }
}

fn parser_config_digest(options: &IncrementalIndexOptions) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"phase4-symbol-incremental-v1\n");
    hasher.update(format!("max_file_bytes:{}\n", options.rules.max_file_bytes));
    hasher.update(format!(
        "diagnostics_max_per_file:{}\n",
        options.diagnostics_max_per_file
    ));
    hasher.update(format!(
        "exclude_vendor_paths:{}\n",
        options.rules.exclude_vendor_paths
    ));
    hasher.update(format!(
        "exclude_generated_paths:{}\n",
        options.rules.exclude_generated_paths
    ));
    hasher.update(format!(
        "exclude_binary_like_contents:{}\n",
        options.rules.exclude_binary_like_contents
    ));
    let mut ignore_patterns = options.rules.ignore_patterns.clone();
    ignore_patterns.sort();
    for pattern in ignore_patterns {
        hasher.update(format!("ignore:{pattern}\n"));
    }
    let mut languages = options
        .rules
        .enabled_languages
        .iter()
        .map(|language| format!("{language:?}"))
        .collect::<Vec<_>>();
    languages.sort();
    for language in languages {
        hasher.update(format!("language:{language}\n"));
    }
    format!("{:x}", hasher.finalize())
}

fn updated_symbol_count(previous: &FactsBatch, current: &FactsBatch) -> usize {
    let previous = previous
        .files
        .iter()
        .flat_map(|file| file.symbol_facts.iter())
        .map(|record| {
            (
                record.symbol.symbol_id.0.clone(),
                serde_json::to_string(record).unwrap_or_default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let current = current
        .files
        .iter()
        .flat_map(|file| file.symbol_facts.iter())
        .map(|record| {
            (
                record.symbol.symbol_id.0.clone(),
                serde_json::to_string(record).unwrap_or_default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    symmetric_difference_count(&previous, &current)
}

fn updated_edge_count(previous: &FactsBatch, current: &FactsBatch) -> usize {
    let previous_occurrences = batch_occurrences(previous);
    let current_occurrences = batch_occurrences(current);
    let previous = batch_edges(previous)
        .into_iter()
        .map(|edge| canonical_edge_key(&edge, &previous_occurrences))
        .collect::<BTreeSet<_>>();
    let current = batch_edges(current)
        .into_iter()
        .map(|edge| canonical_edge_key(&edge, &current_occurrences))
        .collect::<BTreeSet<_>>();
    previous.symmetric_difference(&current).count()
}

fn batch_occurrences(batch: &FactsBatch) -> BTreeMap<String, SymbolOccurrence> {
    batch
        .files
        .iter()
        .flat_map(|file| file.facts.occurrences.iter())
        .map(|occurrence| (occurrence.occurrence_id.0.clone(), occurrence.clone()))
        .collect()
}

fn batch_edges(batch: &FactsBatch) -> Vec<GraphEdge> {
    batch
        .files
        .iter()
        .flat_map(|file| file.facts.edges.iter().cloned())
        .collect()
}

fn symmetric_difference_count(
    previous: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> usize {
    let keys = previous
        .keys()
        .chain(current.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    keys.into_iter()
        .filter(|key| previous.get(key) != current.get(key))
        .count()
}

fn canonical_edge_key(
    edge: &GraphEdge,
    occurrences: &BTreeMap<String, SymbolOccurrence>,
) -> String {
    format!(
        "{:?}|{}|{}",
        edge.kind,
        canonical_node_ref(&edge.from, occurrences),
        canonical_node_ref(&edge.to, occurrences)
    )
}

fn canonical_node_ref(
    node: &GraphNodeRef,
    occurrences: &BTreeMap<String, SymbolOccurrence>,
) -> String {
    match node {
        GraphNodeRef::File { path } => format!("file:{path}"),
        GraphNodeRef::Symbol { symbol_id } => format!("symbol:{}", symbol_id.0),
        GraphNodeRef::Occurrence { occurrence_id } => occurrences
            .get(&occurrence_id.0)
            .map(|occurrence| {
                format!(
                    "occ:{}:{}:{:?}:{}:{}-{}:{}",
                    occurrence.path,
                    occurrence.symbol_id.0,
                    occurrence.role,
                    occurrence.span.start.line,
                    occurrence.span.start.column,
                    occurrence.span.end.line,
                    occurrence.span.end.column
                )
            })
            .unwrap_or_else(|| format!("occ:{}", occurrence_id.0)),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use hyperindex_parser::{ParseCore, ParseEligibilityRules, ParseManager, ParseManagerOptions};
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, BufferOverlay, ComposedSnapshot, OverlayEntryKind,
        SnapshotFile, WorkingTreeEntry, WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::{SymbolSearchMode, SymbolSearchQuery};
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        IncrementalIndexOptions, IncrementalRefreshMode, IncrementalSymbolIndexer,
        RebuildFallbackReason,
    };
    use crate::migrations::SYMBOL_STORE_SCHEMA_VERSION;
    use hyperindex_snapshot::SnapshotAssembler;
    use hyperindex_symbols::{FactWorkspace, SymbolGraphBuilder, SymbolQueryEngine};

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

    fn baseline_snapshot() -> ComposedSnapshot {
        snapshot(
            "snap-base",
            vec![
                (
                    "src/lib.ts",
                    r#"
                    export function createSession() {
                      return 1;
                    }
                    "#,
                ),
                (
                    "src/api.ts",
                    r#"
                    import { createSession } from "./lib";
                    export function invalidateSession() {
                      return createSession();
                    }
                    "#,
                ),
                (
                    "src/view.ts",
                    r#"
                    import { invalidateSession } from "./api";
                    export function render() {
                      return invalidateSession();
                    }
                    "#,
                ),
            ],
            Vec::new(),
            Vec::new(),
        )
    }

    #[test]
    fn single_file_edit_uses_incremental_refresh_and_matches_full_rebuild() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();
        let base = baseline_snapshot();
        let edited = snapshot(
            "snap-edited",
            vec![
                (
                    "src/lib.ts",
                    r#"
                    export function createSession() {
                      return 2;
                    }
                    "#,
                ),
                (
                    "src/api.ts",
                    r#"
                    import { createSession } from "./lib";
                    export function invalidateSession() {
                      return createSession();
                    }
                    "#,
                ),
                (
                    "src/view.ts",
                    r#"
                    import { invalidateSession } from "./api";
                    export function render() {
                      return invalidateSession();
                    }
                    "#,
                ),
            ],
            Vec::new(),
            Vec::new(),
        );
        let diff = SnapshotAssembler.diff(&base, &edited);

        let mut incremental = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root: root.join("symbols"),
                ..Default::default()
            },
        )
        .unwrap();
        let baseline = incremental.refresh(None, &base, None).unwrap();
        let refreshed = incremental
            .refresh(Some(&base), &edited, Some(&diff))
            .unwrap();

        let mut full = IncrementalSymbolIndexer::open(
            &root.join("full"),
            "repo-1",
            IncrementalIndexOptions {
                store_root: root.join("full-symbols"),
                ..Default::default()
            },
        )
        .unwrap();
        let rebuilt = full.refresh(None, &edited, None).unwrap();

        assert_eq!(baseline.stats.files_reparsed, 3);
        assert_eq!(refreshed.stats.mode, IncrementalRefreshMode::Incremental);
        assert_eq!(refreshed.stats.files_reparsed, 1);
        assert_eq!(refreshed.stats.files_reused, 2);
        assert!(refreshed.stats.files_reparsed < rebuilt.stats.files_reparsed);
        assert_eq!(refreshed.facts, rebuilt.facts);
        assert_eq!(refreshed.graph, rebuilt.graph);
    }

    #[test]
    fn add_delete_modify_and_buffer_only_flows_produce_correct_snapshot() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();
        let base = baseline_snapshot();
        let next = snapshot(
            "snap-next",
            vec![
                (
                    "src/lib.ts",
                    r#"
                    export function createSession() {
                      return 99;
                    }
                    "#,
                ),
                (
                    "src/api.ts",
                    r#"
                    import { createSession } from "./lib";
                    export function invalidateSession() {
                      return createSession();
                    }
                    "#,
                ),
                (
                    "src/new.ts",
                    r#"
                    import { createSession } from "./lib";
                    export function useNewPath() {
                      return createSession();
                    }
                    "#,
                ),
            ],
            Vec::new(),
            Vec::new(),
        );
        let diff = SnapshotAssembler.diff(&base, &next);
        let mut indexer = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root: root.join("symbols"),
                ..Default::default()
            },
        )
        .unwrap();
        indexer.refresh(None, &base, None).unwrap();
        let refreshed = indexer.refresh(Some(&base), &next, Some(&diff)).unwrap();

        let indexed_paths = refreshed
            .facts
            .files
            .iter()
            .map(|file| file.facts.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            indexed_paths,
            vec![
                "src/api.ts".to_string(),
                "src/lib.ts".to_string(),
                "src/new.ts".to_string()
            ]
        );
        assert_eq!(refreshed.stats.files_reparsed, 2);
        assert_eq!(refreshed.stats.files_reused, 1);
        assert!(
            refreshed
                .graph
                .outgoing_edges
                .contains_key("file:src/new.ts")
        );
    }

    #[test]
    fn buffer_overlays_refresh_effective_symbol_results() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();
        let base = snapshot(
            "snap-base",
            vec![(
                "src/service.ts",
                r#"
                export function invalidateSession() {
                  return 1;
                }
                "#,
            )],
            Vec::new(),
            Vec::new(),
        );
        let overlaid = snapshot(
            "snap-buffer",
            vec![(
                "src/service.ts",
                r#"
                export function invalidateSession() {
                  return 1;
                }
                "#,
            )],
            Vec::new(),
            vec![BufferOverlay {
                buffer_id: "buffer-1".to_string(),
                path: "src/service.ts".to_string(),
                version: 1,
                content_sha256: "sha-buffer".to_string(),
                content_bytes: 82,
                contents: r#"
                export function invalidateSessionBuffered() {
                  return 2;
                }
                "#
                .to_string(),
            }],
        );
        let diff = SnapshotAssembler.diff(&base, &overlaid);
        let mut indexer = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root: root.join("symbols"),
                ..Default::default()
            },
        )
        .unwrap();
        indexer.refresh(None, &base, None).unwrap();
        let refreshed = indexer
            .refresh(Some(&base), &overlaid, Some(&diff))
            .unwrap();

        assert_eq!(
            diff.buffer_only_changed_paths,
            vec!["src/service.ts".to_string()]
        );
        let names = refreshed.facts.files[0]
            .facts
            .symbols
            .iter()
            .map(|symbol| symbol.display_name.clone())
            .collect::<Vec<_>>();
        assert!(names.contains(&"invalidateSessionBuffered".to_string()));
        assert!(!names.contains(&"invalidateSession".to_string()));
    }

    #[test]
    fn config_and_store_consistency_failures_fall_back_to_full_rebuild() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();
        let base = baseline_snapshot();
        let edited = snapshot(
            "snap-edited",
            vec![
                (
                    "src/lib.ts",
                    r#"
                    export function createSession(now: number) {
                      return now;
                    }
                    "#,
                ),
                (
                    "src/api.ts",
                    r#"
                    import { createSession } from "./lib";
                    export function invalidateSession() {
                      return createSession(1);
                    }
                    "#,
                ),
                (
                    "src/view.ts",
                    r#"
                    import { invalidateSession } from "./api";
                    export function render() {
                      return invalidateSession();
                    }
                    "#,
                ),
            ],
            Vec::new(),
            Vec::new(),
        );
        let diff = SnapshotAssembler.diff(&base, &edited);
        let store_root = root.join("symbols");

        let mut indexer = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root: store_root.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        indexer.refresh(None, &base, None).unwrap();

        let mut config_changed = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root: store_root.clone(),
                diagnostics_max_per_file: 8,
                ..Default::default()
            },
        )
        .unwrap();
        let config_refresh = config_changed
            .refresh(Some(&base), &edited, Some(&diff))
            .unwrap();
        assert_eq!(
            config_refresh.stats.mode,
            IncrementalRefreshMode::FullRebuild
        );
        assert_eq!(
            config_refresh.fallback_reason,
            Some(RebuildFallbackReason::IncompatibleConfigChange)
        );

        let db_path = store_root.join("repo-1.symbols.sqlite3");
        let connection = Connection::open(db_path).unwrap();
        connection
            .execute(
                "UPDATE indexed_snapshot_state SET schema_version = 1 WHERE snapshot_id = ?1",
                [&base.snapshot_id],
            )
            .unwrap();
        drop(connection);

        let mut schema_changed = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root: store_root.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        let schema_refresh = schema_changed
            .refresh(Some(&base), &edited, Some(&diff))
            .unwrap();
        assert_eq!(
            schema_refresh.stats.mode,
            IncrementalRefreshMode::FullRebuild
        );
        assert_eq!(
            schema_refresh.fallback_reason,
            Some(RebuildFallbackReason::SchemaVersionChanged)
        );

        let connection = Connection::open(store_root.join("repo-1.symbols.sqlite3")).unwrap();
        connection
            .execute(
                "UPDATE indexed_snapshot_state SET schema_version = ?1 WHERE snapshot_id = ?2",
                rusqlite::params![SYMBOL_STORE_SCHEMA_VERSION, &base.snapshot_id],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM indexed_files WHERE snapshot_id = ?1 AND path = 'src/view.ts'",
                [&base.snapshot_id],
            )
            .unwrap();
        drop(connection);

        let mut inconsistent = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root,
                ..Default::default()
            },
        )
        .unwrap();
        let inconsistent_refresh = inconsistent
            .refresh(Some(&base), &edited, Some(&diff))
            .unwrap();
        assert_eq!(
            inconsistent_refresh.fallback_reason,
            Some(RebuildFallbackReason::UnresolvedConsistencyIssue)
        );
    }

    #[test]
    fn index_corruption_falls_back_to_full_rebuild() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();
        let base = baseline_snapshot();
        let edited = snapshot(
            "snap-edited",
            vec![
                (
                    "src/lib.ts",
                    r#"
                    export function createSession() {
                      return 7;
                    }
                    "#,
                ),
                (
                    "src/api.ts",
                    r#"
                    import { createSession } from "./lib";
                    export function invalidateSession() {
                      return createSession();
                    }
                    "#,
                ),
                (
                    "src/view.ts",
                    r#"
                    import { invalidateSession } from "./api";
                    export function render() {
                      return invalidateSession();
                    }
                    "#,
                ),
            ],
            Vec::new(),
            Vec::new(),
        );
        let diff = SnapshotAssembler.diff(&base, &edited);
        let store_root = root.join("symbols");

        let mut indexer = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root: store_root.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        indexer.refresh(None, &base, None).unwrap();

        let connection = Connection::open(store_root.join("repo-1.symbols.sqlite3")).unwrap();
        connection
            .execute(
                "UPDATE indexed_files SET facts_json = '{bad-json' WHERE snapshot_id = ?1",
                [&base.snapshot_id],
            )
            .unwrap();
        drop(connection);

        let mut corrupted = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root,
                ..Default::default()
            },
        )
        .unwrap();
        let refreshed = corrupted
            .refresh(Some(&base), &edited, Some(&diff))
            .unwrap();

        assert_eq!(refreshed.stats.mode, IncrementalRefreshMode::FullRebuild);
        assert_eq!(
            refreshed.fallback_reason,
            Some(RebuildFallbackReason::CacheOrIndexCorruption)
        );
    }

    #[test]
    fn snapshot_diff_with_delete_overlay_still_matches_final_files() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();
        let base = baseline_snapshot();
        let deleted = snapshot(
            "snap-delete",
            vec![
                (
                    "src/lib.ts",
                    r#"
                    export function createSession() {
                      return 1;
                    }
                    "#,
                ),
                (
                    "src/api.ts",
                    r#"
                    import { createSession } from "./lib";
                    export function invalidateSession() {
                      return createSession();
                    }
                    "#,
                ),
                (
                    "src/view.ts",
                    r#"
                    import { invalidateSession } from "./api";
                    export function render() {
                      return invalidateSession();
                    }
                    "#,
                ),
            ],
            vec![WorkingTreeEntry {
                path: "src/view.ts".to_string(),
                kind: OverlayEntryKind::Delete,
                content_sha256: None,
                content_bytes: None,
                contents: None,
            }],
            Vec::new(),
        );
        let diff = SnapshotAssembler.diff(&base, &deleted);
        let mut indexer = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root: root.join("symbols"),
                ..Default::default()
            },
        )
        .unwrap();
        indexer.refresh(None, &base, None).unwrap();
        let refreshed = indexer.refresh(Some(&base), &deleted, Some(&diff)).unwrap();

        let paths = refreshed
            .facts
            .files
            .iter()
            .map(|file| file.facts.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec!["src/api.ts".to_string(), "src/lib.ts".to_string()]
        );
    }

    #[test]
    #[ignore = "profiling smoke for Phase 4 hot paths"]
    fn phase4_hot_path_profile_smoke() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();
        let store_root = root.join("symbols");
        let parse_root = root.join("parse");
        let snapshot = profiled_snapshot("snap-profile-base", 48);
        let warm_snapshot = profiled_snapshot("snap-profile-warm", 48);

        let full_parse_ms = {
            let mut manager = ParseManager::new(ParseManagerOptions {
                store_root: parse_root.clone(),
                rules: ParseEligibilityRules::default(),
                diagnostics_max_per_file: 32,
            });
            let started = Instant::now();
            let build = manager.build_snapshot(&snapshot, false).unwrap();
            assert!(build.stats.parsed_file_count > 0);
            started.elapsed().as_millis()
        };

        let warm_parse_ms = {
            let mut manager = ParseManager::new(ParseManagerOptions {
                store_root: parse_root,
                rules: ParseEligibilityRules::default(),
                diagnostics_max_per_file: 32,
            });
            let started = Instant::now();
            let build = manager.build_snapshot(&warm_snapshot, false).unwrap();
            assert!(build.stats.reused_file_count > 0);
            started.elapsed().as_millis()
        };

        let mut parser = ParseCore::default();
        let parse_plan = parser.parse_snapshot(&snapshot).unwrap();

        let facts_started = Instant::now();
        let facts = FactWorkspace.extract("repo-1", &snapshot.snapshot_id, &parse_plan.artifacts);
        let fact_extraction_ms = facts_started.elapsed().as_millis();

        let graph_started = Instant::now();
        let graph = SymbolGraphBuilder.build_with_snapshot(&facts, &snapshot);
        let graph_construction_ms = graph_started.elapsed().as_millis();

        let query_started = Instant::now();
        let engine = SymbolQueryEngine;
        for _ in 0..200 {
            let hits = engine.search_hits(
                &graph,
                &SymbolSearchQuery {
                    text: "createSession47".to_string(),
                    mode: SymbolSearchMode::Exact,
                    kinds: Vec::new(),
                    path_prefix: None,
                },
                10,
            );
            assert!(!hits.is_empty());
        }
        let symbol_query_ms = query_started.elapsed().as_millis();

        let edited = edited_profiled_snapshot(&snapshot, 24);
        let diff = SnapshotAssembler.diff(&snapshot, &edited);
        let mut indexer = IncrementalSymbolIndexer::open(
            root,
            "repo-1",
            IncrementalIndexOptions {
                store_root,
                ..Default::default()
            },
        )
        .unwrap();
        indexer.refresh(None, &snapshot, None).unwrap();
        let incremental = indexer
            .refresh(Some(&snapshot), &edited, Some(&diff))
            .unwrap();

        eprintln!(
            "phase4_hot_path_profile={}",
            serde_json::to_string(&json!({
                "full_parse_build_ms": full_parse_ms,
                "warm_load_ms": warm_parse_ms,
                "fact_extraction_ms": fact_extraction_ms,
                "graph_construction_ms": graph_construction_ms,
                "symbol_query_200x_ms": symbol_query_ms,
                "incremental_single_file_update_ms": incremental.stats.elapsed_ms,
                "incremental_files_reparsed": incremental.stats.files_reparsed,
                "incremental_files_reused": incremental.stats.files_reused,
                "indexed_files": graph.indexed_files,
                "symbols": graph.symbol_count,
                "occurrences": graph.occurrence_count,
                "edges": graph.edge_count,
            }))
            .unwrap()
        );
    }

    fn profiled_snapshot(snapshot_id: &str, file_count: usize) -> ComposedSnapshot {
        let mut files = Vec::new();
        files.push((
            "package.json".to_string(),
            "{ \"name\": \"profiled-repo\" }".to_string(),
        ));
        for index in 0..file_count {
            let path = format!("src/mod{index}.ts");
            let contents = if index == 0 {
                format!("export function createSession{index}() {{\n  return {index};\n}}\n")
            } else {
                let previous = index - 1;
                format!(
                    "import {{ createSession{previous} }} from \"./mod{previous}\";\nexport function createSession{index}() {{\n  return createSession{previous}();\n}}\n"
                )
            };
            files.push((path, contents));
        }
        let base_files = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.as_str()))
            .collect::<Vec<_>>();
        snapshot(snapshot_id, base_files, Vec::new(), Vec::new())
    }

    fn edited_profiled_snapshot(base: &ComposedSnapshot, target_index: usize) -> ComposedSnapshot {
        let files = base
            .base
            .files
            .iter()
            .map(|file| {
                let contents = if file.path == format!("src/mod{target_index}.ts") {
                    file.contents.replace(
                        &format!("createSession{}();", target_index.saturating_sub(1)),
                        &format!("createSession{}() + 1;", target_index.saturating_sub(1)),
                    )
                } else {
                    file.contents.clone()
                };
                (file.path.clone(), contents)
            })
            .collect::<Vec<_>>();
        let base_files = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.as_str()))
            .collect::<Vec<_>>();
        snapshot("snap-profile-edited", base_files, Vec::new(), Vec::new())
    }
}
