use std::collections::BTreeSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use hyperindex_config::load_or_default;
use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_daemon::impact::build_graph_from_store;
use hyperindex_impact::IncrementalImpactBuilder;
use hyperindex_impact_store::{ImpactStore, StoredImpactBuild};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::impact::{
    ImpactAnalyzeParams, ImpactAnalyzeResponse, ImpactChangeScenario, ImpactEntityRef,
    ImpactExplainParams, ImpactExplainResponse, ImpactStatusParams, ImpactStatusResponse,
    ImpactTargetKind, ImpactTargetRef,
};
use hyperindex_protocol::impact::{ImpactRefreshMode, ImpactRefreshTrigger};
use hyperindex_protocol::repo::RepoRecord;
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_repo_store::RepoStore;
use hyperindex_symbol_store::{SymbolStore, migrations::SYMBOL_STORE_SCHEMA_VERSION};
use serde::Serialize;

use crate::client::DaemonClient;

pub fn status(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::ImpactStatus(ImpactStatusParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
    }))? {
        SuccessPayload::ImpactStatus(response) => render_status(&response, json_output),
        other => Err(unexpected_response("impact_status", other)),
    }
}

pub fn analyze(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    target_kind: &str,
    value: &str,
    change_hint: &str,
    limit: u32,
    include_transitive: bool,
    include_reason_paths: bool,
    max_transitive_depth: Option<u32>,
    max_nodes_visited: Option<u32>,
    max_edges_traversed: Option<u32>,
    max_candidates_considered: Option<u32>,
    json_output: bool,
) -> HyperindexResult<String> {
    let params = build_analyze_params(
        repo_id,
        snapshot_id,
        target_kind,
        value,
        change_hint,
        limit,
        include_transitive,
        include_reason_paths,
        max_transitive_depth,
        max_nodes_visited,
        max_edges_traversed,
        max_candidates_considered,
    )?;

    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::ImpactAnalyze(params))? {
        SuccessPayload::ImpactAnalyze(response) => render_analyze(&response, json_output),
        other => Err(unexpected_response("impact_analyze", other)),
    }
}

pub fn explain(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    target_kind: &str,
    value: &str,
    change_hint: &str,
    impacted_kind: &str,
    impacted_value: &str,
    impacted_path: Option<&str>,
    impacted_symbol_id: Option<&str>,
    max_reason_paths: u32,
    json_output: bool,
) -> HyperindexResult<String> {
    let params = build_explain_params(
        repo_id,
        snapshot_id,
        target_kind,
        value,
        change_hint,
        impacted_kind,
        impacted_value,
        impacted_path,
        impacted_symbol_id,
        max_reason_paths,
    )?;

    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::ImpactExplain(params))? {
        SuccessPayload::ImpactExplain(response) => render_explain(&response, json_output),
        other => Err(unexpected_response("impact_explain", other)),
    }
}

pub fn rebuild(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let context = LocalImpactContext::load(config_path, repo_id, snapshot_id)?;
    let graph = build_graph_from_store(
        &context.loaded.config.symbol_index.store_dir,
        &context.repo.repo_id,
        &context.snapshot,
    )
    .map_err(|error| HyperindexError::Message(error.message))?;
    let store = context.impact_store()?;
    let builder = IncrementalImpactBuilder::new(&context.loaded.config.impact);
    let refresh = builder.build_full(
        &context.snapshot,
        &graph,
        ImpactRefreshTrigger::Bootstrap,
        None,
    );
    let build = stored_build(&context.snapshot, &store, &refresh);
    store
        .persist_build(&build)
        .map_err(|error| HyperindexError::Message(error.to_string()))?;
    let report = LocalImpactRebuildReport {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        store_path: store.store_path.display().to_string(),
        refresh_mode: build.refresh_mode,
        files_touched: refresh.stats.files_touched,
        entities_recomputed: refresh.stats.entities_recomputed,
        edges_refreshed: refresh.stats.edges_refreshed,
        elapsed_ms: refresh.stats.elapsed_ms,
        fallback_reason: refresh
            .fallback_reason
            .as_ref()
            .map(|reason| reason.as_str().to_string()),
    };
    if json_output {
        return render_json(&report);
    }
    Ok([
        format!("impact rebuild {}", report.snapshot_id),
        format!("repo_id: {}", report.repo_id),
        format!("store_path: {}", report.store_path),
        format!("refresh_mode: {}", report.refresh_mode),
        format!("files_touched: {}", report.files_touched),
        format!("entities_recomputed: {}", report.entities_recomputed),
        format!("edges_refreshed: {}", report.edges_refreshed),
        format!("elapsed_ms: {}", report.elapsed_ms),
        format!(
            "fallback_reason: {}",
            report.fallback_reason.as_deref().unwrap_or("-")
        ),
    ]
    .join("\n"))
}

pub fn stats(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let report = inspect_local_impact_state(config_path, repo_id, snapshot_id)?;
    if json_output {
        return render_json(&report);
    }
    Ok(render_local_impact_report("impact stats", &report))
}

