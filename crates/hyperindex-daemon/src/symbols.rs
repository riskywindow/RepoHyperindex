use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use hyperindex_config::LoadedConfig;
use hyperindex_core::{HyperindexError, HyperindexResult, normalize_repo_relative_path};
use hyperindex_parser::{ParseBuildStatus, ParseCore, ParseCoreSettings, ParseManager};
use hyperindex_protocol::repo::RepoRecord;
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::status::{ParseRuntimeStatus, SymbolIndexRuntimeStatus};
use hyperindex_protocol::symbols::{
    DefinitionLookupParams, DefinitionLookupResponse, FileFacts, ParseArtifactManifest,
    ParseBuildCounts, ParseBuildId, ParseBuildParams, ParseBuildRecord, ParseBuildResponse,
    ParseBuildState, ParseInspectFileParams, ParseInspectFileResponse, ParseStatusParams,
    ParseStatusResponse, ReferenceLookupParams, ReferenceLookupResponse, ResolvedSymbol,
    SymbolIndexBuildId, SymbolIndexBuildParams, SymbolIndexBuildRecord, SymbolIndexBuildResponse,
    SymbolIndexBuildState, SymbolIndexManifest, SymbolIndexStats, SymbolIndexStatusParams,
    SymbolIndexStatusResponse, SymbolIndexStorage, SymbolIndexStorageFormat, SymbolResolveParams,
    SymbolResolveResponse, SymbolSearchParams, SymbolSearchResponse, SymbolShowParams,
    SymbolShowResponse,
};
use hyperindex_snapshot::SnapshotAssembler;
use hyperindex_symbol_store::{
    IncrementalIndexOptions, IncrementalRefreshMode, IncrementalSymbolIndexer,
    RebuildFallbackReason, SymbolStore, SymbolStoreStatus,
};
use hyperindex_symbols::{
    FactWorkspace, FactsBatch, SymbolGraph, SymbolGraphBuilder, SymbolQueryEngine,
};
use tracing::info;

#[derive(Debug, Clone)]
pub struct ParserSymbolService {
    loaded: LoadedConfig,
}

#[derive(Debug, Clone)]
struct SymbolBuildOutcome {
    graph: SymbolGraph,
    store_status: SymbolStoreStatus,
    parser_build: ParseBuildStatus,
    refresh_mode: Option<String>,
    fallback_reason: Option<String>,
    loaded_from_existing_build: bool,
}

impl ParserSymbolService {
    pub fn from_loaded_config(loaded: &LoadedConfig) -> Self {
        Self {
            loaded: loaded.clone(),
        }
    }

