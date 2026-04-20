use std::collections::BTreeMap;
use std::path::Path;

use hyperindex_config::load_or_default;
use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_daemon::symbols::ParserSymbolService;
use hyperindex_parser::{ParseEligibilityRules, ParseManager, SnapshotFileCatalog};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::repo::RepoRecord;
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::status::DaemonStatusParams;
use hyperindex_protocol::symbols::{
    DefinitionLookupParams, DefinitionLookupResponse, ReferenceLookupParams,
    ReferenceLookupResponse, SymbolIndexBuildParams, SymbolIndexBuildResponse,
    SymbolIndexBuildState, SymbolIndexStatusParams, SymbolIndexStatusResponse,
    SymbolLocationSelector, SymbolResolveParams, SymbolResolveResponse, SymbolSearchMode,
    SymbolSearchParams, SymbolSearchQuery, SymbolSearchResponse, SymbolShowParams,
    SymbolShowResponse,
};
use hyperindex_repo_store::RepoStore;
use hyperindex_symbol_store::{
    SnapshotStorageStats, SymbolStore, migrations::SYMBOL_STORE_SCHEMA_VERSION,
};
use hyperindex_symbols::{FactsBatch, SymbolGraphBuilder};
use serde::Serialize;

use crate::client::DaemonClient;

pub fn build(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    force: bool,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::SymbolIndexBuild(SymbolIndexBuildParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        force,
    }))? {
        SuccessPayload::SymbolIndexBuild(response) => render_build(&response, json_output),
        other => Err(unexpected_response("symbol_index_build", other)),
    }
}

pub fn status(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::SymbolIndexStatus(SymbolIndexStatusParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        build_id: None,
    }))? {
        SuccessPayload::SymbolIndexStatus(response) => render_status(&response, json_output),
        other => Err(unexpected_response("symbol_index_status", other)),
    }
}

pub fn rebuild(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    if daemon_status(config_path).is_ok() {
        return build(config_path, repo_id, snapshot_id, true, json_output);
    }

    let context = LocalSymbolContext::load(config_path, repo_id, snapshot_id)?;
    let response = context
        .service()
        .symbol_index_build(
            &context.repo,
            &context.snapshot,
            &SymbolIndexBuildParams {
                repo_id: repo_id.to_string(),
                snapshot_id: snapshot_id.to_string(),
                force: true,
            },
        )
        .map_err(|error| HyperindexError::Message(error.to_string()))?;
    render_build(&response, json_output)
}

pub fn stats(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let report = inspect_local_symbol_state(config_path, repo_id, snapshot_id)?;
    if json_output {
        return render_json(&report);
    }
    Ok(render_local_symbol_report("symbol stats", &report))
}

pub fn doctor(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let report = inspect_local_symbol_state(config_path, repo_id, snapshot_id)?;
    if json_output {
        return render_json(&report);
    }
    Ok(render_local_symbol_report("symbol doctor", &report))
}

pub fn search(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    query: &str,
    limit: usize,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::SymbolSearch(SymbolSearchParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        query: SymbolSearchQuery {
            text: query.to_string(),
            mode: SymbolSearchMode::Exact,
            kinds: Vec::new(),
            path_prefix: None,
        },
        limit: limit as u32,
    }))? {
        SuccessPayload::SymbolSearch(response) => render_search(&response, json_output),
        other => Err(unexpected_response("symbol_search", other)),
    }
}

pub fn show(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    symbol_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::SymbolShow(SymbolShowParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        symbol_id: hyperindex_protocol::symbols::SymbolId(symbol_id.to_string()),
    }))? {
        SuccessPayload::SymbolShow(response) => render_show(&response, json_output),
        other => Err(unexpected_response("symbol_show", other)),
    }
}

pub fn definitions(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    symbol_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::DefinitionLookup(DefinitionLookupParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        symbol_id: hyperindex_protocol::symbols::SymbolId(symbol_id.to_string()),
    }))? {
        SuccessPayload::DefinitionLookup(response) => render_definitions(&response, json_output),
        other => Err(unexpected_response("definition_lookup", other)),
    }
}

pub fn references(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    symbol_id: &str,
    limit: Option<usize>,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::ReferenceLookup(ReferenceLookupParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        symbol_id: hyperindex_protocol::symbols::SymbolId(symbol_id.to_string()),
        limit: limit.map(|value| value as u32),
    }))? {
        SuccessPayload::ReferenceLookup(response) => render_references(&response, json_output),
        other => Err(unexpected_response("reference_lookup", other)),
    }
}