pub fn doctor(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let report = inspect_local_impact_state(config_path, repo_id, snapshot_id)?;
    if json_output {
        return render_json(&report);
    }
    Ok(render_local_impact_report("impact doctor", &report))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalImpactIssue {
    code: &'static str,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalImpactStoreReport {
    db_path: String,
    schema_version: u32,
    build_count: usize,
    quick_check: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalImpactBuildReport {
    materialized: bool,
    refresh_mode: Option<String>,
    fallback_reason: Option<String>,
    file_contribution_count: usize,
    impacted_file_count: usize,
    package_count: usize,
    reverse_reference_symbol_count: usize,
    reverse_dependent_file_count: usize,
    test_association_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalImpactReport {
    daemon_reachable: bool,
    repo_id: String,
    snapshot_id: String,
    repo_last_snapshot_id: Option<String>,
    symbol_index_ready: bool,
    symbol_index_refresh_mode: Option<String>,
    store: Option<LocalImpactStoreReport>,
    build: LocalImpactBuildReport,
    actions: Vec<String>,
    issues: Vec<LocalImpactIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalImpactRebuildReport {
    repo_id: String,
    snapshot_id: String,
    store_path: String,
    refresh_mode: String,
    files_touched: u64,
    entities_recomputed: u64,
    edges_refreshed: u64,
    elapsed_ms: u64,
    fallback_reason: Option<String>,
}

struct LocalImpactContext {
    loaded: hyperindex_config::LoadedConfig,
    repo_store: RepoStore,
    repo: RepoRecord,
    snapshot: ComposedSnapshot,
}

impl LocalImpactContext {
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

    fn impact_store(&self) -> HyperindexResult<ImpactStore> {
        ImpactStore::open_in_store_dir(&self.loaded.config.impact.store_dir, &self.repo.repo_id)
            .map_err(|error| HyperindexError::Message(error.to_string()))
    }

    fn symbol_store(&self) -> HyperindexResult<SymbolStore> {
        SymbolStore::open(
            &self.loaded.config.symbol_index.store_dir,
            &self.repo.repo_id,
        )
        .map_err(|error| HyperindexError::Message(error.to_string()))
    }
}

fn inspect_local_impact_state(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
) -> HyperindexResult<LocalImpactReport> {
    let context = LocalImpactContext::load(config_path, repo_id, snapshot_id)?;
    let daemon_reachable = daemon_status(config_path).is_ok();
    let builder = IncrementalImpactBuilder::new(&context.loaded.config.impact);
    let mut issues = Vec::new();
    let mut actions = Vec::new();
    let mut store = None;
    let mut build = LocalImpactBuildReport {
        materialized: false,
        refresh_mode: None,
        fallback_reason: None,
        file_contribution_count: 0,
        impacted_file_count: 0,
        package_count: 0,
        reverse_reference_symbol_count: 0,
        reverse_dependent_file_count: 0,
        test_association_count: 0,
    };

    if let Some(last_snapshot_id) = context.repo.last_snapshot_id.as_deref() {
        match context.repo_store.load_manifest(last_snapshot_id) {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => issues.push(LocalImpactIssue {
                code: "stale_manifest_ref",
                message: format!(
                    "repo {} points at missing or unreadable snapshot manifest {}",
                    context.repo.repo_id, last_snapshot_id
                ),
            }),
        }
    }

    let symbol_index = context
        .symbol_store()?
        .load_indexed_snapshot_state(snapshot_id)
        .map_err(|error| HyperindexError::Message(error.to_string()))?;
    let symbol_index_ready = symbol_index.is_some();
    let symbol_index_refresh_mode = symbol_index
        .as_ref()
        .map(|state| state.refresh_mode.clone());
    if let Some(indexed_state) = &symbol_index {
        if indexed_state.schema_version != SYMBOL_STORE_SCHEMA_VERSION as u32 {
            issues.push(LocalImpactIssue {
                code: "symbol_store_schema_mismatch",
                message: format!(
                    "symbol index schema {} does not match expected {}",
                    indexed_state.schema_version, SYMBOL_STORE_SCHEMA_VERSION
                ),
            });
        }
    } else {
        issues.push(LocalImpactIssue {
            code: "impact_prerequisite_missing",
            message: format!(
                "no symbol index exists for snapshot {}; run `hyperctl symbol build --repo-id {} --snapshot-id {}` first",
                snapshot_id, repo_id, snapshot_id
            ),
        });
    }

    match context.impact_store() {
        Ok(impact_store) => {
            let status = impact_store
                .status()
                .map_err(|error| HyperindexError::Message(error.to_string()))?;
            let quick_check = impact_store
                .quick_check()
                .map_err(|error| HyperindexError::Message(error.to_string()))?;
            if quick_check.iter().any(|result| result != "ok") {
                issues.push(LocalImpactIssue {
                    code: "impact_store_corrupt",
                    message: format!(
                        "impact store quick_check reported: {}",
                        quick_check.join("; ")
                    ),
                });
            }
            store = Some(LocalImpactStoreReport {
                db_path: status.db_path,
                schema_version: status.schema_version,
                build_count: status.build_count,
                quick_check: quick_check.clone(),
            });

            match impact_store.load_build(snapshot_id) {
                Ok(Some(stored_build)) => {
                    build.materialized = true;
                    build.refresh_mode = Some(stored_build.refresh_mode.clone());
                    build.fallback_reason = stored_build.fallback_reason.clone();
                    build.file_contribution_count = stored_build.state.file_contributions.len();
                    build.impacted_file_count = stored_build.state.plan.impacted_files.len();
                    build.package_count = stored_build.state.plan.packages_by_root.len();
                    build.reverse_reference_symbol_count =
                        stored_build.state.plan.reverse_references_by_symbol.len();
                    build.reverse_dependent_file_count =
                        stored_build.state.plan.reverse_dependents_by_file.len();
                    build.test_association_count = stored_build
                        .state
                        .plan
                        .tests_by_file
                        .values()
                        .map(|associations| associations.len())
                        .sum::<usize>()
                        + stored_build
                            .state
                            .plan
                            .tests_by_symbol
                            .values()
                            .map(|associations| associations.len())
                            .sum::<usize>();

                    if stored_build.repo_id != repo_id {
                        issues.push(LocalImpactIssue {
                            code: "impact_build_repo_mismatch",
                            message: format!(
                                "impact build {} belongs to repo {} instead of {}",
                                snapshot_id, stored_build.repo_id, repo_id
                            ),
                        });
                    }
                    if stored_build.schema_version != impact_store.schema_version {
                        issues.push(LocalImpactIssue {
                            code: "impact_build_schema_mismatch",
                            message: format!(
                                "impact build schema {} does not match store schema {}",
                                stored_build.schema_version, impact_store.schema_version
                            ),
                        });
                    }
                    if stored_build.impact_config_digest != builder.config_digest() {
                        issues.push(LocalImpactIssue {
                            code: "impact_build_config_mismatch",
                            message: "stored impact build was created with a different impact config digest".to_string(),
                        });
                    }

                    if symbol_index_ready {
                        match build_graph_from_store(
                            &context.loaded.config.symbol_index.store_dir,
                            &context.repo.repo_id,
                            &context.snapshot,
                        ) {
                            Ok(graph) => {
                                inspect_build_against_graph(&stored_build, &graph, &mut issues)
                            }
                            Err(error) => issues.push(LocalImpactIssue {
                                code: "symbol_graph_unavailable",
                                message: format!(
                                    "failed to load the symbol graph prerequisite: {}",
                                    error.message
                                ),
                            }),
                        }
                    }
                }
                Ok(None) => issues.push(LocalImpactIssue {
                    code: "impact_build_missing",
                    message: format!(
                        "no materialized impact build exists for snapshot {}",
                        snapshot_id
                    ),
                }),
                Err(error) => issues.push(LocalImpactIssue {
                    code: "impact_build_corrupt",
                    message: format!(
                        "failed to load stored impact build for snapshot {}: {}",
                        snapshot_id, error
                    ),
                }),
            }
        }
        Err(error) => issues.push(LocalImpactIssue {
            code: "impact_store_unavailable",
            message: format!("failed to open impact store: {}", error),
        }),
    }

    if issues
        .iter()
        .any(|issue| issue.code == "impact_build_missing")
        && symbol_index_ready
    {
        actions.push(format!(
            "run `hyperctl impact rebuild --repo-id {} --snapshot-id {}` to materialize a fresh impact build",
            repo_id, snapshot_id
        ));
    }
    if issues.iter().any(|issue| {
        matches!(
            issue.code,
            "impact_build_corrupt"
                | "impact_store_corrupt"
                | "impact_build_config_mismatch"
                | "impact_build_schema_mismatch"
                | "impact_build_graph_inconsistent"
        )
    }) {
        actions.push(format!(
            "run `hyperctl impact rebuild --repo-id {} --snapshot-id {}` to replace the stored impact build",
            repo_id, snapshot_id
        ));
    }
    if issues.iter().any(|issue| {
        matches!(
            issue.code,
            "impact_store_corrupt" | "impact_store_unavailable"
        )
    }) {
        actions.push(
            "if rebuild cannot open the store, stop the daemon and run `hyperctl reset-runtime`"
                .to_string(),
        );
    }

    Ok(LocalImpactReport {
        daemon_reachable,
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        repo_last_snapshot_id: context.repo.last_snapshot_id.clone(),
        symbol_index_ready,
        symbol_index_refresh_mode,
        store,
        build,
        actions,
        issues,
    })
}

fn inspect_build_against_graph(
    stored_build: &StoredImpactBuild,
    graph: &hyperindex_symbols::SymbolGraph,
    issues: &mut Vec<LocalImpactIssue>,
) {
    let graph_files = graph
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let contribution_paths = stored_build
        .state
        .file_contributions
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if contribution_paths != graph_files {
        issues.push(LocalImpactIssue {
            code: "stale_manifest_or_index",
            message: format!(
                "stored impact contributions cover {} files but the current symbol graph covers {} files",
                contribution_paths.len(),
                graph_files.len()
            ),
        });
    }

    let mut inconsistent = false;
    for (symbol_id, path) in &stored_build.state.plan.symbol_to_file {
        match graph.symbols.get(symbol_id) {
            Some(symbol) if symbol.path == *path => {}
            _ => {
                inconsistent = true;
                break;
            }
        }
    }
    if !inconsistent {
        for (path, symbol_ids) in &stored_build.state.plan.file_to_symbols {
            if !graph_files.contains(path)
                || symbol_ids
                    .iter()
                    .any(|symbol_id| !graph.symbols.contains_key(symbol_id))
            {
                inconsistent = true;
                break;
            }
        }
    }
    if inconsistent {
        issues.push(LocalImpactIssue {
            code: "impact_build_graph_inconsistent",
            message:
                "stored impact enrichment references symbol/file edges that no longer match the current symbol graph"
                    .to_string(),
        });
    }
}

fn stored_build(
    snapshot: &ComposedSnapshot,
    store: &ImpactStore,
    refresh: &hyperindex_impact::ImpactRefreshResult,
) -> StoredImpactBuild {
    StoredImpactBuild {
        repo_id: snapshot.repo_id.clone(),
        snapshot_id: snapshot.snapshot_id.clone(),
        impact_config_digest: refresh.config_digest.clone(),
        schema_version: store.schema_version,
        symbol_build_id: None,
        created_at: epoch_ms_string(),
        refresh_mode: match refresh.stats.mode {
            ImpactRefreshMode::FullRebuild => "full_rebuild".to_string(),
            ImpactRefreshMode::Incremental => "incremental".to_string(),
        },
        refresh_stats: refresh.stats.clone(),
        fallback_reason: refresh
            .fallback_reason
            .as_ref()
            .map(|reason| reason.as_str().to_string()),
        state: refresh.state.clone(),
    }
}

fn epoch_ms_string() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("epoch-ms:{now}")
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

fn daemon_status(config_path: Option<&Path>) -> HyperindexResult<()> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::DaemonStatus(
        hyperindex_protocol::status::DaemonStatusParams::default(),
    ))? {
        SuccessPayload::DaemonStatus(_) => Ok(()),
        other => Err(HyperindexError::Message(format!(
            "unexpected daemon status response: {other:?}"
        ))),
    }
}

fn render_local_impact_report(title: &str, report: &LocalImpactReport) -> String {
    let mut lines = vec![
        format!("{title} {}", report.snapshot_id),
        format!("daemon_reachable: {}", report.daemon_reachable),
        format!(
            "repo_last_snapshot_id: {}",
            report.repo_last_snapshot_id.as_deref().unwrap_or("-")
        ),
        format!("symbol_index_ready: {}", report.symbol_index_ready),
        format!(
            "symbol_refresh_mode: {}",
            report.symbol_index_refresh_mode.as_deref().unwrap_or("-")
        ),
    ];
    if let Some(store) = &report.store {
        lines.push(format!("store_schema_version: {}", store.schema_version));
        lines.push(format!("store_build_count: {}", store.build_count));
        lines.push(format!(
            "store_quick_check: {}",
            store.quick_check.join("; ")
        ));
    } else {
        lines.push("store_schema_version: -".to_string());
    }
    lines.push(format!("materialized: {}", report.build.materialized));
    lines.push(format!(
        "refresh_mode: {}",
        report.build.refresh_mode.as_deref().unwrap_or("-")
    ));
    lines.push(format!(
        "fallback_reason: {}",
        report.build.fallback_reason.as_deref().unwrap_or("-")
    ));
    lines.push(format!(
        "file_contributions: {}",
        report.build.file_contribution_count
    ));
    lines.push(format!(
        "impacted_files: {}",
        report.build.impacted_file_count
    ));
    lines.push(format!("packages: {}", report.build.package_count));
    lines.push(format!(
        "reverse_reference_symbols: {}",
        report.build.reverse_reference_symbol_count
    ));
    lines.push(format!(
        "reverse_dependent_files: {}",
        report.build.reverse_dependent_file_count
    ));
    lines.push(format!(
        "test_associations: {}",
        report.build.test_association_count
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

fn render_json<T: serde::Serialize>(response: &T) -> HyperindexResult<String> {
    serde_json::to_string_pretty(response)
        .map_err(|error| HyperindexError::Message(format!("failed to render json: {error}")))
}

fn build_analyze_params(
    repo_id: &str,
    snapshot_id: &str,
    target_kind: &str,
    value: &str,
    change_hint: &str,
    limit: u32,
    include_transitive: bool,
    include_reason_paths: bool,
    max_transitive_depth: Option<u32>,
    max_nodes_visited: Option<u32>,
    max_edges_traversed: Option<u32>,
    max_candidates_considered: Option<u32>,
) -> HyperindexResult<ImpactAnalyzeParams> {
    Ok(ImpactAnalyzeParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        target: parse_target(target_kind, value)?,
        change_hint: parse_change_hint(change_hint)?,
        limit,
        include_transitive,
        include_reason_paths,
        max_transitive_depth,
        max_nodes_visited,
        max_edges_traversed,
        max_candidates_considered,
    })
}

fn build_explain_params(
    repo_id: &str,
    snapshot_id: &str,
    target_kind: &str,
    value: &str,
    change_hint: &str,
    impacted_kind: &str,
    impacted_value: &str,
    impacted_path: Option<&str>,
    impacted_symbol_id: Option<&str>,
    max_reason_paths: u32,
) -> HyperindexResult<ImpactExplainParams> {
    Ok(ImpactExplainParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        target: parse_target(target_kind, value)?,
        change_hint: parse_change_hint(change_hint)?,
        impacted: parse_impacted_entity(
            impacted_kind,
            impacted_value,
            impacted_path,
            impacted_symbol_id,
        )?,
        max_reason_paths,
    })
}

fn render_status(response: &ImpactStatusResponse, json_output: bool) -> HyperindexResult<String> {
    if json_output {
        return Ok(serde_json::to_string_pretty(response).unwrap());
    }

    Ok([
        "phase5 impact status".to_string(),
        format!("repo_id: {}", response.repo_id),
        format!("snapshot_id: {}", response.snapshot_id),
        format!("state: {:?}", response.state).to_lowercase(),
        format!("can_analyze: {}", response.capabilities.analyze),
        format!("can_explain: {}", response.capabilities.explain),
        format!(
            "materialized_store: {}",
            response.capabilities.materialized_store
        ),
        format!(
            "build_id: {}",
            response
                .manifest
                .as_ref()
                .map(|manifest| manifest.build_id.0.as_str())
                .unwrap_or("-")
        ),
        format!(
            "store_path: {}",
            response
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.storage.as_ref())
                .map(|storage| storage.path.as_str())
                .unwrap_or("-")
        ),
        format!(
            "refresh_mode: {}",
            response
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.refresh_mode.as_deref())
                .unwrap_or("-")
        ),
        format!("diagnostic_count: {}", response.diagnostics.len()),
    ]
    .join("\n"))
}

fn render_analyze(response: &ImpactAnalyzeResponse, json_output: bool) -> HyperindexResult<String> {
    if json_output {
        return Ok(serde_json::to_string_pretty(response).unwrap());
    }

    Ok([
        "phase5 impact analyze".to_string(),
        format!("repo_id: {}", response.repo_id),
        format!("snapshot_id: {}", response.snapshot_id),
        format!("target_kind: {:?}", response.target.target_kind()).to_lowercase(),
        format!("target: {}", response.target.selector_value()),
        format!(
            "hit_count: {}",
            response
                .groups
                .iter()
                .map(|group| group.hit_count)
                .sum::<u32>()
        ),
        format!("direct_count: {}", response.summary.direct_count),
        format!("transitive_count: {}", response.summary.transitive_count),
        format!("nodes_visited: {}", response.stats.nodes_visited),
        format!("edges_traversed: {}", response.stats.edges_traversed),
        format!("depth_reached: {}", response.stats.depth_reached),
        format!(
            "candidates_considered: {}",
            response.stats.candidates_considered
        ),
        format!("elapsed_ms: {}", response.stats.elapsed_ms),
        format!(
            "refresh_mode: {}",
            response
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.refresh_mode.as_deref())
                .unwrap_or("-")
        ),
        format!(
            "refresh_files_touched: {}",
            response
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.refresh_stats.as_ref())
                .map(|stats| stats.files_touched.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "refresh_entities_recomputed: {}",
            response
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.refresh_stats.as_ref())
                .map(|stats| stats.entities_recomputed.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "refresh_edges_refreshed: {}",
            response
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.refresh_stats.as_ref())
                .map(|stats| stats.edges_refreshed.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!("diagnostic_count: {}", response.diagnostics.len()),
    ]
    .join("\n"))
}

fn render_explain(response: &ImpactExplainResponse, json_output: bool) -> HyperindexResult<String> {
    if json_output {
        return Ok(serde_json::to_string_pretty(response).unwrap());
    }

    let mut lines = vec![
        "phase5 impact explain".to_string(),
        format!("repo_id: {}", response.repo_id),
        format!("snapshot_id: {}", response.snapshot_id),
        format!("target_kind: {:?}", response.target.target_kind()).to_lowercase(),
        format!("target: {}", response.target.selector_value()),
        format!("impacted_kind: {:?}", response.impacted.entity_kind()).to_lowercase(),
        format!("impacted: {}", describe_entity(&response.impacted)),
        format!("certainty: {:?}", response.certainty).to_lowercase(),
        format!("direct: {}", response.direct),
        format!("reason_path_count: {}", response.reason_paths.len()),
        format!("diagnostic_count: {}", response.diagnostics.len()),
    ];
    for (index, path) in response.reason_paths.iter().enumerate() {
        lines.push(format!("path_{}: {}", index + 1, path.summary));
    }
    Ok(lines.join("\n"))
}

fn parse_target(raw_kind: &str, raw_value: &str) -> HyperindexResult<ImpactTargetRef> {
    match parse_target_kind(raw_kind)? {
        ImpactTargetKind::Symbol => Ok(ImpactTargetRef::Symbol {
            value: raw_value.to_string(),
            symbol_id: None,
            path: None,
        }),
        ImpactTargetKind::File => Ok(ImpactTargetRef::File {
            path: raw_value.to_string(),
        }),
    }
}

fn parse_impacted_entity(
    raw_kind: &str,
    raw_value: &str,
    impacted_path: Option<&str>,
    impacted_symbol_id: Option<&str>,
) -> HyperindexResult<ImpactEntityRef> {
    match raw_kind {
        "symbol" => {
            let (path, display_name) =
                parse_selector(raw_value, impacted_path).ok_or_else(|| {
                    HyperindexError::Message(
                    "symbol impact explain requires --impacted-value PATH#NAME or --impacted-path"
                        .to_string(),
                )
                })?;
            let symbol_id = impacted_symbol_id.ok_or_else(|| {
                HyperindexError::Message(
                    "symbol impact explain requires --impacted-symbol-id".to_string(),
                )
            })?;
            Ok(ImpactEntityRef::Symbol {
                symbol_id: hyperindex_protocol::symbols::SymbolId(symbol_id.to_string()),
                path,
                display_name,
            })
        }
        "file" => Ok(ImpactEntityRef::File {
            path: raw_value.to_string(),
        }),
        "package" => Ok(ImpactEntityRef::Package {
            package_name: raw_value.to_string(),
            package_root: impacted_path
                .ok_or_else(|| {
                    HyperindexError::Message(
                        "package impact explain requires --impacted-path with the package root"
                            .to_string(),
                    )
                })?
                .to_string(),
        }),
        "test" => {
            let (path, display_name) =
                parse_selector(raw_value, impacted_path).ok_or_else(|| {
                    HyperindexError::Message(
                    "test impact explain requires --impacted-value PATH#NAME or --impacted-path"
                        .to_string(),
                )
                })?;
            Ok(ImpactEntityRef::Test {
                path,
                display_name,
                symbol_id: impacted_symbol_id
                    .map(|value| hyperindex_protocol::symbols::SymbolId(value.to_string())),
            })
        }
        _ => Err(HyperindexError::Message(format!(
            "unsupported impacted kind {raw_kind}; expected symbol, file, package, or test"
        ))),
    }
}

fn parse_selector(raw_value: &str, explicit_path: Option<&str>) -> Option<(String, String)> {
    if let Some((path, display_name)) = raw_value.split_once('#') {
        return Some((path.to_string(), display_name.to_string()));
    }
    explicit_path.map(|path| (path.to_string(), raw_value.to_string()))
}

fn parse_target_kind(raw: &str) -> HyperindexResult<ImpactTargetKind> {
    match raw {
        "symbol" => Ok(ImpactTargetKind::Symbol),
        "file" => Ok(ImpactTargetKind::File),
        _ => Err(HyperindexError::Message(format!(
            "unsupported impact target kind {raw}; expected symbol or file in the current phase5 slice"
        ))),
    }
}

fn parse_change_hint(raw: &str) -> HyperindexResult<ImpactChangeScenario> {
    match raw {
        "modify_behavior" => Ok(ImpactChangeScenario::ModifyBehavior),
        "signature_change" => Ok(ImpactChangeScenario::SignatureChange),
        "rename" => Ok(ImpactChangeScenario::Rename),
        "delete" => Ok(ImpactChangeScenario::Delete),
        _ => Err(HyperindexError::Message(format!(
            "unsupported impact change hint {raw}; expected modify_behavior, signature_change, rename, or delete"
        ))),
    }
}

fn describe_entity(entity: &ImpactEntityRef) -> String {
    match entity {
        ImpactEntityRef::Symbol {
            symbol_id,
            path,
            display_name,
        } => format!("{path}#{display_name} ({})", symbol_id.0),
        ImpactEntityRef::File { path } => path.clone(),
        ImpactEntityRef::Package {
            package_name,
            package_root,
        } => format!("{package_name} ({package_root})"),
        ImpactEntityRef::Test {
            path,
            display_name,
            symbol_id,
        } => format!(
            "{path}#{display_name} ({})",
            symbol_id
                .as_ref()
                .map(|value| value.0.as_str())
                .unwrap_or("-")
        ),
    }
}

fn unexpected_response(command: &str, other: SuccessPayload) -> HyperindexError {
    HyperindexError::Message(format!("unexpected {command} response: {other:?}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use hyperindex_protocol::config::{RuntimeConfig, TransportKind};
    use hyperindex_protocol::impact::{
        ImpactAnalysisState, ImpactAnalyzeResponse, ImpactCertaintyCounts, ImpactChangeScenario,
        ImpactDiagnostic, ImpactDiagnosticSeverity, ImpactEntityRef, ImpactExplainResponse,
        ImpactStatusResponse, ImpactSummary, ImpactTargetRef, ImpactTraversalStats,
    };
    use hyperindex_protocol::repo::ReposAddResponse;
    use hyperindex_protocol::snapshot::{SnapshotCreateParams, SnapshotCreateResponse};
    use tempfile::tempdir;

    use super::{
        build_analyze_params, build_explain_params, render_analyze, render_explain, render_status,
    };
    use crate::commands::{buffers, impact, repo, snapshot, symbol};

    #[test]
    fn build_analyze_params_normalizes_cli_strings() {
        let params = build_analyze_params(
            "repo-1",
            "snap-1",
            "symbol",
            "packages/auth/src/session/service.ts#invalidateSession",
            "modify_behavior",
            20,
            true,
            true,
            Some(4),
            Some(128),
            Some(256),
            Some(192),
        )
        .unwrap();

        assert_eq!(params.repo_id, "repo-1");
        assert_eq!(params.limit, 20);
        assert!(params.include_transitive);
        assert_eq!(params.max_transitive_depth, Some(4));
        assert_eq!(params.change_hint, ImpactChangeScenario::ModifyBehavior);
        assert_eq!(
            params.target,
            ImpactTargetRef::Symbol {
                value: "packages/auth/src/session/service.ts#invalidateSession".to_string(),
                symbol_id: None,
                path: None,
            }
        );
    }

    #[test]
    fn build_explain_params_normalizes_symbol_inputs() {
        let params = build_explain_params(
            "repo-1",
            "snap-1",
            "symbol",
            "packages/auth/src/session/service.ts#invalidateSession",
            "modify_behavior",
            "symbol",
            "packages/api/src/routes/logout.ts#logout",
            None,
            Some("sym.logout"),
            3,
        )
        .unwrap();

        assert_eq!(
            params.impacted,
            ImpactEntityRef::Symbol {
                symbol_id: hyperindex_protocol::symbols::SymbolId("sym.logout".to_string()),
                path: "packages/api/src/routes/logout.ts".to_string(),
                display_name: "logout".to_string(),
            }
        );
    }

    #[test]
    fn render_status_json_roundtrips() {
        let output = render_status(
            &ImpactStatusResponse {
                repo_id: "repo-1".to_string(),
                snapshot_id: "snap-1".to_string(),
                state: ImpactAnalysisState::Ready,
                capabilities: hyperindex_protocol::impact::ImpactCapabilities {
                    status: true,
                    analyze: true,
                    explain: true,
                    materialized_store: true,
                },
                supported_targets: Vec::new(),
                supported_change_scenarios: Vec::new(),
                supported_result_kinds: Vec::new(),
                certainty_tiers: Vec::new(),
                manifest: None,
                diagnostics: Vec::new(),
            },
            true,
        )
        .unwrap();

        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["repo_id"], "repo-1");
    }

    #[test]
    fn render_analyze_json_roundtrips() {
        let output = render_analyze(
            &ImpactAnalyzeResponse {
                repo_id: "repo-1".to_string(),
                snapshot_id: "snap-1".to_string(),
                target: build_analyze_params(
                    "repo-1",
                    "snap-1",
                    "symbol",
                    "packages/auth/src/session/service.ts#invalidateSession",
                    "modify_behavior",
                    20,
                    true,
                    true,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
                .target,
                change_hint: ImpactChangeScenario::ModifyBehavior,
                summary: ImpactSummary {
                    direct_count: 0,
                    transitive_count: 0,
                    certainty_counts: ImpactCertaintyCounts {
                        certain: 0,
                        likely: 0,
                        possible: 0,
                    },
                },
                stats: ImpactTraversalStats {
                    nodes_visited: 3,
                    edges_traversed: 4,
                    depth_reached: 2,
                    candidates_considered: 5,
                    elapsed_ms: 7,
                    cutoffs_triggered: Vec::new(),
                },
                groups: Vec::new(),
                diagnostics: vec![ImpactDiagnostic {
                    severity: ImpactDiagnosticSeverity::Warning,
                    code: "impact_cutoff_nodes_visited".to_string(),
                    message: "cutoff".to_string(),
                }],
                manifest: None,
            },
            true,
        )
        .unwrap();

        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["repo_id"], "repo-1");
    }

    #[test]
    fn render_explain_json_roundtrips() {
        let output = render_explain(
            &ImpactExplainResponse {
                repo_id: "repo-1".to_string(),
                snapshot_id: "snap-1".to_string(),
                target: ImpactTargetRef::File {
                    path: "packages/auth/src/session/service.ts".to_string(),
                },
                impacted: ImpactEntityRef::File {
                    path: "packages/api/src/routes/logout.ts".to_string(),
                },
                certainty: hyperindex_protocol::impact::ImpactCertaintyTier::Certain,
                direct: true,
                reason_paths: Vec::new(),
                diagnostics: Vec::new(),
            },
            true,
        )
        .unwrap();

        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["snapshot_id"], "snap-1");
    }

    #[test]
    fn daemon_backed_cli_commands_refresh_impact_results() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);

        let added = repo::add(
            Some(&config_path),
            &repo_root,
            Some("Impact Repo".to_string()),
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

        let not_ready = impact::status(
            Some(&config_path),
            &added.repo.repo_id,
            &clean.snapshot.snapshot_id,
            true,
        )
        .unwrap();
        let not_ready: ImpactStatusResponse = serde_json::from_str(&not_ready).unwrap();
        assert_eq!(not_ready.state, ImpactAnalysisState::NotReady);

        symbol::build(
            Some(&config_path),
            &added.repo.repo_id,
            &clean.snapshot.snapshot_id,
            false,
            true,
        )
        .unwrap();

        let ready = impact::status(
            Some(&config_path),
            &added.repo.repo_id,
            &clean.snapshot.snapshot_id,
            true,
        )
        .unwrap();
        let ready: ImpactStatusResponse = serde_json::from_str(&ready).unwrap();
        assert_eq!(ready.state, ImpactAnalysisState::Ready);
        assert!(ready.capabilities.explain);

        let clean_analyze = impact::analyze(
            Some(&config_path),
            &added.repo.repo_id,
            &clean.snapshot.snapshot_id,
            "symbol",
            "packages/auth/src/session/service.ts#invalidateSession",
            "modify_behavior",
            20,
            true,
            true,
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();
        let clean_analyze: ImpactAnalyzeResponse = serde_json::from_str(&clean_analyze).unwrap();
        assert!(
            clean_analyze
                .groups
                .iter()
                .flat_map(|group| group.hits.iter())
                .any(|hit| match &hit.entity {
                    ImpactEntityRef::File { path } => path == "packages/api/src/routes/logout.ts",
                    _ => false,
                })
        );

        let explain = impact::explain(
            Some(&config_path),
            &added.repo.repo_id,
            &clean.snapshot.snapshot_id,
            "symbol",
            "packages/auth/src/session/service.ts#invalidateSession",
            "modify_behavior",
            "file",
            "packages/api/src/routes/logout.ts",
            None,
            None,
            2,
            true,
        )
        .unwrap();
        let explain: ImpactExplainResponse = serde_json::from_str(&explain).unwrap();
        assert_eq!(
            explain.impacted,
            ImpactEntityRef::File {
                path: "packages/api/src/routes/logout.ts".to_string(),
            }
        );
        assert!(!explain.reason_paths.is_empty());

        let buffer_path = tempdir.path().join("logout.overlay.ts");
        fs::write(
            &buffer_path,
            "export function logout(userId: string) {\n  return `local:${userId}`;\n}\n",
        )
        .unwrap();
        buffers::set_from_file(
            Some(&config_path),
            &added.repo.repo_id,
            "buffer-1",
            "packages/api/src/routes/logout.ts",
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

        symbol::build(
            Some(&config_path),
            &added.repo.repo_id,
            &buffered.snapshot.snapshot_id,
            false,
            true,
        )
        .unwrap();

        let refreshed = impact::analyze(
            Some(&config_path),
            &added.repo.repo_id,
            &buffered.snapshot.snapshot_id,
            "symbol",
            "packages/auth/src/session/service.ts#invalidateSession",
            "modify_behavior",
            20,
            true,
            true,
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();
        let refreshed: ImpactAnalyzeResponse = serde_json::from_str(&refreshed).unwrap();
        assert_eq!(
            refreshed
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.refresh_mode.as_deref()),
            Some("incremental")
        );
        assert!(
            refreshed
                .groups
                .iter()
                .flat_map(|group| group.hits.iter())
                .all(|hit| match &hit.entity {
                    ImpactEntityRef::File { path } => path != "packages/api/src/routes/logout.ts",
                    ImpactEntityRef::Symbol { path, .. } => {
                        path != "packages/api/src/routes/logout.ts"
                    }
                    _ => true,
                })
        );
    }

    #[test]
    fn local_impact_doctor_and_rebuild_cover_missing_and_recovered_builds() {
        let tempdir = tempdir().unwrap();
        let config_path = write_test_config(tempdir.path());
        let repo_root = tempdir.path().join("repo-local-impact");
        init_repo(&repo_root);

        let added = repo::add(
            Some(&config_path),
            &repo_root,
            Some("Impact Local Repo".to_string()),
            Vec::new(),
            Vec::new(),
            true,
        )
        .unwrap();
        let added: ReposAddResponse = serde_json::from_str(&added).unwrap();
        let snapshot = snapshot::create(
            Some(&config_path),
            &SnapshotCreateParams {
                repo_id: added.repo.repo_id.clone(),
                include_working_tree: true,
                buffer_ids: Vec::new(),
            },
            true,
        )
        .unwrap();
        let snapshot: SnapshotCreateResponse = serde_json::from_str(&snapshot).unwrap();

        symbol::build(
            Some(&config_path),
            &added.repo.repo_id,
            &snapshot.snapshot.snapshot_id,
            false,
            true,
        )
        .unwrap();

        let doctor_before = impact::doctor(
            Some(&config_path),
            &added.repo.repo_id,
            &snapshot.snapshot.snapshot_id,
            true,
        )
        .unwrap();
        let doctor_before: serde_json::Value = serde_json::from_str(&doctor_before).unwrap();
        assert_eq!(doctor_before["build"]["materialized"], false);
        assert!(
            doctor_before["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue["code"] == "impact_build_missing")
        );

        let rebuilt = impact::rebuild(
            Some(&config_path),
            &added.repo.repo_id,
            &snapshot.snapshot.snapshot_id,
            true,
        )
        .unwrap();
        let rebuilt: serde_json::Value = serde_json::from_str(&rebuilt).unwrap();
        assert_eq!(rebuilt["refresh_mode"], "full_rebuild");

        let stats = impact::stats(
            Some(&config_path),
            &added.repo.repo_id,
            &snapshot.snapshot.snapshot_id,
            true,
        )
        .unwrap();
        let stats: serde_json::Value = serde_json::from_str(&stats).unwrap();
        assert_eq!(stats["build"]["materialized"], true);
        assert!(stats["build"]["file_contribution_count"].as_u64().unwrap() > 0);

        let doctor_after = impact::doctor(
            Some(&config_path),
            &added.repo.repo_id,
            &snapshot.snapshot.snapshot_id,
            true,
        )
        .unwrap();
        let doctor_after: serde_json::Value = serde_json::from_str(&doctor_after).unwrap();
        assert_eq!(doctor_after["issues"], serde_json::Value::Array(Vec::new()));
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
        config.impact.store_dir = runtime_root.join("data/impact");
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        config_path
    }

    fn init_repo(repo_root: &Path) {
        fs::create_dir_all(repo_root.join("packages/auth/src/session")).unwrap();
        fs::create_dir_all(repo_root.join("packages/api/src/routes")).unwrap();
        fs::create_dir_all(repo_root.join("packages/web/src/auth")).unwrap();
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["checkout", "-b", "trunk"]);
        fs::write(
            repo_root.join("packages/auth/src/session/service.ts"),
            "export function invalidateSession(userId: string) {\n  return `invalidated:${userId}`;\n}\n",
        )
        .unwrap();
        fs::write(
            repo_root.join("packages/auth/src/session/service.test.ts"),
            "import { invalidateSession } from \"./service\";\n\ntest(\"invalidates\", () => {\n  expect(invalidateSession(\"u-1\")).toContain(\"invalidated\");\n});\n",
        )
        .unwrap();
        fs::write(
            repo_root.join("packages/api/src/routes/logout.ts"),
            "import { invalidateSession } from \"../../../auth/src/session/service\";\n\nexport function logout(userId: string) {\n  return invalidateSession(userId);\n}\n",
        )
        .unwrap();
        fs::write(
            repo_root.join("packages/web/src/auth/logout-client.ts"),
            "import { logout } from \"../../../api/src/routes/logout\";\n\nexport function triggerLogout(userId: string) {\n  return logout(userId);\n}\n",
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
