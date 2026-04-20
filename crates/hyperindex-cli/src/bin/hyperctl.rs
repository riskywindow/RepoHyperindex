use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use hyperindex_cli::commands;
use hyperindex_core::init_tracing;
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    name = "hyperctl",
    version = "0.1.0",
    about = "Repo Hyperindex CLI scaffold."
)]
struct Cli {
    #[arg(long)]
    config_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Buffers(BuffersCommand),
    Cleanup {
        #[arg(long)]
        json: bool,
    },
    Config(ConfigCommand),
    Daemon(DaemonCommand),
    Impact(ImpactCommand),
    Parse(ParseCommand),
    Semantic(SemanticCommand),
    Doctor {
        #[arg(long)]
        json: bool,
    },
    ResetRuntime {
        #[arg(long)]
        json: bool,
    },
    Status {
        #[arg(long)]
        json: bool,
    },
    Repo(RepoCommand),
    Repos(ReposCommand),
    Snapshot(SnapshotCommand),
    Symbol(SymbolCommand),
    Watch(WatchCommand),
}

#[derive(Debug, Args)]
struct BuffersCommand {
    #[command(subcommand)]
    command: BuffersSubcommand,
}

#[derive(Debug, Subcommand)]
enum BuffersSubcommand {
    Set {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        buffer_id: String,

        #[arg(long)]
        path: String,

        #[arg(long)]
        from_file: PathBuf,

        #[arg(long, default_value_t = 1)]
        version: u64,

        #[arg(long)]
        language: Option<String>,

        #[arg(long)]
        json: bool,
    },
    Clear {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        buffer_id: String,

        #[arg(long)]
        json: bool,
    },
    List {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct ConfigCommand {
    #[command(subcommand)]
    command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
enum ConfigSubcommand {
    Init {
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Args)]
struct DaemonCommand {
    #[command(subcommand)]
    command: DaemonSubcommand,
}

#[derive(Debug, Subcommand)]
enum DaemonSubcommand {
    Start {
        #[arg(long)]
        json: bool,
    },
    Status {
        #[arg(long)]
        json: bool,
    },
    Stop {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct ImpactCommand {
    #[command(subcommand)]
    command: ImpactSubcommand,
}

#[derive(Debug, Subcommand)]
enum ImpactSubcommand {
    Rebuild {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    Status {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    Analyze {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        target_kind: String,

        #[arg(long)]
        value: String,

        #[arg(long)]
        change_hint: String,

        #[arg(long, default_value_t = 20)]
        limit: u32,

        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        include_transitive: bool,

        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        include_reason_paths: bool,

        #[arg(long)]
        max_transitive_depth: Option<u32>,

        #[arg(long)]
        max_nodes_visited: Option<u32>,

        #[arg(long)]
        max_edges_traversed: Option<u32>,

        #[arg(long)]
        max_candidates_considered: Option<u32>,

        #[arg(long)]
        json: bool,
    },
    Explain {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        target_kind: String,

        #[arg(long)]
        value: String,

        #[arg(long)]
        change_hint: String,

        #[arg(long)]
        impacted_kind: String,

        #[arg(long)]
        impacted_value: String,

        #[arg(long)]
        impacted_path: Option<String>,

        #[arg(long)]
        impacted_symbol_id: Option<String>,

        #[arg(long, default_value_t = 4)]
        max_reason_paths: u32,

        #[arg(long)]
        json: bool,
    },
    Stats {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    Doctor {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct ParseCommand {
    #[command(subcommand)]
    command: ParseSubcommand,
}

#[derive(Debug, Subcommand)]
enum ParseSubcommand {
    Build {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        force: bool,

        #[arg(long)]
        json: bool,
    },
    Status {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    InspectFile {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        path: String,

        #[arg(long)]
        include_facts: bool,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct SemanticCommand {
    #[command(subcommand)]
    command: SemanticSubcommand,
}

#[derive(Debug, Subcommand)]
enum SemanticSubcommand {
    Status {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    #[command(alias = "search")]
    Query {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        query: String,

        #[arg(long, default_value_t = 20)]
        limit: u32,

        #[arg(long = "path-glob")]
        path_globs: Vec<String>,

        #[arg(long, default_value = "hybrid")]
        rerank_mode: String,

        #[arg(long)]
        json: bool,
    },
    Build {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        force: bool,

        #[arg(long)]
        json: bool,
    },
    Rebuild {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    InspectChunk {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        chunk_id: String,

        #[arg(long)]
        json: bool,
    },
    InspectIndex {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    Stats {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct ReposCommand {
    #[command(subcommand)]
    command: ReposSubcommand,
}

#[derive(Debug, Args)]
struct RepoCommand {
    #[command(subcommand)]
    command: RepoSubcommand,
}

#[derive(Debug, Subcommand)]
enum RepoSubcommand {
    Status {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        json: bool,
    },
    Head {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ReposSubcommand {
    Add {
        #[arg(long)]
        path: PathBuf,

        #[arg(long)]
        name: Option<String>,

        #[arg(long = "note")]
        notes: Vec<String>,

        #[arg(long = "ignore")]
        ignore_patterns: Vec<String>,

        #[arg(long)]
        json: bool,
    },
    List {
        #[arg(long)]
        json: bool,
    },
    Show {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        json: bool,
    },
    Remove {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        purge_state: bool,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct SnapshotCommand {
    #[command(subcommand)]
    command: SnapshotSubcommand,
}

#[derive(Debug, Subcommand)]
enum SnapshotSubcommand {
    Create {
        #[arg(long)]
        repo_id: String,

        #[arg(long, default_value_t = true)]
        include_working_tree: bool,

        #[arg(long = "buffer-id")]
        buffer_ids: Vec<String>,

        #[arg(long)]
        json: bool,
    },
    Show {
        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    Diff {
        #[arg(long)]
        left_snapshot_id: String,

        #[arg(long)]
        right_snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    ReadFile {
        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        path: String,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct SymbolCommand {
    #[command(subcommand)]
    command: SymbolSubcommand,
}

#[derive(Debug, Subcommand)]
enum SymbolSubcommand {
    Build {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        force: bool,

        #[arg(long)]
        json: bool,
    },
    Rebuild {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    Status {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    Stats {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    Doctor {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        json: bool,
    },
    #[command(alias = "lookup")]
    Search {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        query: String,

        #[arg(long, default_value_t = 10)]
        limit: usize,

        #[arg(long)]
        json: bool,
    },
    #[command(alias = "explain")]
    Show {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        symbol_id: String,

        #[arg(long)]
        json: bool,
    },
    #[command(alias = "definitions")]
    Defs {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        symbol_id: String,

        #[arg(long)]
        json: bool,
    },
    #[command(alias = "references")]
    Refs {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        symbol_id: String,

        #[arg(long)]
        json: bool,
    },
    Resolve {
        #[arg(long)]
        repo_id: String,

        #[arg(long)]
        snapshot_id: String,

        #[arg(long)]
        path: String,

        #[arg(long)]
        line: Option<u32>,

        #[arg(long)]
        column: Option<u32>,

        #[arg(long)]
        offset: Option<u32>,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct WatchCommand {
    #[command(subcommand)]
    command: WatchSubcommand,
}

#[derive(Debug, Subcommand)]
enum WatchSubcommand {
    Once {
        #[arg(long)]
        repo_id: String,

        #[arg(long, default_value_t = 500)]
        timeout_ms: u64,

        #[arg(long)]
        json: bool,
    },
}

impl Cli {
    fn wants_json(&self) -> bool {
        match &self.command {
            Commands::Buffers(command) => match &command.command {
                BuffersSubcommand::Set { json, .. } => *json,
                BuffersSubcommand::Clear { json, .. } => *json,
                BuffersSubcommand::List { json, .. } => *json,
            },
            Commands::Cleanup { json } => *json,
            Commands::Config(_) => true,
            Commands::Daemon(command) => match &command.command {
                DaemonSubcommand::Start { json } => *json,
                DaemonSubcommand::Status { json } => *json,
                DaemonSubcommand::Stop { json } => *json,
            },
            Commands::Impact(command) => match &command.command {
                ImpactSubcommand::Rebuild { json, .. } => *json,
                ImpactSubcommand::Status { json, .. } => *json,
                ImpactSubcommand::Analyze { json, .. } => *json,
                ImpactSubcommand::Explain { json, .. } => *json,
                ImpactSubcommand::Stats { json, .. } => *json,
                ImpactSubcommand::Doctor { json, .. } => *json,
            },
            Commands::Parse(command) => match &command.command {
                ParseSubcommand::Build { json, .. } => *json,
                ParseSubcommand::Status { json, .. } => *json,
                ParseSubcommand::InspectFile { json, .. } => *json,
            },
            Commands::Semantic(command) => match &command.command {
                SemanticSubcommand::Status { json, .. } => *json,
                SemanticSubcommand::Query { json, .. } => *json,
                SemanticSubcommand::Build { json, .. } => *json,
                SemanticSubcommand::Rebuild { json, .. } => *json,
                SemanticSubcommand::InspectChunk { json, .. } => *json,
                SemanticSubcommand::InspectIndex { json, .. } => *json,
                SemanticSubcommand::Stats { json, .. } => *json,
            },
            Commands::Doctor { json } => *json,
            Commands::ResetRuntime { json } => *json,
            Commands::Status { json } => *json,
            Commands::Repo(command) => match &command.command {
                RepoSubcommand::Status { json, .. } => *json,
                RepoSubcommand::Head { json, .. } => *json,
            },
            Commands::Repos(command) => match &command.command {
                ReposSubcommand::Add { json, .. } => *json,
                ReposSubcommand::List { json } => *json,
                ReposSubcommand::Show { json, .. } => *json,
                ReposSubcommand::Remove { json, .. } => *json,
            },
            Commands::Snapshot(command) => match &command.command {
                SnapshotSubcommand::Create { json, .. } => *json,
                SnapshotSubcommand::Show { json, .. } => *json,
                SnapshotSubcommand::Diff { json, .. } => *json,
                SnapshotSubcommand::ReadFile { json, .. } => *json,
            },
            Commands::Symbol(command) => match &command.command {
                SymbolSubcommand::Build { json, .. } => *json,
                SymbolSubcommand::Rebuild { json, .. } => *json,
                SymbolSubcommand::Status { json, .. } => *json,
                SymbolSubcommand::Stats { json, .. } => *json,
                SymbolSubcommand::Doctor { json, .. } => *json,
                SymbolSubcommand::Search { json, .. } => *json,
                SymbolSubcommand::Show { json, .. } => *json,
                SymbolSubcommand::Defs { json, .. } => *json,
                SymbolSubcommand::Refs { json, .. } => *json,
                SymbolSubcommand::Resolve { json, .. } => *json,
            },
            Commands::Watch(command) => match &command.command {
                WatchSubcommand::Once { json, .. } => *json,
            },
        }
    }
}

fn main() -> Result<()> {
    init_tracing("hyperctl");
    let cli = Cli::parse();
    let wants_json = cli.wants_json();

    match dispatch(cli) {
        Ok(output) => {
            println!("{output}");
            Ok(())
        }
        Err(error) if wants_json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "error": {
                        "message": error.to_string(),
                    }
                }))?
            );
            Err(anyhow!(error.to_string()))
        }
        Err(error) => Err(error),
    }
}

fn dispatch(cli: Cli) -> Result<String> {
    let config_path = cli.config_path;
    let output = match cli.command {
        Commands::Buffers(command) => match command.command {
            BuffersSubcommand::Set {
                repo_id,
                buffer_id,
                path,
                from_file,
                version,
                language,
                json,
            } => commands::buffers::set_from_file(
                config_path.as_deref(),
                &repo_id,
                &buffer_id,
                &path,
                &from_file,
                version,
                language,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            BuffersSubcommand::Clear {
                repo_id,
                buffer_id,
                json,
            } => commands::buffers::clear(config_path.as_deref(), &repo_id, &buffer_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            BuffersSubcommand::List { repo_id, json } => {
                commands::buffers::list(config_path.as_deref(), &repo_id, json)
                    .map_err(|error| anyhow!(error.to_string()))?
            }
        },
        Commands::Config(command) => match command.command {
            ConfigSubcommand::Init { force } => {
                commands::config::init(config_path.as_deref(), force)?
            }
        },
        Commands::Cleanup { json } => commands::maintenance::cleanup(config_path.as_deref(), json)
            .map_err(|error| anyhow!(error.to_string()))?,
        Commands::Daemon(command) => match command.command {
            DaemonSubcommand::Start { json } => {
                commands::daemon::start(config_path.as_deref(), json)
                    .map_err(|error| anyhow!(error.to_string()))?
            }
            DaemonSubcommand::Status { json } => {
                commands::daemon::status(config_path.as_deref(), json)
                    .map_err(|error| anyhow!(error.to_string()))?
            }
            DaemonSubcommand::Stop { json } => commands::daemon::stop(config_path.as_deref(), json)
                .map_err(|error| anyhow!(error.to_string()))?,
        },
        Commands::Impact(command) => match command.command {
            ImpactSubcommand::Rebuild {
                repo_id,
                snapshot_id,
                json,
            } => commands::impact::rebuild(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            ImpactSubcommand::Status {
                repo_id,
                snapshot_id,
                json,
            } => commands::impact::status(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            ImpactSubcommand::Analyze {
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
                json,
            } => commands::impact::analyze(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                &target_kind,
                &value,
                &change_hint,
                limit,
                include_transitive,
                include_reason_paths,
                max_transitive_depth,
                max_nodes_visited,
                max_edges_traversed,
                max_candidates_considered,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            ImpactSubcommand::Explain {
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
                json,
            } => commands::impact::explain(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                &target_kind,
                &value,
                &change_hint,
                &impacted_kind,
                &impacted_value,
                impacted_path.as_deref(),
                impacted_symbol_id.as_deref(),
                max_reason_paths,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            ImpactSubcommand::Stats {
                repo_id,
                snapshot_id,
                json,
            } => commands::impact::stats(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            ImpactSubcommand::Doctor {
                repo_id,
                snapshot_id,
                json,
            } => commands::impact::doctor(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
        },
        Commands::Parse(command) => match command.command {
            ParseSubcommand::Build {
                repo_id,
                snapshot_id,
                force,
                json,
            } => {
                commands::parse::build(config_path.as_deref(), &repo_id, &snapshot_id, force, json)
                    .map_err(|error| anyhow!(error.to_string()))?
            }
            ParseSubcommand::Status {
                repo_id,
                snapshot_id,
                json,
            } => commands::parse::status(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            ParseSubcommand::InspectFile {
                repo_id,
                snapshot_id,
                path,
                include_facts,
                json,
            } => commands::parse::inspect_file(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                &path,
                include_facts,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
        },
        Commands::Semantic(command) => match command.command {
            SemanticSubcommand::Status {
                repo_id,
                snapshot_id,
                json,
            } => commands::semantic::status(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            SemanticSubcommand::Query {
                repo_id,
                snapshot_id,
                query,
                limit,
                path_globs,
                rerank_mode,
                json,
            } => commands::semantic::query(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                &query,
                limit,
                path_globs,
                &rerank_mode,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SemanticSubcommand::Build {
                repo_id,
                snapshot_id,
                force,
                json,
            } => commands::semantic::build(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                force,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SemanticSubcommand::Rebuild {
                repo_id,
                snapshot_id,
                json,
            } => commands::semantic::build(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                true,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SemanticSubcommand::InspectChunk {
                repo_id,
                snapshot_id,
                chunk_id,
                json,
            } => commands::semantic::inspect_chunk(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                &chunk_id,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SemanticSubcommand::InspectIndex {
                repo_id,
                snapshot_id,
                json,
            } => commands::semantic::inspect_index(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SemanticSubcommand::Stats {
                repo_id,
                snapshot_id,
                json,
            } => commands::semantic::stats(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
        },
        Commands::Doctor { json } => commands::maintenance::doctor(config_path.as_deref(), json)
            .map_err(|error| anyhow!(error.to_string()))?,
        Commands::ResetRuntime { json } => {
            commands::maintenance::reset_runtime(config_path.as_deref(), json)
                .map_err(|error| anyhow!(error.to_string()))?
        }
        Commands::Status { json } => commands::status::render_status(config_path.as_deref(), json)
            .map_err(|error| anyhow!(error.to_string()))?,
        Commands::Repo(command) => match command.command {
            RepoSubcommand::Status { repo_id, json } => {
                commands::repo::status(config_path.as_deref(), &repo_id, json)
                    .map_err(|error| anyhow!(error.to_string()))?
            }
            RepoSubcommand::Head { repo_id, json } => {
                commands::repo::head(config_path.as_deref(), &repo_id, json)
                    .map_err(|error| anyhow!(error.to_string()))?
            }
        },
        Commands::Repos(command) => match command.command {
            ReposSubcommand::Add {
                path,
                name,
                notes,
                ignore_patterns,
                json,
            } => commands::repo::add(
                config_path.as_deref(),
                &path,
                name,
                notes,
                ignore_patterns,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            ReposSubcommand::List { json } => commands::repo::list(config_path.as_deref(), json)
                .map_err(|error| anyhow!(error.to_string()))?,
            ReposSubcommand::Show { repo_id, json } => {
                commands::repo::show(config_path.as_deref(), &repo_id, json)
                    .map_err(|error| anyhow!(error.to_string()))?
            }
            ReposSubcommand::Remove {
                repo_id,
                purge_state,
                json,
            } => commands::repo::remove(config_path.as_deref(), &repo_id, purge_state, json)
                .map_err(|error| anyhow!(error.to_string()))?,
        },
        Commands::Snapshot(command) => match command.command {
            SnapshotSubcommand::Create {
                repo_id,
                include_working_tree,
                buffer_ids,
                json,
            } => commands::snapshot::create(
                config_path.as_deref(),
                &hyperindex_protocol::snapshot::SnapshotCreateParams {
                    repo_id,
                    include_working_tree,
                    buffer_ids,
                },
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SnapshotSubcommand::Show { snapshot_id, json } => {
                commands::snapshot::show(config_path.as_deref(), &snapshot_id, json)
                    .map_err(|error| anyhow!(error.to_string()))?
            }
            SnapshotSubcommand::Diff {
                left_snapshot_id,
                right_snapshot_id,
                json,
            } => commands::snapshot::diff(
                config_path.as_deref(),
                &left_snapshot_id,
                &right_snapshot_id,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SnapshotSubcommand::ReadFile {
                snapshot_id,
                path,
                json,
            } => commands::snapshot::read_file(config_path.as_deref(), &snapshot_id, &path, json)
                .map_err(|error| anyhow!(error.to_string()))?,
        },
        Commands::Symbol(command) => match command.command {
            SymbolSubcommand::Build {
                repo_id,
                snapshot_id,
                force,
                json,
            } => {
                commands::symbol::build(config_path.as_deref(), &repo_id, &snapshot_id, force, json)
                    .map_err(|error| anyhow!(error.to_string()))?
            }
            SymbolSubcommand::Rebuild {
                repo_id,
                snapshot_id,
                json,
            } => commands::symbol::rebuild(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            SymbolSubcommand::Status {
                repo_id,
                snapshot_id,
                json,
            } => commands::symbol::status(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            SymbolSubcommand::Stats {
                repo_id,
                snapshot_id,
                json,
            } => commands::symbol::stats(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            SymbolSubcommand::Doctor {
                repo_id,
                snapshot_id,
                json,
            } => commands::symbol::doctor(config_path.as_deref(), &repo_id, &snapshot_id, json)
                .map_err(|error| anyhow!(error.to_string()))?,
            SymbolSubcommand::Search {
                repo_id,
                snapshot_id,
                query,
                limit,
                json,
            } => commands::symbol::search(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                &query,
                limit,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SymbolSubcommand::Show {
                repo_id,
                snapshot_id,
                symbol_id,
                json,
            } => commands::symbol::show(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                &symbol_id,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SymbolSubcommand::Defs {
                repo_id,
                snapshot_id,
                symbol_id,
                json,
            } => commands::symbol::definitions(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                &symbol_id,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SymbolSubcommand::Refs {
                repo_id,
                snapshot_id,
                symbol_id,
                json,
            } => commands::symbol::references(
                config_path.as_deref(),
                &repo_id,
                &snapshot_id,
                &symbol_id,
                None,
                json,
            )
            .map_err(|error| anyhow!(error.to_string()))?,
            SymbolSubcommand::Resolve {
                repo_id,
                snapshot_id,
                path,
                line,
                column,
                offset,
                json,
            } => match (line, column, offset) {
                (Some(line), Some(column), None) => commands::symbol::resolve_line_column(
                    config_path.as_deref(),
                    &repo_id,
                    &snapshot_id,
                    &path,
                    line,
                    column,
                    json,
                )
                .map_err(|error| anyhow!(error.to_string()))?,
                (None, None, Some(offset)) => commands::symbol::resolve_offset(
                    config_path.as_deref(),
                    &repo_id,
                    &snapshot_id,
                    &path,
                    offset,
                    json,
                )
                .map_err(|error| anyhow!(error.to_string()))?,
                _ => {
                    return Err(anyhow!(
                        "symbol resolve requires either --line with --column, or --offset"
                    ));
                }
            },
        },
        Commands::Watch(command) => match command.command {
            WatchSubcommand::Once {
                repo_id,
                timeout_ms,
                json,
            } => commands::watch::once(config_path.as_deref(), &repo_id, timeout_ms, json)
                .map_err(|error| anyhow!(error.to_string()))?,
        },
    };
    Ok(output)
}