    pub fn parse_build(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &ParseBuildParams,
    ) -> HyperindexResult<ParseBuildResponse> {
        let build = self.ensure_parse_build(snapshot, params.force)?;
        Ok(ParseBuildResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            build: protocol_parse_build_record(&build),
        })
    }

    pub fn parse_status(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &ParseStatusParams,
    ) -> HyperindexResult<ParseStatusResponse> {
        let maybe_build = self.load_parse_build(snapshot)?;
        let builds = maybe_build
            .into_iter()
            .filter(|build| {
                params
                    .build_id
                    .as_ref()
                    .map(|build_id| build_id.0 == build.build_id)
                    .unwrap_or(true)
            })
            .map(|build| protocol_parse_build_record(&build))
            .collect();
        Ok(ParseStatusResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            builds,
        })
    }

    pub fn parse_inspect_file(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &ParseInspectFileParams,
    ) -> HyperindexResult<ParseInspectFileResponse> {
        let normalized_path = normalize_repo_relative_path(&params.path, "parse inspect")?;
        let mut manager = self.parse_manager();
        let record = manager
            .inspect_file(snapshot, &normalized_path)
            .map_err(|error| HyperindexError::Message(error.to_string()))?
            .ok_or_else(|| {
                HyperindexError::Message(format!(
                    "parse artifact for {} was not found",
                    normalized_path
                ))
            })?;

        Ok(ParseInspectFileResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            artifact: record.inspection.artifact,
            facts: if params.include_facts {
                self.extract_file_facts(snapshot, &normalized_path)?
            } else {
                None
            },
        })
    }

    pub fn symbol_index_build(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &SymbolIndexBuildParams,
    ) -> HyperindexResult<SymbolIndexBuildResponse> {
        let outcome = self.ensure_symbol_graph(repo, snapshot, params.force)?;
        Ok(SymbolIndexBuildResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            build: protocol_symbol_build_record(repo, snapshot, &outcome),
        })
    }

    pub fn symbol_index_status(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &SymbolIndexStatusParams,
    ) -> HyperindexResult<SymbolIndexStatusResponse> {
        let builds = self
            .load_symbol_graph(repo, snapshot)?
            .into_iter()
            .map(|outcome| protocol_symbol_build_record(repo, snapshot, &outcome))
            .filter(|build| {
                params
                    .build_id
                    .as_ref()
                    .map(|build_id| build_id.0 == build.build_id.0)
                    .unwrap_or(true)
            })
            .collect();
        Ok(SymbolIndexStatusResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            builds,
        })
    }

    pub fn search(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &SymbolSearchParams,
    ) -> HyperindexResult<SymbolSearchResponse> {
        let outcome = self.ensure_symbol_graph(repo, snapshot, false)?;
        let engine = SymbolQueryEngine;
        let hits = engine.search_hits(
            &outcome.graph,
            &params.query,
            self.search_limit(params.limit),
        );
        Ok(SymbolSearchResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            manifest: Some(symbol_index_manifest(repo, snapshot, &outcome)),
            hits,
            diagnostics: collect_graph_diagnostics(&outcome.graph),
        })
    }

    pub fn show(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &SymbolShowParams,
    ) -> HyperindexResult<SymbolShowResponse> {
        let outcome = self.ensure_symbol_graph(repo, snapshot, false)?;
        let engine = SymbolQueryEngine;
        let symbol = outcome
            .graph
            .symbols
            .get(&params.symbol_id.0)
            .cloned()
            .ok_or_else(|| {
                HyperindexError::Message(format!("symbol {} was not found", params.symbol_id.0))
            })?;
        let show = engine
            .show(&outcome.graph, &params.symbol_id.0)
            .ok_or_else(|| {
                HyperindexError::Message(format!("symbol {} was not found", params.symbol_id.0))
            })?;
        let file = load_snapshot_facts(self.symbol_store(repo)?, snapshot)?
            .files
            .into_iter()
            .find(|entry| entry.facts.path == symbol.path)
            .map(|entry| entry.artifact);

        Ok(SymbolShowResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            manifest: Some(symbol_index_manifest(repo, snapshot, &outcome)),
            symbol: show.symbol,
            definitions: show.definitions,
            related_edges: show.related_edges,
            file,
        })
    }

    pub fn definitions(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &DefinitionLookupParams,
    ) -> HyperindexResult<DefinitionLookupResponse> {
        let outcome = self.ensure_symbol_graph(repo, snapshot, false)?;
        let engine = SymbolQueryEngine;
        if !outcome.graph.symbols.contains_key(&params.symbol_id.0) {
            return Err(HyperindexError::Message(format!(
                "symbol {} was not found",
                params.symbol_id.0
            )));
        }
        Ok(DefinitionLookupResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            symbol_id: params.symbol_id.clone(),
            manifest: Some(symbol_index_manifest(repo, snapshot, &outcome)),
            definitions: engine.definition_occurrences(&outcome.graph, &params.symbol_id.0),
        })
    }

    pub fn references(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &ReferenceLookupParams,
    ) -> HyperindexResult<ReferenceLookupResponse> {
        let outcome = self.ensure_symbol_graph(repo, snapshot, false)?;
        let engine = SymbolQueryEngine;
        if !outcome.graph.symbols.contains_key(&params.symbol_id.0) {
            return Err(HyperindexError::Message(format!(
                "symbol {} was not found",
                params.symbol_id.0
            )));
        }
        let mut references =
            engine.reference_occurrences(&outcome.graph, &params.symbol_id.0, None);
        if let Some(limit) = params.limit {
            references.truncate(limit as usize);
        }
        Ok(ReferenceLookupResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            symbol_id: params.symbol_id.clone(),
            manifest: Some(symbol_index_manifest(repo, snapshot, &outcome)),
            references,
        })
    }

    pub fn resolve(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        params: &SymbolResolveParams,
    ) -> HyperindexResult<SymbolResolveResponse> {
        let outcome = self.ensure_symbol_graph(repo, snapshot, false)?;
        let engine = SymbolQueryEngine;
        let resolution = match &params.selector {
            hyperindex_protocol::symbols::SymbolLocationSelector::LineColumn { path, .. }
            | hyperindex_protocol::symbols::SymbolLocationSelector::ByteOffset { path, .. } => {
                let normalized_path = normalize_repo_relative_path(path, "symbol resolve")?;
                resolve_with_normalized_selector(
                    &engine,
                    &outcome.graph,
                    &params.selector,
                    &normalized_path,
                )
            }
        };
        Ok(SymbolResolveResponse {
            repo_id: repo.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            selector: params.selector.clone(),
            resolution,
            diagnostics: collect_graph_diagnostics(&outcome.graph),
        })
    }

    fn ensure_parse_build(
        &self,
        snapshot: &ComposedSnapshot,
        force: bool,
    ) -> HyperindexResult<ParseBuildStatus> {
        let mut manager = self.parse_manager();
        manager
            .build_snapshot(snapshot, force)
            .map_err(|error| HyperindexError::Message(error.to_string()))
    }

    fn load_parse_build(
        &self,
        snapshot: &ComposedSnapshot,
    ) -> HyperindexResult<Option<ParseBuildStatus>> {
        let manager = self.parse_manager();
        manager
            .load_build_status(snapshot)
            .map_err(|error| HyperindexError::Message(error.to_string()))
    }

    fn parse_manager(&self) -> ParseManager {
        ParseManager::from_runtime_config(&self.loaded.config)
    }

    fn ensure_symbol_graph(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
        force: bool,
    ) -> HyperindexResult<SymbolBuildOutcome> {
        let parser_build = self.ensure_parse_build(snapshot, false)?;
        if !force {
            if let Some(existing) = self.load_symbol_graph(repo, snapshot)? {
                return Ok(existing);
            }
        }

        let mut indexer = IncrementalSymbolIndexer::open(
            &self.loaded.config.symbol_index.store_dir,
            &repo.repo_id,
            incremental_index_options(&self.loaded),
        )
        .map_err(symbol_store_error)?;
        let previous_snapshot = if force {
            None
        } else {
            self.previous_indexed_snapshot(repo, &snapshot.snapshot_id)?
        };
        let diff = previous_snapshot
            .as_ref()
            .map(|previous| SnapshotAssembler.diff(previous, snapshot));
        let refresh = if force {
            indexer
                .refresh(None, snapshot, None)
                .map_err(symbol_store_error)?
        } else {
            indexer
                .refresh(previous_snapshot.as_ref(), snapshot, diff.as_ref())
                .map_err(symbol_store_error)?
        };
        let store_status = self
            .symbol_store(repo)?
            .status()
            .map_err(symbol_store_error)?;
        info!(
            repo_id = %repo.repo_id,
            snapshot_id = %snapshot.snapshot_id,
            mode = %refresh_mode_name(&refresh.stats.mode),
            loaded_from_existing_build = false,
            "built parser and symbol index snapshot"
        );
        Ok(SymbolBuildOutcome {
            graph: refresh.graph,
            store_status,
            parser_build,
            refresh_mode: Some(refresh_mode_name(&refresh.stats.mode)),
            fallback_reason: refresh.fallback_reason.as_ref().map(fallback_reason_name),
            loaded_from_existing_build: false,
        })
    }

    fn load_symbol_graph(
        &self,
        repo: &RepoRecord,
        snapshot: &ComposedSnapshot,
    ) -> HyperindexResult<Option<SymbolBuildOutcome>> {
        let store = self.symbol_store(repo)?;
        let Some(indexed_state) = store
            .load_indexed_snapshot_state(&snapshot.snapshot_id)
            .map_err(symbol_store_error)?
        else {
            return Ok(None);
        };
        if indexed_state.schema_version
            != hyperindex_symbol_store::migrations::SYMBOL_STORE_SCHEMA_VERSION as u32
        {
            return Ok(None);
        }
        let extracted = match load_snapshot_facts(store, snapshot) {
            Ok(extracted) => extracted,
            Err(_) => return Ok(None),
        };
        let parser_build = self.ensure_parse_build(snapshot, false)?;
        let graph = SymbolGraphBuilder.build_with_snapshot(&extracted, snapshot);
        let store_status = self
            .symbol_store(repo)?
            .status()
            .map_err(symbol_store_error)?;
        Ok(Some(SymbolBuildOutcome {
            graph,
            store_status,
            parser_build,
            refresh_mode: Some(indexed_state.refresh_mode),
            fallback_reason: None,
            loaded_from_existing_build: true,
        }))
    }

    fn previous_indexed_snapshot(
        &self,
        repo: &RepoRecord,
        current_snapshot_id: &str,
    ) -> HyperindexResult<Option<ComposedSnapshot>> {
        let manifest_store =
            hyperindex_repo_store::RepoStore::open_from_config(&self.loaded.config)?;
        let manifests = match manifest_store.list_manifests(&repo.repo_id, 32) {
            Ok(manifests) => manifests,
            Err(error) => {
                info!(
                    repo_id = %repo.repo_id,
                    error = %error,
                    "skipping prior indexed snapshot lookup because manifest listing failed"
                );
                return Ok(None);
            }
        };
        for manifest in manifests {
            if manifest.snapshot_id == current_snapshot_id {
                continue;
            }
            if self
                .symbol_store(repo)?
                .load_indexed_snapshot_state(&manifest.snapshot_id)
                .map_err(symbol_store_error)?
                .is_some()
            {
                match manifest_store.load_manifest(&manifest.snapshot_id) {
                    Ok(Some(snapshot)) => return Ok(Some(snapshot)),
                    Ok(None) => continue,
                    Err(error) => {
                        info!(
                            repo_id = %repo.repo_id,
                            snapshot_id = %manifest.snapshot_id,
                            error = %error,
                            "skipping stale prior indexed snapshot manifest"
                        );
                        continue;
                    }
                }
            }
        }
        Ok(None)
    }

    fn extract_file_facts(
        &self,
        snapshot: &ComposedSnapshot,
        path: &str,
    ) -> HyperindexResult<Option<FileFacts>> {
        let mut parser = ParseCore::with_settings(ParseCoreSettings {
            diagnostics_max_per_file: self.loaded.config.parser.diagnostics_max_per_file,
        });
        let artifact = parser
            .parse_file_from_snapshot(snapshot, path)
            .map_err(|error| HyperindexError::Message(error.to_string()))?;
        Ok(artifact.and_then(|artifact| {
            FactWorkspace
                .extract(&snapshot.repo_id, &snapshot.snapshot_id, &[artifact])
                .files
                .into_iter()
                .next()
                .map(|facts| facts.facts)
        }))
    }

    fn search_limit(&self, requested: u32) -> usize {
        let max = self.loaded.config.symbol_index.max_search_limit.max(1);
        let default = self.loaded.config.symbol_index.default_search_limit.max(1);
        let requested = if requested == 0 {
            default
        } else {
            requested as usize
        };
        requested.min(max)
    }

    fn symbol_store(&self, repo: &RepoRecord) -> HyperindexResult<SymbolStore> {
        SymbolStore::open(&self.loaded.config.symbol_index.store_dir, &repo.repo_id)
            .map_err(symbol_store_error)
    }
}