pub fn resolve_line_column(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    path: &str,
    line: u32,
    column: u32,
    json_output: bool,
) -> HyperindexResult<String> {
    resolve(
        config_path,
        repo_id,
        snapshot_id,
        SymbolLocationSelector::LineColumn {
            path: path.to_string(),
            line,
            column,
        },
        json_output,
    )
}

pub fn resolve_offset(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    path: &str,
    offset: u32,
    json_output: bool,
) -> HyperindexResult<String> {
    resolve(
        config_path,
        repo_id,
        snapshot_id,
        SymbolLocationSelector::ByteOffset {
            path: path.to_string(),
            offset,
        },
        json_output,
    )
}

fn resolve(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    selector: SymbolLocationSelector,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::SymbolResolve(SymbolResolveParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        selector,
    }))? {
        SuccessPayload::SymbolResolve(response) => render_resolve(&response, json_output),
        other => Err(unexpected_response("symbol_resolve", other)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSymbolIssue {
    code: &'static str,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSnapshotStats {
    eligible_file_count: usize,
    skipped_file_count: usize,
    broken_file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalParseBuildStats {
    loaded: bool,
    parsed_file_count: u64,
    reused_file_count: u64,
    skipped_file_count: u64,
    diagnostic_count: u64,
    elapsed_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalStoreReport {
    db_path: String,
    schema_version: u32,
    indexed_snapshots: usize,
    quick_check: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalStorageStats {
    indexed_file_count: usize,
    symbol_fact_count: usize,
    occurrence_count: usize,
    edge_count: usize,
    import_fact_count: usize,
    export_fact_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalIndexReport {
    indexed: bool,
    refresh_mode: Option<String>,
    indexed_file_count: Option<usize>,
    storage: Option<LocalStorageStats>,
    symbol_count: Option<usize>,
    occurrence_count: Option<usize>,
    edge_count: Option<usize>,
    diagnostic_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalSymbolReport {
    daemon_reachable: bool,
    repo_id: String,
    snapshot_id: String,
    repo_last_snapshot_id: Option<String>,
    snapshot: LocalSnapshotStats,
    parse_build: Option<LocalParseBuildStats>,
    store: Option<LocalStoreReport>,
    index: LocalIndexReport,
    actions: Vec<String>,
    issues: Vec<LocalSymbolIssue>,
}

struct LocalSymbolContext {
    loaded: hyperindex_config::LoadedConfig,
    repo_store: RepoStore,
    repo: RepoRecord,
    snapshot: ComposedSnapshot,
}

impl LocalSymbolContext {
    fn load(
        config_path: Option<&Path>,
        repo_id: &str,
        snapshot_id: &str,
    ) -> HyperindexResult<Self> {
        let loaded = load_or_default(config_path)?;
        let repo_store = RepoStore::open_from_config(&loaded.config)?;
        let repo = repo_store.show_repo(repo_id)?;
        let snapshot = load_snapshot_for_repo(&repo_store, repo_id, snapshot_id)?;
        Ok(Self {
            loaded,
            repo_store,
            repo,
            snapshot,
        })
    }

    fn service(&self) -> ParserSymbolService {
        ParserSymbolService::from_loaded_config(&self.loaded)
    }

    fn symbol_store(&self) -> HyperindexResult<SymbolStore> {
        SymbolStore::open(
            &self.loaded.config.symbol_index.store_dir,
            &self.repo.repo_id,
        )
        .map_err(|error| HyperindexError::Message(error.to_string()))
    }
}

fn inspect_local_symbol_state(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
) -> HyperindexResult<LocalSymbolReport> {
    let context = LocalSymbolContext::load(config_path, repo_id, snapshot_id)?;
    let daemon_reachable = daemon_status(config_path).is_ok();
    let rules = ParseEligibilityRules::from_runtime_config(&context.loaded.config);
    let catalog = SnapshotFileCatalog::build(&context.snapshot, &rules);
    let parse_build = ParseManager::from_runtime_config(&context.loaded.config)
        .load_build_status(&context.snapshot)
        .map_err(|error| HyperindexError::Message(error.to_string()))?
        .map(|build| LocalParseBuildStats {
            loaded: build.loaded_from_existing_build,
            parsed_file_count: build.stats.parsed_file_count,
            reused_file_count: build.stats.reused_file_count,
            skipped_file_count: build.stats.skipped_file_count,
            diagnostic_count: build.stats.diagnostic_count,
            elapsed_ms: build.stats.elapsed_ms,
        });

    let expected_by_path = catalog
        .eligible_files
        .iter()
        .map(|file| {
            (
                file.path.clone(),
                (file.content_sha256.clone(), file.content_bytes as u64),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut issues = Vec::new();
    let mut actions = Vec::new();
    let mut broken_file_count = 0usize;
    let mut index = LocalIndexReport {
        indexed: false,
        refresh_mode: None,
        indexed_file_count: None,
        storage: None,
        symbol_count: None,
        occurrence_count: None,
        edge_count: None,
        diagnostic_count: None,
    };
    let mut store = None;

    if let Some(last_snapshot_id) = context.repo.last_snapshot_id.as_deref() {
        match context.repo_store.load_manifest(last_snapshot_id) {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                issues.push(LocalSymbolIssue {
                    code: "stale_manifest_ref",
                    message: format!(
                        "repo {} points at missing or unreadable snapshot manifest {}",
                        context.repo.repo_id, last_snapshot_id
                    ),
                });
            }
        }
    }

    match context.symbol_store() {
        Ok(symbol_store) => {
            let status = symbol_store
                .status()
                .map_err(|error| HyperindexError::Message(error.to_string()))?;
            let quick_check = symbol_store
                .quick_check()
                .map_err(|error| HyperindexError::Message(error.to_string()))?;
            if quick_check.iter().any(|result| result != "ok") {
                issues.push(LocalSymbolIssue {
                    code: "symbol_store_corrupt",
                    message: format!(
                        "symbol store quick_check reported: {}",
                        quick_check.join("; ")
                    ),
                });
            }
            if status.schema_version != SYMBOL_STORE_SCHEMA_VERSION as u32 {
                issues.push(LocalSymbolIssue {
                    code: "symbol_store_schema_mismatch",
                    message: format!(
                        "symbol store user_version {} does not match expected schema {}",
                        status.schema_version, SYMBOL_STORE_SCHEMA_VERSION
                    ),
                });
            }
            store = Some(LocalStoreReport {
                db_path: status.db_path.clone(),
                schema_version: status.schema_version,
                indexed_snapshots: status.indexed_snapshots,
                quick_check: quick_check.clone(),
            });

            let indexed_state = symbol_store
                .load_indexed_snapshot_state(snapshot_id)
                .map_err(|error| HyperindexError::Message(error.to_string()))?;
            if let Some(indexed_state) = indexed_state {
                index.indexed = true;
                index.refresh_mode = Some(indexed_state.refresh_mode.clone());
                index.indexed_file_count = Some(indexed_state.indexed_file_count);
                if indexed_state.repo_id != repo_id {
                    issues.push(LocalSymbolIssue {
                        code: "indexed_snapshot_repo_mismatch",
                        message: format!(
                            "indexed snapshot {} belongs to repo {} instead of {}",
                            snapshot_id, indexed_state.repo_id, repo_id
                        ),
                    });
                }
                if indexed_state.schema_version != SYMBOL_STORE_SCHEMA_VERSION as u32 {
                    issues.push(LocalSymbolIssue {
                        code: "indexed_snapshot_schema_mismatch",
                        message: format!(
                            "indexed snapshot schema {} does not match expected {}",
                            indexed_state.schema_version, SYMBOL_STORE_SCHEMA_VERSION
                        ),
                    });
                }
                if indexed_state.indexed_file_count != catalog.eligible_files.len() {
                    issues.push(LocalSymbolIssue {
                        code: "indexed_file_count_mismatch",
                        message: format!(
                            "indexed snapshot recorded {} files but snapshot currently resolves {} eligible files",
                            indexed_state.indexed_file_count,
                            catalog.eligible_files.len()
                        ),
                    });
                }

                let storage_stats = symbol_store
                    .snapshot_storage_stats(snapshot_id)
                    .map_err(|error| HyperindexError::Message(error.to_string()))?;
                index.storage = Some(storage_stats_report(&storage_stats));
                if storage_stats.indexed_file_count != catalog.eligible_files.len() {
                    issues.push(LocalSymbolIssue {
                        code: "stored_rows_mismatch",
                        message: format!(
                            "indexed_files contains {} rows but snapshot currently resolves {} eligible files",
                            storage_stats.indexed_file_count,
                            catalog.eligible_files.len()
                        ),
                    });
                }

                match symbol_store.load_snapshot_facts(snapshot_id) {
                    Ok(extracted) => {
                        broken_file_count = extracted
                            .files
                            .iter()
                            .filter(|file| !file.facts.diagnostics.is_empty())
                            .count();
                        let diagnostic_count = extracted
                            .files
                            .iter()
                            .map(|file| file.facts.diagnostics.len())
                            .sum::<usize>();
                        let graph = SymbolGraphBuilder.build_with_snapshot(
                            &FactsBatch {
                                files: extracted.files.clone(),
                            },
                            &context.snapshot,
                        );
                        index.symbol_count = Some(graph.symbol_count);
                        index.occurrence_count = Some(graph.occurrence_count);
                        index.edge_count = Some(graph.edge_count);
                        index.diagnostic_count = Some(diagnostic_count);

                        if extracted.files.len() != catalog.eligible_files.len() {
                            issues.push(LocalSymbolIssue {
                                code: "facts_batch_mismatch",
                                message: format!(
                                    "stored facts contain {} files but snapshot currently resolves {} eligible files",
                                    extracted.files.len(),
                                    catalog.eligible_files.len()
                                ),
                            });
                        }

                        for file in &extracted.files {
                            match hyperindex_core::normalize_repo_relative_path(
                                &file.facts.path,
                                "stored symbol fact",
                            ) {
                                Ok(normalized) if normalized != file.facts.path => {
                                    issues.push(LocalSymbolIssue {
                                        code: "stored_path_not_normalized",
                                        message: format!(
                                            "stored symbol fact path {} normalizes to {}",
                                            file.facts.path, normalized
                                        ),
                                    });
                                }
                                Err(error) => {
                                    issues.push(LocalSymbolIssue {
                                        code: "stored_path_invalid",
                                        message: format!(
                                            "stored symbol fact path {} is invalid: {}",
                                            file.facts.path, error
                                        ),
                                    });
                                }
                                Ok(_) => {}
                            }

                            match expected_by_path.get(&file.facts.path) {
                                Some((sha, bytes))
                                    if file.artifact.content_sha256 == *sha
                                        && file.artifact.content_bytes == *bytes => {}
                                Some(_) => {
                                    issues.push(LocalSymbolIssue {
                                        code: "stale_manifest_or_index",
                                        message: format!(
                                            "stored symbol facts for {} do not match the current snapshot contents",
                                            file.facts.path
                                        ),
                                    });
                                }
                                None => {
                                    issues.push(LocalSymbolIssue {
                                        code: "indexed_path_missing",
                                        message: format!(
                                            "stored symbol facts include {} which is no longer an eligible snapshot file",
                                            file.facts.path
                                        ),
                                    });
                                }
                            }
                        }
                    }
                    Err(error) => issues.push(LocalSymbolIssue {
                        code: "symbol_facts_corrupt",
                        message: format!(
                            "failed to load stored symbol facts for snapshot {}: {}",
                            snapshot_id, error
                        ),
                    }),
                }
            } else {
                issues.push(LocalSymbolIssue {
                    code: "symbol_index_missing",
                    message: format!("no stored symbol index exists for snapshot {}", snapshot_id),
                });
            }
        }
        Err(error) => issues.push(LocalSymbolIssue {
            code: "symbol_store_unavailable",
            message: format!("failed to open symbol store: {}", error),
        }),
    }

    if !issues.is_empty() {
        actions.push(format!(
            "run `hyperctl symbol rebuild --repo-id {} --snapshot-id {}` to rebuild the symbol index",
            repo_id, snapshot_id
        ));
        if issues.iter().any(|issue| {
            issue.code == "symbol_store_corrupt" || issue.code == "symbol_store_unavailable"
        }) {
            actions.push(
                "if rebuild cannot open the store, stop the daemon and run `hyperctl reset-runtime`"
                    .to_string(),
            );
        }
    }

    Ok(LocalSymbolReport {
        daemon_reachable,
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        repo_last_snapshot_id: context.repo.last_snapshot_id.clone(),
        snapshot: LocalSnapshotStats {
            eligible_file_count: catalog.eligible_files.len(),
            skipped_file_count: catalog.skipped_files.len(),
            broken_file_count,
        },
        parse_build,
        store,
        index,
        actions,
        issues,
    })
}

fn load_snapshot_for_repo(
    repo_store: &RepoStore,
    repo_id: &str,
    snapshot_id: &str,
) -> HyperindexResult<ComposedSnapshot> {
    let snapshot = repo_store.load_manifest(snapshot_id)?.ok_or_else(|| {
        HyperindexError::Message(format!("snapshot {} was not found", snapshot_id))
    })?;
    if snapshot.repo_id != repo_id {
        return Err(HyperindexError::Message(format!(
            "snapshot {} does not belong to repo {}",
            snapshot_id, repo_id
        )));
    }
    Ok(snapshot)
}

fn storage_stats_report(stats: &SnapshotStorageStats) -> LocalStorageStats {
    LocalStorageStats {
        indexed_file_count: stats.indexed_file_count,
        symbol_fact_count: stats.symbol_fact_count,
        occurrence_count: stats.occurrence_count,
        edge_count: stats.edge_count,
        import_fact_count: stats.import_fact_count,
        export_fact_count: stats.export_fact_count,
    }
}

fn daemon_status(config_path: Option<&Path>) -> HyperindexResult<()> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::DaemonStatus(DaemonStatusParams::default()))? {
        SuccessPayload::DaemonStatus(_) => Ok(()),
        other => Err(HyperindexError::Message(format!(
            "unexpected daemon status response: {other:?}"
        ))),
    }
}

fn render_local_symbol_report(title: &str, report: &LocalSymbolReport) -> String {
    let mut lines = vec![
        format!("{title} {}", report.snapshot_id),
        format!("daemon_reachable: {}", report.daemon_reachable),
        format!(
            "repo_last_snapshot_id: {}",
            report.repo_last_snapshot_id.as_deref().unwrap_or("-")
        ),
        format!("eligible_files: {}", report.snapshot.eligible_file_count),
        format!("skipped_files: {}", report.snapshot.skipped_file_count),
        format!("broken_files: {}", report.snapshot.broken_file_count),
    ];
    if let Some(parse_build) = &report.parse_build {
        lines.push(format!(
            "parse_build: parsed={} reused={} skipped={} diagnostics={} elapsed_ms={}",
            parse_build.parsed_file_count,
            parse_build.reused_file_count,
            parse_build.skipped_file_count,
            parse_build.diagnostic_count,
            parse_build.elapsed_ms
        ));
    } else {
        lines.push("parse_build: -".to_string());
    }
    if let Some(store) = &report.store {
        lines.push(format!("store_schema_version: {}", store.schema_version));
        lines.push(format!(
            "store_indexed_snapshots: {}",
            store.indexed_snapshots
        ));
        lines.push(format!(
            "store_quick_check: {}",
            store.quick_check.join("; ")
        ));
    } else {
        lines.push("store_schema_version: -".to_string());
    }
    lines.push(format!("indexed: {}", report.index.indexed));
    lines.push(format!(
        "refresh_mode: {}",
        report.index.refresh_mode.as_deref().unwrap_or("-")
    ));
    lines.push(format!(
        "indexed_file_count: {}",
        report
            .index
            .indexed_file_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    ));
    lines.push(format!(
        "symbol_count: {}",
        report
            .index
            .symbol_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    ));
    lines.push(format!(
        "occurrence_count: {}",
        report
            .index
            .occurrence_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    ));
    lines.push(format!(
        "edge_count: {}",
        report
            .index
            .edge_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    ));
    if report.actions.is_empty() {
        lines.push("actions: -".to_string());
    } else {
        lines.push(format!("actions: {}", report.actions.join("; ")));
    }
    if report.issues.is_empty() {
        lines.push("issues: none".to_string());
    } else {
        lines.push(format!("issues: {}", report.issues.len()));
        lines.extend(
            report
                .issues
                .iter()
                .map(|issue| format!("- [{}] {}", issue.code, issue.message)),
        );
    }
    lines.join("\n")
}

fn render_build(
    response: &SymbolIndexBuildResponse,
    json_output: bool,
) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    Ok(render_build_summary("symbol build", &response.build))
}

fn render_status(
    response: &SymbolIndexStatusResponse,
    json_output: bool,
) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    match response.builds.first() {
        Some(build) => Ok(render_build_summary("symbol status", build)),
        None => Ok(format!(
            "symbol status {}\nno persisted symbol index found",
            response.snapshot_id
        )),
    }
}

fn render_search(response: &SymbolSearchResponse, json_output: bool) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    if response.hits.is_empty() {
        return Ok(format!("symbol search {}\nno hits", response.snapshot_id));
    }
    Ok(response
        .hits
        .iter()
        .enumerate()
        .map(|(index, hit)| {
            format!(
                "{}. {} [{}] {} ({})",
                index + 1,
                hit.symbol.display_name,
                format!("{:?}", hit.symbol.kind).to_lowercase(),
                hit.symbol.path,
                hit.reason
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn render_show(response: &SymbolShowResponse, json_output: bool) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    Ok([
        format!("symbol_id: {}", response.symbol.symbol_id.0),
        format!("display_name: {}", response.symbol.display_name),
        format!("kind: {:?}", response.symbol.kind).to_lowercase(),
        format!("path: {}", response.symbol.path),
        format!("definitions: {}", response.definitions.len()),
        format!("related_edges: {}", response.related_edges.len()),
    ]
    .join("\n"))
}

fn render_definitions(
    response: &DefinitionLookupResponse,
    json_output: bool,
) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    Ok(render_occurrences("definitions", &response.definitions))
}

fn render_references(
    response: &ReferenceLookupResponse,
    json_output: bool,
) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    Ok(render_occurrences("references", &response.references))
}

fn render_resolve(response: &SymbolResolveResponse, json_output: bool) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    match &response.resolution {
        Some(resolution) => Ok([
            format!("symbol_id: {}", resolution.symbol.symbol_id.0),
            format!("display_name: {}", resolution.symbol.display_name),
            format!("path: {}", resolution.symbol.path),
            format!(
                "occurrence: {}",
                resolution
                    .occurrence
                    .as_ref()
                    .map(|occurrence| occurrence.occurrence_id.0.clone())
                    .unwrap_or_else(|| "-".to_string())
            ),
        ]
        .join("\n")),
        None => Ok("no symbol resolved".to_string()),
    }
}

fn render_build_summary(
    title: &str,
    build: &hyperindex_protocol::symbols::SymbolIndexBuildRecord,
) -> String {
    format!(
        "{title} {}\nstate: {}\nfiles: {}\nsymbols: {}\noccurrences: {}\nedges: {}\nrefresh_mode: {}\nfallback_reason: {}\nloaded_from_existing_build: {}",
        build.build_id.0,
        match build.state {
            SymbolIndexBuildState::Queued => "queued",
            SymbolIndexBuildState::Running => "running",
            SymbolIndexBuildState::Succeeded => "succeeded",
            SymbolIndexBuildState::Failed => "failed",
        },
        build.stats.file_count,
        build.stats.symbol_count,
        build.stats.occurrence_count,
        build.stats.edge_count,
        build.refresh_mode.as_deref().unwrap_or("-"),
        build.fallback_reason.as_deref().unwrap_or("-"),
        build.loaded_from_existing_build,
    )
}

fn render_occurrences(
    title: &str,
    occurrences: &[hyperindex_protocol::symbols::SymbolOccurrence],
) -> String {
    if occurrences.is_empty() {
        return format!("{title}: none");
    }
    occurrences
        .iter()
        .map(|occurrence| {
            format!(
                "{} {}:{} {:?}",
                occurrence.path,
                occurrence.span.start.line,
                occurrence.span.start.column,
                occurrence.role
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_json<T: serde::Serialize>(response: &T) -> HyperindexResult<String> {
    serde_json::to_string_pretty(response)
        .map_err(|error| HyperindexError::Message(format!("failed to render json: {error}")))
}

fn unexpected_response(method: &str, other: SuccessPayload) -> HyperindexError {
    HyperindexError::Message(format!("unexpected {method} response: {other:?}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use hyperindex_protocol::config::{RuntimeConfig, TransportKind};
    use hyperindex_protocol::repo::ReposAddResponse;
    use hyperindex_protocol::snapshot::{SnapshotCreateParams, SnapshotCreateResponse};
    use hyperindex_protocol::symbols::{
        ByteRange, LinePosition, OccurrenceId, OccurrenceRole, ParseBuildId, SourceSpan, SymbolId,
        SymbolIndexBuildId, SymbolIndexBuildRecord, SymbolIndexBuildResponse,
        SymbolIndexBuildState, SymbolIndexStats, SymbolOccurrence, SymbolResolveResponse,
        SymbolSearchResponse,
    };
    use tempfile::tempdir;

    use super::{render_build_summary, render_occurrences};
    use crate::commands::{buffers, parse, repo, snapshot, symbol};

    #[test]
    fn build_summary_includes_refresh_details() {
        let build = SymbolIndexBuildRecord {
            build_id: SymbolIndexBuildId("symbol-index-1".to_string()),
            state: SymbolIndexBuildState::Succeeded,
            requested_at: "epoch-ms:1".to_string(),
            started_at: Some("epoch-ms:1".to_string()),
            finished_at: Some("epoch-ms:2".to_string()),
            parser_build_id: ParseBuildId("parse-1".to_string()),
            stats: SymbolIndexStats {
                file_count: 2,
                symbol_count: 4,
                occurrence_count: 8,
                edge_count: 6,
                diagnostic_count: 0,
            },
            manifest: None,
            refresh_mode: Some("incremental".to_string()),
            fallback_reason: None,
            loaded_from_existing_build: false,
        };

        let rendered = render_build_summary("symbol build", &build);
        assert!(rendered.contains("refresh_mode: incremental"));
    }

    #[test]
    fn occurrence_renderer_outputs_line_column() {
        let rendered = render_occurrences(
            "references",
            &[SymbolOccurrence {
                occurrence_id: OccurrenceId("occ-1".to_string()),
                symbol_id: SymbolId("sym-1".to_string()),
                path: "src/app.ts".to_string(),
                span: SourceSpan {
                    start: LinePosition { line: 3, column: 5 },
                    end: LinePosition { line: 3, column: 8 },
                    bytes: ByteRange { start: 10, end: 13 },
                },
                role: OccurrenceRole::Reference,
            }],
        );
        assert!(rendered.contains("src/app.ts 3:5"));
    }

    #[test]
    fn daemon_backed_cli_commands_refresh_symbol_results() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);

        let added = repo::add(
            Some(&config_path),
            &repo_root,
            Some("CLI Repo".to_string()),
            Vec::new(),
            Vec::new(),
            true,
        )
        .unwrap();
        let added: ReposAddResponse = serde_json::from_str(&added).unwrap();

        let clean = snapshot::create(
            Some(&config_path),
            &SnapshotCreateParams {
                repo_id: added.repo.repo_id.clone(),
                include_working_tree: true,
                buffer_ids: Vec::new(),
            },
            true,
        )
        .unwrap();
        let clean: SnapshotCreateResponse = serde_json::from_str(&clean).unwrap();

        let parse_build = parse::build(
            Some(&config_path),
            &added.repo.repo_id,
            &clean.snapshot.snapshot_id,
            false,
            true,
        )
        .unwrap();
        assert!(parse_build.contains("\"reused_file_count\""));
        assert!(parse_build.contains("\"build_id\""));

        let symbol_build = symbol::build(
            Some(&config_path),
            &added.repo.repo_id,
            &clean.snapshot.snapshot_id,
            false,
            true,
        )
        .unwrap();
        let symbol_build: SymbolIndexBuildResponse = serde_json::from_str(&symbol_build).unwrap();
        assert_eq!(
            symbol_build.build.refresh_mode.as_deref(),
            Some("full_rebuild")
        );

        let clean_search = symbol::search(
            Some(&config_path),
            &added.repo.repo_id,
            &clean.snapshot.snapshot_id,
            "createSession",
            5,
            true,
        )
        .unwrap();
        let clean_search: SymbolSearchResponse = serde_json::from_str(&clean_search).unwrap();
        assert_eq!(clean_search.hits.len(), 1);

        let buffer_path = tempdir.path().join("buffer.ts");
        fs::write(
            &buffer_path,
            "export function createBufferedSession() {\n  return \"buffer\";\n}\n\nexport function run() {\n  return createBufferedSession();\n}\n",
        )
        .unwrap();
        buffers::set_from_file(
            Some(&config_path),
            &added.repo.repo_id,
            "buffer-1",
            "src/app.ts",
            &buffer_path,
            1,
            Some("typescript".to_string()),
            true,
        )
        .unwrap();

        let buffered = snapshot::create(
            Some(&config_path),
            &SnapshotCreateParams {
                repo_id: added.repo.repo_id.clone(),
                include_working_tree: true,
                buffer_ids: vec!["buffer-1".to_string()],
            },
            true,
        )
        .unwrap();
        let buffered: SnapshotCreateResponse = serde_json::from_str(&buffered).unwrap();

        let refreshed = symbol::build(
            Some(&config_path),
            &added.repo.repo_id,
            &buffered.snapshot.snapshot_id,
            false,
            true,
        )
        .unwrap();
        let refreshed: SymbolIndexBuildResponse = serde_json::from_str(&refreshed).unwrap();
        assert_eq!(refreshed.build.refresh_mode.as_deref(), Some("incremental"));

        let buffered_search = symbol::search(
            Some(&config_path),
            &added.repo.repo_id,
            &buffered.snapshot.snapshot_id,
            "createBufferedSession",
            5,
            true,
        )
        .unwrap();
        let buffered_search: SymbolSearchResponse = serde_json::from_str(&buffered_search).unwrap();
        assert_eq!(buffered_search.hits.len(), 1);

        let resolved = symbol::resolve_line_column(
            Some(&config_path),
            &added.repo.repo_id,
            &buffered.snapshot.snapshot_id,
            "src/app.ts",
            6,
            10,
            true,
        )
        .unwrap();
        let resolved: SymbolResolveResponse = serde_json::from_str(&resolved).unwrap();
        assert_eq!(
            resolved
                .resolution
                .as_ref()
                .map(|resolution| resolution.symbol.display_name.as_str()),
            Some("createBufferedSession")
        );
    }

    #[test]
    fn local_symbol_operator_commands_report_clean_state() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);

        let added = repo::add(
            Some(&config_path),
            &repo_root,
            Some("CLI Repo".to_string()),
            Vec::new(),
            Vec::new(),
            true,
        )
        .unwrap();
        let added: ReposAddResponse = serde_json::from_str(&added).unwrap();
        let created = snapshot::create(
            Some(&config_path),
            &SnapshotCreateParams {
                repo_id: added.repo.repo_id.clone(),
                include_working_tree: true,
                buffer_ids: Vec::new(),
            },
            true,
        )
        .unwrap();
        let created: SnapshotCreateResponse = serde_json::from_str(&created).unwrap();

        let rebuilt = symbol::rebuild(
            Some(&config_path),
            &added.repo.repo_id,
            &created.snapshot.snapshot_id,
            true,
        )
        .unwrap();
        let rebuilt: SymbolIndexBuildResponse = serde_json::from_str(&rebuilt).unwrap();
        assert_eq!(
            rebuilt.build.stats.file_count, 1,
            "local rebuild should succeed without a daemon"
        );

        let stats = symbol::stats(
            Some(&config_path),
            &added.repo.repo_id,
            &created.snapshot.snapshot_id,
            true,
        )
        .unwrap();
        assert!(stats.contains("\"eligible_file_count\": 1"));
        assert!(stats.contains("\"indexed\": true"));

        let doctor = symbol::doctor(
            Some(&config_path),
            &added.repo.repo_id,
            &created.snapshot.snapshot_id,
            true,
        )
        .unwrap();
        assert!(doctor.contains("\"issues\": []"));
    }

    fn write_test_config(root: &Path) -> PathBuf {
        let config_path = root.join("config.toml");
        let runtime_root = root.join(".hyperindex");
        let state_dir = runtime_root.join("state");
        let manifests_dir = runtime_root.join("data/manifests");

        let mut config = RuntimeConfig::default();
        config.directories.runtime_root = runtime_root.clone();
        config.directories.state_dir = state_dir.clone();
        config.directories.data_dir = runtime_root.join("data");
        config.directories.manifests_dir = manifests_dir.clone();
        config.directories.logs_dir = runtime_root.join("logs");
        config.directories.temp_dir = runtime_root.join("tmp");
        config.transport.kind = TransportKind::Stdio;
        config.transport.socket_path = runtime_root.join("hyperd.sock");
        config.repo_registry.sqlite_path = state_dir.join("runtime.sqlite3");
        config.repo_registry.manifests_dir = manifests_dir;
        config.parser.artifact_dir = runtime_root.join("data/parse-artifacts");
        config.symbol_index.store_dir = runtime_root.join("data/symbols");
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        config_path
    }

    fn init_repo(repo_root: &Path) {
        fs::create_dir_all(repo_root.join("src")).unwrap();
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["checkout", "-b", "trunk"]);
        fs::write(
            repo_root.join("src/app.ts"),
            "export function createSession() {\n  return \"disk\";\n}\n\nexport function run() {\n  return createSession();\n}\n",
        )
        .unwrap();
        run_git(repo_root, &["add", "."]);
        commit_all(repo_root, "initial");
    }

    fn commit_all(repo_root: &Path, message: &str) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .arg("commit")
            .arg("-m")
            .arg(message)
            .env("GIT_AUTHOR_NAME", "Codex")
            .env("GIT_AUTHOR_EMAIL", "codex@example.com")
            .env("GIT_COMMITTER_NAME", "Codex")
            .env("GIT_COMMITTER_EMAIL", "codex@example.com")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_git(repo_root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
