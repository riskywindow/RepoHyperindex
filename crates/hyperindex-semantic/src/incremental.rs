use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use hyperindex_protocol::config::SemanticConfig;
use hyperindex_protocol::semantic::{
    SemanticBuildId, SemanticChunkRecord, SemanticDiagnostic, SemanticDiagnosticSeverity,
    SemanticRefreshStats,
};
use hyperindex_protocol::snapshot::{ComposedSnapshot, SnapshotDiffResponse};
use hyperindex_protocol::symbols::SymbolIndexBuildId;
use hyperindex_semantic_store::{SemanticBuildProfile, SemanticStore, StoredChunkVector};
use hyperindex_symbols::{FactsBatch, SymbolGraph};

use crate::common::{stable_digest, unix_timestamp_string};
use crate::embedding_pipeline::EmbeddingPipeline;
use crate::embedding_provider::provider_from_config;
use crate::semantic_model::truncate_oversized_chunks;
use crate::{SemanticBuildDraft, SemanticResult, SemanticScaffoldBuilder};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticRefreshMode {
    FullRebuild,
    Incremental,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticRefreshTrigger {
    Bootstrap,
    SnapshotDiff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticRebuildFallbackReason {
    NoPriorSnapshot,
    NoPriorBuild,
    MissingSnapshotDiff,
    StoreSchemaVersionChanged,
    ChunkSchemaVersionChanged,
    IncompatibleConfigChange,
    EmbeddingProviderChanged,
    CacheOrIndexCorruption,
    UnresolvedConsistencyIssue,
}

impl SemanticRebuildFallbackReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoPriorSnapshot => "no_prior_snapshot",
            Self::NoPriorBuild => "no_prior_build",
            Self::MissingSnapshotDiff => "missing_snapshot_diff",
            Self::StoreSchemaVersionChanged => "store_schema_version_changed",
            Self::ChunkSchemaVersionChanged => "chunk_schema_version_changed",
            Self::IncompatibleConfigChange => "incompatible_config_change",
            Self::EmbeddingProviderChanged => "embedding_provider_changed",
            Self::CacheOrIndexCorruption => "cache_or_index_corruption",
            Self::UnresolvedConsistencyIssue => "unresolved_consistency_issue",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SemanticRefreshResult {
    pub build: SemanticBuildDraft,
    pub mode: SemanticRefreshMode,
    pub trigger: SemanticRefreshTrigger,
    pub fallback_reason: Option<SemanticRebuildFallbackReason>,
    pub stats: SemanticRefreshStats,
}

#[derive(Debug, Clone)]
pub struct IncrementalSemanticIndexer {
    config: SemanticConfig,
    builder: SemanticScaffoldBuilder,
}

impl IncrementalSemanticIndexer {
    pub fn new(config: &SemanticConfig) -> Self {
        Self {
            config: config.clone(),
            builder: SemanticScaffoldBuilder::from_config(config),
        }
    }

    pub fn refresh(
        &self,
        store: &SemanticStore,
        previous_snapshot: Option<&ComposedSnapshot>,
        snapshot: &ComposedSnapshot,
        diff: Option<&SnapshotDiffResponse>,
        facts: &FactsBatch,
        graph: &SymbolGraph,
        symbol_index_build_id: Option<SymbolIndexBuildId>,
    ) -> SemanticResult<SemanticRefreshResult> {
        let started = Instant::now();
        let trigger = if previous_snapshot.is_some() || diff.is_some() {
            SemanticRefreshTrigger::SnapshotDiff
        } else {
            SemanticRefreshTrigger::Bootstrap
        };

        match self.plan_refresh(store, previous_snapshot, snapshot, diff)? {
            RefreshPlan::Full(reason) => self.run_full_rebuild(
                store,
                &started,
                trigger,
                snapshot,
                facts,
                graph,
                symbol_index_build_id,
                Some(reason),
            ),
            RefreshPlan::Incremental {
                diff,
                previous_build,
                previous_chunks,
                previous_vectors,
            } => self.run_incremental(
                store,
                &started,
                trigger,
                snapshot,
                facts,
                graph,
                symbol_index_build_id,
                diff,
                previous_build,
                previous_chunks,
                previous_vectors,
            ),
        }
    }

    pub fn full_rebuild(
        &self,
        store: &SemanticStore,
        snapshot: &ComposedSnapshot,
        facts: &FactsBatch,
        graph: &SymbolGraph,
        symbol_index_build_id: Option<SymbolIndexBuildId>,
    ) -> SemanticResult<SemanticRefreshResult> {
        let started = Instant::now();
        self.run_full_rebuild(
            store,
            &started,
            SemanticRefreshTrigger::Bootstrap,
            snapshot,
            facts,
            graph,
            symbol_index_build_id,
            None,
        )
    }

    fn plan_refresh(
        &self,
        store: &SemanticStore,
        previous_snapshot: Option<&ComposedSnapshot>,
        snapshot: &ComposedSnapshot,
        diff: Option<&SnapshotDiffResponse>,
    ) -> SemanticResult<RefreshPlan> {
        let Some(previous_snapshot) = previous_snapshot else {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::NoPriorSnapshot,
            ));
        };
        let Some(diff) = diff.cloned() else {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::MissingSnapshotDiff,
            ));
        };
        if previous_snapshot.repo_id != snapshot.repo_id
            || diff.left_snapshot_id != previous_snapshot.snapshot_id
            || diff.right_snapshot_id != snapshot.snapshot_id
        {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::UnresolvedConsistencyIssue,
            ));
        }

        let quick_check = match store.quick_check() {
            Ok(results) => results,
            Err(_) => {
                return Ok(RefreshPlan::Full(
                    SemanticRebuildFallbackReason::CacheOrIndexCorruption,
                ));
            }
        };
        if quick_check.iter().any(|result| result != "ok") {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::CacheOrIndexCorruption,
            ));
        }

        let Some(previous_build) = store.load_build(&previous_snapshot.snapshot_id)? else {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::NoPriorBuild,
            ));
        };
        if previous_build.repo_id != snapshot.repo_id {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::UnresolvedConsistencyIssue,
            ));
        }
        if previous_build.schema_version != store.schema_version {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::StoreSchemaVersionChanged,
            ));
        }
        if previous_build.chunk_schema_version != self.config.chunk_schema_version {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::ChunkSchemaVersionChanged,
            ));
        }
        if previous_build.embedding_provider != self.config.embedding_provider {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::EmbeddingProviderChanged,
            ));
        }
        if previous_build.chunk_text != self.config.chunk_text
            || previous_build.semantic_config_digest != self.builder.config_digest()
        {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::IncompatibleConfigChange,
            ));
        }

        if store
            .load_vector_index(&previous_snapshot.snapshot_id, &previous_build)
            .is_err()
        {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::CacheOrIndexCorruption,
            ));
        }
        let previous_chunks = match store.list_chunks(&previous_snapshot.snapshot_id) {
            Ok(chunks) => chunks,
            Err(_) => {
                return Ok(RefreshPlan::Full(
                    SemanticRebuildFallbackReason::CacheOrIndexCorruption,
                ));
            }
        };
        let previous_vectors = match store.load_chunk_vectors(&previous_snapshot.snapshot_id) {
            Ok(vectors) => vectors,
            Err(_) => {
                return Ok(RefreshPlan::Full(
                    SemanticRebuildFallbackReason::CacheOrIndexCorruption,
                ));
            }
        };
        let indexed_file_count = previous_chunks
            .iter()
            .map(|chunk| chunk.metadata.path.clone())
            .collect::<BTreeSet<_>>()
            .len();
        if previous_build.chunk_count != previous_chunks.len()
            || previous_build.embedding_count != previous_vectors.len()
            || previous_build.indexed_file_count != indexed_file_count
        {
            return Ok(RefreshPlan::Full(
                SemanticRebuildFallbackReason::UnresolvedConsistencyIssue,
            ));
        }

        Ok(RefreshPlan::Incremental {
            diff,
            previous_build,
            previous_chunks,
            previous_vectors,
        })
    }

    fn run_full_rebuild(
        &self,
        store: &SemanticStore,
        started: &Instant,
        trigger: SemanticRefreshTrigger,
        snapshot: &ComposedSnapshot,
        facts: &FactsBatch,
        graph: &SymbolGraph,
        symbol_index_build_id: Option<SymbolIndexBuildId>,
        fallback_reason: Option<SemanticRebuildFallbackReason>,
    ) -> SemanticResult<SemanticRefreshResult> {
        let mut build = self.builder.build_from_symbol_index(
            snapshot,
            facts,
            graph,
            symbol_index_build_id,
            store,
        )?;
        let stats = SemanticRefreshStats {
            files_touched: facts.files.len() as u64,
            chunks_rebuilt: build.chunk_count as u64,
            embeddings_regenerated: build.embedding_stats.cache_writes as u64,
            vector_entries_added: build.chunk_count as u64,
            vector_entries_updated: 0,
            vector_entries_removed: 0,
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        build.refresh_mode = "full_rebuild".to_string();
        build.fallback_reason = fallback_reason
            .as_ref()
            .map(|reason| reason.as_str().to_string());
        build.refresh_stats = Some(stats.clone());
        if let Some(reason) = &fallback_reason {
            build.diagnostics.push(SemanticDiagnostic {
                severity: SemanticDiagnosticSeverity::Info,
                code: "semantic_incremental_fallback".to_string(),
                message: format!(
                    "semantic refresh fell back to a full rebuild because {}",
                    reason.as_str()
                ),
            });
        }

        Ok(SemanticRefreshResult {
            build,
            mode: SemanticRefreshMode::FullRebuild,
            trigger,
            fallback_reason,
            stats,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn run_incremental(
        &self,
        store: &SemanticStore,
        started: &Instant,
        trigger: SemanticRefreshTrigger,
        snapshot: &ComposedSnapshot,
        facts: &FactsBatch,
        graph: &SymbolGraph,
        symbol_index_build_id: Option<SymbolIndexBuildId>,
        diff: SnapshotDiffResponse,
        previous_build: hyperindex_semantic_store::StoredSemanticBuild,
        previous_chunks: Vec<SemanticChunkRecord>,
        previous_vectors: Vec<StoredChunkVector>,
    ) -> SemanticResult<SemanticRefreshResult> {
        let current_files = facts
            .files
            .iter()
            .map(|file| (file.facts.path.clone(), file))
            .collect::<BTreeMap<_, _>>();
        let previous_chunks_by_path = previous_chunks.iter().fold(
            BTreeMap::<String, Vec<SemanticChunkRecord>>::new(),
            |mut map, chunk| {
                map.entry(chunk.metadata.path.clone())
                    .or_default()
                    .push(chunk.clone());
                map
            },
        );
        let previous_vectors_by_chunk_id = previous_vectors
            .iter()
            .map(|vector| (vector.chunk_id.0.clone(), vector.clone()))
            .collect::<BTreeMap<_, _>>();
        let touched_paths = diff
            .changed_paths
            .into_iter()
            .filter(|path| {
                current_files.contains_key(path) || previous_chunks_by_path.contains_key(path)
            })
            .collect::<BTreeSet<_>>();

        let touched_files = touched_paths
            .iter()
            .filter_map(|path| current_files.get(path).copied())
            .cloned()
            .collect::<Vec<_>>();
        let chunk_materialization_started = Instant::now();
        let mut rebuilt = self
            .builder
            .build_for_files(snapshot, &touched_files, graph)?;
        let chunk_materialization_ms = chunk_materialization_started.elapsed().as_millis() as u64;
        let provider = provider_from_config(&self.config)?;
        let truncated = truncate_oversized_chunks(
            &mut rebuilt.chunks,
            provider.config().max_input_bytes as usize,
        );
        if truncated > 0 {
            rebuilt.diagnostics.push(SemanticDiagnostic {
                severity: SemanticDiagnosticSeverity::Warning,
                code: "semantic_chunks_truncated".to_string(),
                message: format!(
                    "truncated {} semantic chunk(s) to respect embedding max_input_bytes={}",
                    truncated,
                    provider.config().max_input_bytes
                ),
            });
        }
        let embedding_started = Instant::now();
        let embedded = EmbeddingPipeline::new(provider.as_ref())
            .embed_chunk_documents(store, &rebuilt.chunks)?;
        let embedding_resolution_ms = embedding_started.elapsed().as_millis() as u64;

        let rebuilt_paths = touched_files
            .iter()
            .map(|file| file.facts.path.clone())
            .collect::<BTreeSet<_>>();
        let mut next_chunks = Vec::<SemanticChunkRecord>::new();
        let mut next_vectors = Vec::<StoredChunkVector>::new();

        for (path, file) in &current_files {
            if rebuilt_paths.contains(path) {
                continue;
            }
            let Some(previous_path_chunks) = previous_chunks_by_path.get(path) else {
                return self.run_full_rebuild(
                    store,
                    started,
                    trigger,
                    snapshot,
                    facts,
                    graph,
                    symbol_index_build_id,
                    Some(SemanticRebuildFallbackReason::UnresolvedConsistencyIssue),
                );
            };
            if previous_path_chunks
                .iter()
                .any(|chunk| chunk.metadata.content_sha256 != file.artifact.content_sha256)
            {
                return self.run_full_rebuild(
                    store,
                    started,
                    trigger,
                    snapshot,
                    facts,
                    graph,
                    symbol_index_build_id,
                    Some(SemanticRebuildFallbackReason::UnresolvedConsistencyIssue),
                );
            }
            for chunk in previous_path_chunks {
                let Some(vector) = previous_vectors_by_chunk_id.get(&chunk.metadata.chunk_id.0)
                else {
                    return self.run_full_rebuild(
                        store,
                        started,
                        trigger,
                        snapshot,
                        facts,
                        graph,
                        symbol_index_build_id,
                        Some(SemanticRebuildFallbackReason::UnresolvedConsistencyIssue),
                    );
                };
                next_chunks.push(chunk.clone());
                next_vectors.push(vector.clone());
            }
        }

        next_chunks.extend(embedded.chunks.iter().cloned());
        next_vectors.extend(embedded.chunk_vectors.iter().cloned());
        next_chunks.sort_by(|left, right| {
            left.metadata
                .path
                .cmp(&right.metadata.path)
                .then_with(|| left.metadata.chunk_id.0.cmp(&right.metadata.chunk_id.0))
        });
        next_vectors.sort_by(|left, right| left.chunk_id.0.cmp(&right.chunk_id.0));

        let semantic_config_digest = self.builder.config_digest();
        let symbol_build = symbol_index_build_id
            .as_ref()
            .map(|build_id| build_id.0.as_str())
            .unwrap_or("no-symbol-build");
        let build_id = SemanticBuildId(format!(
            "semantic-build-{}",
            &stable_digest(&[
                &snapshot.repo_id,
                &snapshot.snapshot_id,
                &semantic_config_digest,
                symbol_build,
            ])[..16]
        ));
        let mut diagnostics = self
            .builder
            .diagnostics_for_index(facts, graph, next_chunks.len());
        diagnostics.extend(
            rebuilt
                .diagnostics
                .into_iter()
                .filter(|diagnostic| diagnostic.code == "semantic_chunks_truncated"),
        );
        diagnostics.push(SemanticDiagnostic {
            severity: SemanticDiagnosticSeverity::Info,
            code: "semantic_embedding_provider_ready".to_string(),
            message: format!(
                "embedding provider {} refreshed {} touched document embeddings with {} cache hits and {} misses",
                provider.identity().model_id,
                embedded.chunks.len(),
                embedded.stats.cache_hits,
                embedded.stats.cache_misses,
            ),
        });
        diagnostics.push(SemanticDiagnostic {
            severity: SemanticDiagnosticSeverity::Info,
            code: "semantic_incremental_refresh_applied".to_string(),
            message: format!(
                "reused semantic state from {} while rebuilding {} touched files",
                previous_build.snapshot_id,
                touched_paths.len()
            ),
        });

        let vector_delta = compute_vector_delta(&previous_chunks, &next_chunks);
        let build = SemanticBuildDraft {
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            semantic_build_id: build_id,
            semantic_config_digest,
            embedding_provider: self.config.embedding_provider.clone(),
            chunk_text: self.config.chunk_text.clone(),
            chunk_schema_version: self.config.chunk_schema_version,
            symbol_index_build_id,
            created_at: unix_timestamp_string(),
            refresh_mode: "incremental".to_string(),
            fallback_reason: None,
            refresh_stats: Some(SemanticRefreshStats {
                files_touched: touched_paths.len() as u64,
                chunks_rebuilt: embedded.chunks.len() as u64,
                embeddings_regenerated: embedded.stats.cache_writes as u64,
                vector_entries_added: vector_delta.added,
                vector_entries_updated: vector_delta.updated,
                vector_entries_removed: vector_delta.removed,
                elapsed_ms: started.elapsed().as_millis() as u64,
            }),
            chunk_count: next_chunks.len(),
            indexed_file_count: current_files.len(),
            embedding_count: next_chunks.len(),
            embedding_stats: embedded.stats,
            profile: Some(SemanticBuildProfile {
                chunk_materialization_ms,
                embedding_resolution_ms,
                vector_persist_ms: 0,
            }),
            diagnostics,
            chunks: next_chunks,
            chunk_vectors: next_vectors,
        };
        Ok(SemanticRefreshResult {
            stats: build.refresh_stats.clone().unwrap(),
            build,
            mode: SemanticRefreshMode::Incremental,
            trigger,
            fallback_reason: None,
        })
    }
}

enum RefreshPlan {
    Full(SemanticRebuildFallbackReason),
    Incremental {
        diff: SnapshotDiffResponse,
        previous_build: hyperindex_semantic_store::StoredSemanticBuild,
        previous_chunks: Vec<SemanticChunkRecord>,
        previous_vectors: Vec<StoredChunkVector>,
    },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct VectorDelta {
    added: u64,
    updated: u64,
    removed: u64,
}

fn compute_vector_delta(
    previous_chunks: &[SemanticChunkRecord],
    next_chunks: &[SemanticChunkRecord],
) -> VectorDelta {
    let previous = previous_chunks
        .iter()
        .map(|chunk| (logical_chunk_key(chunk), chunk))
        .collect::<BTreeMap<_, _>>();
    let next = next_chunks
        .iter()
        .map(|chunk| (logical_chunk_key(chunk), chunk))
        .collect::<BTreeMap<_, _>>();
    let all_keys = previous
        .keys()
        .chain(next.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut delta = VectorDelta::default();
    for key in all_keys {
        match (previous.get(&key), next.get(&key)) {
            (None, Some(_)) => delta.added += 1,
            (Some(_), None) => delta.removed += 1,
            (Some(left), Some(right)) => {
                if left.metadata.chunk_id != right.metadata.chunk_id
                    || left.metadata.text.text_digest != right.metadata.text.text_digest
                {
                    delta.updated += 1;
                }
            }
            (None, None) => {}
        }
    }
    delta
}

fn logical_chunk_key(chunk: &SemanticChunkRecord) -> String {
    format!(
        "{}|{:?}|{:?}|{}|{}",
        chunk.metadata.path,
        chunk.metadata.chunk_kind,
        chunk.metadata.source_kind,
        chunk
            .metadata
            .symbol_id
            .as_ref()
            .map(|value| value.0.as_str())
            .unwrap_or("-"),
        chunk
            .metadata
            .span
            .as_ref()
            .map(render_span_key)
            .unwrap_or_else(|| "-".to_string())
    )
}

fn render_span_key(span: &hyperindex_protocol::symbols::SourceSpan) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        span.start.line,
        span.start.column,
        span.end.line,
        span.end.column,
        span.bytes.start,
        span.bytes.end
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rusqlite::Connection;
    use tempfile::tempdir;

    use hyperindex_protocol::config::SemanticConfig;
    use hyperindex_protocol::semantic::{
        SemanticChunkRecord, SemanticQueryFilters, SemanticQueryParams, SemanticQueryText,
        SemanticRerankMode,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, BufferOverlay, ComposedSnapshot, SnapshotFile,
        WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::SymbolIndexBuildId;
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};
    use hyperindex_semantic_store::{SemanticStore, StoredChunkVector, StoredSemanticBuild};
    use hyperindex_symbols::{FactsBatch, SymbolGraph, SymbolWorkspace};

    use crate::common::stable_digest;
    use crate::{SemanticBuildDraft, SemanticSearchEngine, SemanticSearchScaffold};

    use super::{IncrementalSemanticIndexer, SemanticRebuildFallbackReason, SemanticRefreshMode};

    #[test]
    fn single_file_edit_avoids_full_rebuild_and_matches_full_output() {
        let tempdir = tempdir().unwrap();
        let config = SemanticConfig::default();
        let indexer = IncrementalSemanticIndexer::new(&config);
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-1").unwrap();

        let base = semantic_snapshot(
            "snap-base",
            vec![
                (
                    "src/session.ts",
                    r#"export function invalidateSession(sessionId: string): string {
  return `session:${sessionId}`;
}
"#,
                ),
                (
                    "src/logout.ts",
                    r#"import { invalidateSession } from "./session";

export function logout(sessionId: string): string {
  return invalidateSession(sessionId);
}
"#,
                ),
                (
                    "tests/session.test.ts",
                    r#"import { logout } from "../src/logout";

it("invalidates sessions", () => {
  expect(logout("1")).toContain("session");
});
"#,
                ),
            ],
            Vec::new(),
        );
        let (base_facts, base_graph) = prepare_symbol_index(&base);
        let base_refresh = indexer
            .full_rebuild(
                &store,
                &base,
                &base_facts,
                &base_graph,
                Some(symbol_build_id(&base)),
            )
            .unwrap();
        persist_build(&store, &base_refresh.build);

        let next = semantic_snapshot(
            "snap-next",
            vec![
                (
                    "src/session.ts",
                    r#"export function invalidateSession(sessionId: string): string {
  return `updated:${sessionId}`;
}
"#,
                ),
                (
                    "src/logout.ts",
                    r#"import { invalidateSession } from "./session";

export function logout(sessionId: string): string {
  return invalidateSession(sessionId);
}
"#,
                ),
                (
                    "tests/session.test.ts",
                    r#"import { logout } from "../src/logout";

it("invalidates sessions", () => {
  expect(logout("1")).toContain("updated");
});
"#,
                ),
            ],
            Vec::new(),
        );
        let diff = hyperindex_snapshot::SnapshotAssembler.diff(&base, &next);
        let (next_facts, next_graph) = prepare_symbol_index(&next);
        let incremental = indexer
            .refresh(
                &store,
                Some(&base),
                &next,
                Some(&diff),
                &next_facts,
                &next_graph,
                Some(symbol_build_id(&next)),
            )
            .unwrap();
        let full_store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-1-full").unwrap();
        let full = indexer
            .full_rebuild(
                &full_store,
                &next,
                &next_facts,
                &next_graph,
                Some(symbol_build_id(&next)),
            )
            .unwrap();

        assert_eq!(incremental.mode, SemanticRefreshMode::Incremental);
        assert!(incremental.fallback_reason.is_none());
        assert_eq!(incremental.stats.files_touched, 2);
        assert!(incremental.stats.chunks_rebuilt < incremental.build.chunk_count as u64);
        assert_eq!(
            chunk_fingerprints(&incremental.build.chunks),
            chunk_fingerprints(&full.build.chunks)
        );
        assert_eq!(
            vector_fingerprints(&incremental.build.chunk_vectors),
            vector_fingerprints(&full.build.chunk_vectors)
        );
    }

    #[test]
    fn add_delete_modify_flows_are_correct() {
        let tempdir = tempdir().unwrap();
        let config = SemanticConfig::default();
        let indexer = IncrementalSemanticIndexer::new(&config);
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-1").unwrap();

        let base = semantic_snapshot(
            "snap-base",
            vec![
                (
                    "src/session.ts",
                    r#"export function invalidateSession(sessionId: string): string {
  return `session:${sessionId}`;
}
"#,
                ),
                (
                    "src/logout.ts",
                    r#"import { invalidateSession } from "./session";

export function logout(sessionId: string): string {
  return invalidateSession(sessionId);
}
"#,
                ),
            ],
            Vec::new(),
        );
        let (base_facts, base_graph) = prepare_symbol_index(&base);
        let base_refresh = indexer
            .full_rebuild(
                &store,
                &base,
                &base_facts,
                &base_graph,
                Some(symbol_build_id(&base)),
            )
            .unwrap();
        persist_build(&store, &base_refresh.build);

        let next = semantic_snapshot(
            "snap-next",
            vec![
                (
                    "src/session.ts",
                    r#"export function invalidateSessionBuffered(sessionId: string): string {
  return `buffered:${sessionId}`;
}
"#,
                ),
                (
                    "src/router.ts",
                    r#"import { invalidateSessionBuffered } from "./session";

export function routeLogout(sessionId: string): string {
  return invalidateSessionBuffered(sessionId);
}
"#,
                ),
            ],
            Vec::new(),
        );
        let diff = hyperindex_snapshot::SnapshotAssembler.diff(&base, &next);
        let (next_facts, next_graph) = prepare_symbol_index(&next);
        let refresh = indexer
            .refresh(
                &store,
                Some(&base),
                &next,
                Some(&diff),
                &next_facts,
                &next_graph,
                Some(symbol_build_id(&next)),
            )
            .unwrap();

        let paths = refresh
            .build
            .chunks
            .iter()
            .map(|chunk| chunk.metadata.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(refresh.mode, SemanticRefreshMode::Incremental);
        assert_eq!(refresh.stats.files_touched, 3);
        assert!(paths.contains(&"src/session.ts".to_string()));
        assert!(paths.contains(&"src/router.ts".to_string()));
        assert!(!paths.contains(&"src/logout.ts".to_string()));
        assert!(refresh.stats.vector_entries_added > 0);
        assert!(refresh.stats.vector_entries_removed > 0);
        assert!(refresh.stats.chunks_rebuilt > 0);
    }

    #[test]
    fn buffer_overlay_changes_semantic_results_before_save_and_matches_full_output() {
        let tempdir = tempdir().unwrap();
        let config = SemanticConfig::default();
        let indexer = IncrementalSemanticIndexer::new(&config);
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-1").unwrap();

        let base = semantic_snapshot(
            "snap-base",
            vec![
                (
                    "src/session.ts",
                    r#"export function invalidateSession(sessionId: string): string {
  return `session:${sessionId}`;
}
"#,
                ),
                (
                    "src/logout.ts",
                    r#"import { invalidateSession } from "./session";

export function logout(sessionId: string): string {
  return invalidateSession(sessionId);
}
"#,
                ),
            ],
            Vec::new(),
        );
        let (base_facts, base_graph) = prepare_symbol_index(&base);
        let base_refresh = indexer
            .full_rebuild(
                &store,
                &base,
                &base_facts,
                &base_graph,
                Some(symbol_build_id(&base)),
            )
            .unwrap();
        persist_build(&store, &base_refresh.build);

        let buffered = semantic_snapshot(
            "snap-buffered",
            vec![
                (
                    "src/session.ts",
                    r#"export function invalidateSession(sessionId: string): string {
  return `session:${sessionId}`;
}
"#,
                ),
                (
                    "src/logout.ts",
                    r#"import { invalidateSession } from "./session";

export function logout(sessionId: string): string {
  return invalidateSession(sessionId);
}
"#,
                ),
            ],
            vec![(
                "buffer-1",
                "src/session.ts",
                r#"export function invalidateSessionBuffered(sessionId: string): string {
  return `buffered:${sessionId}`;
}
"#,
            )],
        );
        let diff = hyperindex_snapshot::SnapshotAssembler.diff(&base, &buffered);
        let (buffered_facts, buffered_graph) = prepare_symbol_index(&buffered);
        let incremental = indexer
            .refresh(
                &store,
                Some(&base),
                &buffered,
                Some(&diff),
                &buffered_facts,
                &buffered_graph,
                Some(symbol_build_id(&buffered)),
            )
            .unwrap();
        persist_build(&store, &incremental.build);
        let query = query_symbol_names(&store, &incremental.build, &config, "buffered session");

        let full_store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-1-full").unwrap();
        let full = indexer
            .full_rebuild(
                &full_store,
                &buffered,
                &buffered_facts,
                &buffered_graph,
                Some(symbol_build_id(&buffered)),
            )
            .unwrap();

        assert_eq!(
            diff.buffer_only_changed_paths,
            vec!["src/session.ts".to_string()]
        );
        assert_eq!(incremental.mode, SemanticRefreshMode::Incremental);
        assert_eq!(incremental.stats.files_touched, 1);
        assert!(query.contains(&"invalidateSessionBuffered".to_string()));
        assert!(!query.contains(&"invalidateSession".to_string()));
        assert_eq!(
            chunk_fingerprints(&incremental.build.chunks),
            chunk_fingerprints(&full.build.chunks)
        );
        assert_eq!(
            vector_fingerprints(&incremental.build.chunk_vectors),
            vector_fingerprints(&full.build.chunk_vectors)
        );
    }

    #[test]
    fn provider_changes_force_full_rebuild() {
        let tempdir = tempdir().unwrap();
        let config = SemanticConfig::default();
        let indexer = IncrementalSemanticIndexer::new(&config);
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-1").unwrap();

        let base = semantic_snapshot(
            "snap-base",
            vec![(
                "src/session.ts",
                r#"export function invalidateSession(sessionId: string): string {
  return `session:${sessionId}`;
}
"#,
            )],
            Vec::new(),
        );
        let (base_facts, base_graph) = prepare_symbol_index(&base);
        let base_refresh = indexer
            .full_rebuild(
                &store,
                &base,
                &base_facts,
                &base_graph,
                Some(symbol_build_id(&base)),
            )
            .unwrap();
        persist_build(&store, &base_refresh.build);

        let next = semantic_snapshot(
            "snap-next",
            vec![(
                "src/session.ts",
                r#"export function invalidateSessionBuffered(sessionId: string): string {
  return `buffered:${sessionId}`;
}
"#,
            )],
            Vec::new(),
        );
        let diff = hyperindex_snapshot::SnapshotAssembler.diff(&base, &next);
        let (next_facts, next_graph) = prepare_symbol_index(&next);
        let mut changed_config = config.clone();
        changed_config.embedding_provider.model_digest =
            "phase6-deterministic-fixture-v2".to_string();
        let changed_indexer = IncrementalSemanticIndexer::new(&changed_config);
        let refresh = changed_indexer
            .refresh(
                &store,
                Some(&base),
                &next,
                Some(&diff),
                &next_facts,
                &next_graph,
                Some(symbol_build_id(&next)),
            )
            .unwrap();

        assert_eq!(refresh.mode, SemanticRefreshMode::FullRebuild);
        assert_eq!(
            refresh.fallback_reason,
            Some(SemanticRebuildFallbackReason::EmbeddingProviderChanged)
        );
        assert_eq!(refresh.build.refresh_mode, "full_rebuild");
        assert_eq!(
            refresh.build.fallback_reason.as_deref(),
            Some("embedding_provider_changed")
        );
    }

    #[test]
    fn corruption_forces_full_rebuild() {
        let tempdir = tempdir().unwrap();
        let config = SemanticConfig::default();
        let indexer = IncrementalSemanticIndexer::new(&config);
        let store = SemanticStore::open_in_store_dir(tempdir.path(), "repo-1").unwrap();

        let base = semantic_snapshot(
            "snap-base",
            vec![(
                "src/session.ts",
                r#"export function invalidateSession(sessionId: string): string {
  return `session:${sessionId}`;
}
"#,
            )],
            Vec::new(),
        );
        let (base_facts, base_graph) = prepare_symbol_index(&base);
        let base_refresh = indexer
            .full_rebuild(
                &store,
                &base,
                &base_facts,
                &base_graph,
                Some(symbol_build_id(&base)),
            )
            .unwrap();
        persist_build(&store, &base_refresh.build);

        let connection = Connection::open(&store.store_path).unwrap();
        connection
            .execute(
                "UPDATE semantic_vector_index_metadata SET vector_dimensions = ?1 WHERE snapshot_id = ?2",
                rusqlite::params![999u32, "snap-base"],
            )
            .unwrap();

        let next = semantic_snapshot(
            "snap-next",
            vec![(
                "src/session.ts",
                r#"export function invalidateSessionBuffered(sessionId: string): string {
  return `buffered:${sessionId}`;
}
"#,
            )],
            Vec::new(),
        );
        let diff = hyperindex_snapshot::SnapshotAssembler.diff(&base, &next);
        let (next_facts, next_graph) = prepare_symbol_index(&next);
        let refresh = indexer
            .refresh(
                &store,
                Some(&base),
                &next,
                Some(&diff),
                &next_facts,
                &next_graph,
                Some(symbol_build_id(&next)),
            )
            .unwrap();

        assert_eq!(refresh.mode, SemanticRefreshMode::FullRebuild);
        assert_eq!(
            refresh.fallback_reason,
            Some(SemanticRebuildFallbackReason::CacheOrIndexCorruption)
        );
    }

    fn prepare_symbol_index(snapshot: &ComposedSnapshot) -> (FactsBatch, SymbolGraph) {
        let mut workspace = SymbolWorkspace::default();
        let prepared = workspace.prepare_snapshot(snapshot).unwrap();
        (prepared.facts, prepared.graph)
    }

    fn persist_build(store: &SemanticStore, draft: &SemanticBuildDraft) {
        let stored = StoredSemanticBuild {
            repo_id: draft.repo_id.clone(),
            snapshot_id: draft.snapshot_id.clone(),
            semantic_build_id: draft.semantic_build_id.clone(),
            semantic_config_digest: draft.semantic_config_digest.clone(),
            schema_version: store.schema_version,
            chunk_schema_version: draft.chunk_schema_version,
            embedding_provider: draft.embedding_provider.clone(),
            chunk_text: draft.chunk_text.clone(),
            symbol_index_build_id: draft.symbol_index_build_id.clone(),
            created_at: draft.created_at.clone(),
            refresh_mode: draft.refresh_mode.clone(),
            chunk_count: draft.chunk_count,
            indexed_file_count: draft.indexed_file_count,
            embedding_count: draft.embedding_count,
            embedding_cache_hits: draft.embedding_stats.cache_hits,
            embedding_cache_misses: draft.embedding_stats.cache_misses,
            embedding_cache_writes: draft.embedding_stats.cache_writes,
            embedding_provider_batches: draft.embedding_stats.provider_batches,
            profile: draft.profile.clone(),
            refresh_stats: draft.refresh_stats.clone(),
            fallback_reason: draft.fallback_reason.clone(),
            diagnostics: draft.diagnostics.clone(),
        };
        store
            .persist_build_with_chunks_and_vectors(&stored, &draft.chunks, &draft.chunk_vectors)
            .unwrap();
    }

    fn query_symbol_names(
        store: &SemanticStore,
        build: &SemanticBuildDraft,
        config: &SemanticConfig,
        query: &str,
    ) -> Vec<String> {
        let provider = crate::provider_from_config(config).unwrap();
        let index = store
            .load_vector_index(&build.snapshot_id, &stored_build(store, build))
            .unwrap();
        let chunks = store.list_chunks(&build.snapshot_id).unwrap();
        let response = SemanticSearchEngine::default()
            .search(
                &SemanticSearchScaffold {
                    manifest: store.manifest_for(&stored_build(store, build)),
                    chunks,
                    index,
                    diagnostics: build.diagnostics.clone(),
                },
                &SemanticQueryParams {
                    repo_id: build.repo_id.clone(),
                    snapshot_id: build.snapshot_id.clone(),
                    query: SemanticQueryText {
                        text: query.to_string(),
                    },
                    filters: SemanticQueryFilters::default(),
                    limit: 5,
                    rerank_mode: SemanticRerankMode::Hybrid,
                },
                store,
                provider.as_ref(),
            )
            .unwrap();
        response
            .hits
            .iter()
            .filter_map(|hit| hit.chunk.symbol_display_name.clone())
            .collect()
    }

    fn stored_build(store: &SemanticStore, draft: &SemanticBuildDraft) -> StoredSemanticBuild {
        StoredSemanticBuild {
            repo_id: draft.repo_id.clone(),
            snapshot_id: draft.snapshot_id.clone(),
            semantic_build_id: draft.semantic_build_id.clone(),
            semantic_config_digest: draft.semantic_config_digest.clone(),
            schema_version: store.schema_version,
            chunk_schema_version: draft.chunk_schema_version,
            embedding_provider: draft.embedding_provider.clone(),
            chunk_text: draft.chunk_text.clone(),
            symbol_index_build_id: draft.symbol_index_build_id.clone(),
            created_at: draft.created_at.clone(),
            refresh_mode: draft.refresh_mode.clone(),
            chunk_count: draft.chunk_count,
            indexed_file_count: draft.indexed_file_count,
            embedding_count: draft.embedding_count,
            embedding_cache_hits: draft.embedding_stats.cache_hits,
            embedding_cache_misses: draft.embedding_stats.cache_misses,
            embedding_cache_writes: draft.embedding_stats.cache_writes,
            embedding_provider_batches: draft.embedding_stats.provider_batches,
            profile: draft.profile.clone(),
            refresh_stats: draft.refresh_stats.clone(),
            fallback_reason: draft.fallback_reason.clone(),
            diagnostics: draft.diagnostics.clone(),
        }
    }

    fn chunk_fingerprints(chunks: &[SemanticChunkRecord]) -> BTreeMap<String, String> {
        chunks
            .iter()
            .map(|chunk| {
                (
                    chunk.metadata.chunk_id.0.clone(),
                    format!(
                        "{}|{}|{}",
                        chunk.metadata.path, chunk.metadata.text.text_digest, chunk.serialized_text
                    ),
                )
            })
            .collect()
    }

    fn vector_fingerprints(vectors: &[StoredChunkVector]) -> BTreeMap<String, Vec<f32>> {
        vectors
            .iter()
            .map(|vector| (vector.chunk_id.0.clone(), vector.vector.clone()))
            .collect()
    }

    fn symbol_build_id(snapshot: &ComposedSnapshot) -> SymbolIndexBuildId {
        SymbolIndexBuildId(format!("symbol-index-scaffold-{}", snapshot.snapshot_id))
    }

    fn semantic_snapshot(
        snapshot_id: &str,
        files: Vec<(&str, &str)>,
        buffers: Vec<(&str, &str, &str)>,
    ) -> ComposedSnapshot {
        let base_files = files
            .into_iter()
            .map(|(path, contents)| SnapshotFile {
                path: path.to_string(),
                content_sha256: format!("sha-{}", stable_digest(&[path, contents])),
                content_bytes: contents.len(),
                contents: contents.to_string(),
            })
            .collect::<Vec<_>>();
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: snapshot_id.to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "deadbeef".to_string(),
                digest: format!("base-{}", snapshot_id),
                file_count: base_files.len(),
                files: base_files,
            },
            working_tree: WorkingTreeOverlay {
                digest: format!("work-{}", snapshot_id),
                entries: Vec::new(),
            },
            buffers: buffers
                .into_iter()
                .enumerate()
                .map(|(index, (buffer_id, path, contents))| BufferOverlay {
                    buffer_id: buffer_id.to_string(),
                    path: path.to_string(),
                    version: index as u64 + 1,
                    content_sha256: format!("sha-{}", stable_digest(&[path, contents, buffer_id])),
                    content_bytes: contents.len(),
                    contents: contents.to_string(),
                })
                .collect(),
        }
    }
}