pub fn scan_parse_runtime_status(loaded: &LoadedConfig) -> HyperindexResult<ParseRuntimeStatus> {
    let artifact_dir = &loaded.config.parser.artifact_dir;
    let mut repo_ids = BTreeSet::new();
    let mut build_count = 0usize;
    scan_parse_builds(artifact_dir, artifact_dir, &mut repo_ids, &mut build_count)?;
    Ok(ParseRuntimeStatus {
        enabled: loaded.config.parser.enabled,
        artifact_dir: artifact_dir.display().to_string(),
        repo_count: repo_ids.len(),
        build_count,
    })
}

pub fn scan_symbol_runtime_status(
    loaded: &LoadedConfig,
) -> HyperindexResult<SymbolIndexRuntimeStatus> {
    let store_dir = &loaded.config.symbol_index.store_dir;
    if !store_dir.exists() {
        return Ok(SymbolIndexRuntimeStatus {
            enabled: loaded.config.symbol_index.enabled,
            store_dir: store_dir.display().to_string(),
            repo_count: 0,
            indexed_snapshot_count: 0,
        });
    }

    let mut repo_count = 0usize;
    let mut indexed_snapshot_count = 0usize;
    for entry in fs::read_dir(store_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(repo_id) = name.strip_suffix(".symbols.sqlite3") else {
            continue;
        };
        let status = SymbolStore::open(store_dir, repo_id)
            .map_err(symbol_store_error)?
            .status()
            .map_err(symbol_store_error)?;
        repo_count += 1;
        indexed_snapshot_count += status.indexed_snapshots;
    }

    Ok(SymbolIndexRuntimeStatus {
        enabled: loaded.config.symbol_index.enabled,
        store_dir: store_dir.display().to_string(),
        repo_count,
        indexed_snapshot_count,
    })
}

fn protocol_parse_build_record(build: &ParseBuildStatus) -> ParseBuildRecord {
    let build_id = ParseBuildId(build.build_id.clone());
    ParseBuildRecord {
        build_id: build_id.clone(),
        state: ParseBuildState::Succeeded,
        requested_at: epoch_ms_string(build.created_at_epoch_ms),
        started_at: Some(epoch_ms_string(build.created_at_epoch_ms)),
        finished_at: Some(epoch_ms_string(build.created_at_epoch_ms)),
        counts: ParseBuildCounts {
            planned_file_count: build.stats.planned_file_count,
            parsed_file_count: build.stats.parsed_file_count,
            reused_file_count: build.stats.reused_file_count,
            skipped_file_count: build.stats.skipped_file_count,
            diagnostic_count: build.stats.diagnostic_count,
        },
        manifest: Some(ParseArtifactManifest {
            build_id,
            repo_id: build.repo_id.clone(),
            snapshot_id: build.snapshot_id.clone(),
            parser_config_digest: build.parser_config_digest.clone(),
            artifact_root: build.artifact_root.clone(),
            file_count: build.files.len() as u64,
            diagnostic_count: build.stats.diagnostic_count,
            created_at: epoch_ms_string(build.created_at_epoch_ms),
        }),
        loaded_from_existing_build: build.loaded_from_existing_build,
    }
}

fn protocol_symbol_build_record(
    repo: &RepoRecord,
    snapshot: &ComposedSnapshot,
    outcome: &SymbolBuildOutcome,
) -> SymbolIndexBuildRecord {
    let build_id = symbol_build_id(snapshot, &outcome.parser_build);
    SymbolIndexBuildRecord {
        build_id: build_id.clone(),
        state: SymbolIndexBuildState::Succeeded,
        requested_at: epoch_ms_string(outcome.parser_build.created_at_epoch_ms),
        started_at: Some(epoch_ms_string(outcome.parser_build.created_at_epoch_ms)),
        finished_at: Some(epoch_ms_string(outcome.parser_build.created_at_epoch_ms)),
        parser_build_id: ParseBuildId(outcome.parser_build.build_id.clone()),
        stats: symbol_index_stats(&outcome.graph),
        manifest: Some(symbol_index_manifest(repo, snapshot, outcome)),
        refresh_mode: outcome.refresh_mode.clone(),
        fallback_reason: outcome.fallback_reason.clone(),
        loaded_from_existing_build: outcome.loaded_from_existing_build,
    }
}

fn symbol_index_manifest(
    repo: &RepoRecord,
    snapshot: &ComposedSnapshot,
    outcome: &SymbolBuildOutcome,
) -> SymbolIndexManifest {
    SymbolIndexManifest {
        build_id: symbol_build_id(snapshot, &outcome.parser_build),
        repo_id: repo.repo_id.clone(),
        snapshot_id: snapshot.snapshot_id.clone(),
        parser_build_id: ParseBuildId(outcome.parser_build.build_id.clone()),
        created_at: epoch_ms_string(outcome.parser_build.created_at_epoch_ms),
        stats: symbol_index_stats(&outcome.graph),
        storage: SymbolIndexStorage {
            format: SymbolIndexStorageFormat::Sqlite,
            path: outcome.store_status.db_path.clone(),
            schema_version: outcome.store_status.schema_version,
            manifest_sha256: None,
        },
    }
}

fn symbol_index_stats(graph: &SymbolGraph) -> SymbolIndexStats {
    SymbolIndexStats {
        file_count: graph.indexed_files as u64,
        symbol_count: graph.symbol_count as u64,
        occurrence_count: graph.occurrence_count as u64,
        edge_count: graph.edge_count as u64,
        diagnostic_count: collect_graph_diagnostics(graph).len() as u64,
    }
}

fn load_snapshot_facts(
    store: SymbolStore,
    snapshot: &ComposedSnapshot,
) -> HyperindexResult<FactsBatch> {
    let extracted = store
        .load_snapshot_facts(&snapshot.snapshot_id)
        .map_err(symbol_store_error)?;
    Ok(FactsBatch {
        files: extracted.files,
    })
}

fn resolve_with_normalized_selector(
    engine: &SymbolQueryEngine,
    graph: &SymbolGraph,
    selector: &hyperindex_protocol::symbols::SymbolLocationSelector,
    normalized_path: &str,
) -> Option<ResolvedSymbol> {
    match selector {
        hyperindex_protocol::symbols::SymbolLocationSelector::LineColumn {
            line, column, ..
        } => engine.resolve(
            graph,
            &hyperindex_protocol::symbols::SymbolLocationSelector::LineColumn {
                path: normalized_path.to_string(),
                line: *line,
                column: *column,
            },
        ),
        hyperindex_protocol::symbols::SymbolLocationSelector::ByteOffset { offset, .. } => engine
            .resolve(
                graph,
                &hyperindex_protocol::symbols::SymbolLocationSelector::ByteOffset {
                    path: normalized_path.to_string(),
                    offset: *offset,
                },
            ),
    }
}

fn collect_graph_diagnostics(
    graph: &SymbolGraph,
) -> Vec<hyperindex_protocol::symbols::ParseDiagnostic> {
    let mut diagnostics = graph
        .files
        .iter()
        .flat_map(|file| file.diagnostics.iter().cloned())
        .collect::<Vec<_>>();
    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics.dedup();
    diagnostics
}

fn symbol_build_id(
    snapshot: &ComposedSnapshot,
    parse_build: &ParseBuildStatus,
) -> SymbolIndexBuildId {
    SymbolIndexBuildId(format!(
        "symbol-index-{}-{}",
        snapshot.snapshot_id, parse_build.build_id
    ))
}

fn incremental_index_options(loaded: &LoadedConfig) -> IncrementalIndexOptions {
    IncrementalIndexOptions {
        store_root: loaded.config.symbol_index.store_dir.clone(),
        rules: hyperindex_parser::ParseEligibilityRules::from_runtime_config(&loaded.config),
        diagnostics_max_per_file: loaded.config.parser.diagnostics_max_per_file,
    }
}

fn refresh_mode_name(mode: &IncrementalRefreshMode) -> String {
    match mode {
        IncrementalRefreshMode::FullRebuild => "full_rebuild".to_string(),
        IncrementalRefreshMode::Incremental => "incremental".to_string(),
    }
}

fn fallback_reason_name(reason: &RebuildFallbackReason) -> String {
    match reason {
        RebuildFallbackReason::NoPriorSnapshot => "no_prior_snapshot".to_string(),
        RebuildFallbackReason::MissingSnapshotDiff => "missing_snapshot_diff".to_string(),
        RebuildFallbackReason::SchemaVersionChanged => "schema_version_changed".to_string(),
        RebuildFallbackReason::IncompatibleConfigChange => "incompatible_config_change".to_string(),
        RebuildFallbackReason::CacheOrIndexCorruption => "cache_or_index_corruption".to_string(),
        RebuildFallbackReason::UnresolvedConsistencyIssue => {
            "unresolved_consistency_issue".to_string()
        }
    }
}

fn scan_parse_builds(
    root: &Path,
    current: &Path,
    repo_ids: &mut BTreeSet<String>,
    build_count: &mut usize,
) -> HyperindexResult<()> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|value| value.to_str()) == Some("_cache") {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(root) {
                if let Some(repo_id) = relative.components().next().and_then(component_name) {
                    repo_ids.insert(repo_id.to_string());
                }
            }
            scan_parse_builds(root, &path, repo_ids, build_count)?;
            continue;
        }
        if path.file_name().and_then(|value| value.to_str()) == Some("build.json") {
            *build_count += 1;
        }
    }
    Ok(())
}

fn component_name(component: std::path::Component<'_>) -> Option<&str> {
    match component {
        std::path::Component::Normal(value) => value.to_str(),
        _ => None,
    }
}

fn epoch_ms_string(value: u128) -> String {
    format!("epoch-ms:{value}")
}

fn io_error(error: std::io::Error) -> HyperindexError {
    HyperindexError::Message(error.to_string())
}

fn symbol_store_error(error: impl std::fmt::Display) -> HyperindexError {
    HyperindexError::Message(error.to_string())
}
