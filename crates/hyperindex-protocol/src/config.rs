use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::impact::ImpactMaterializationMode;
use crate::planner::{PlannerBudgetPolicy, PlannerMode, PlannerRouteBudget, PlannerRouteKind};
use crate::semantic::{
    SemanticChunkTextConfig, SemanticEmbeddingProviderConfig, SemanticEmbeddingProviderKind,
    SemanticRerankMode,
};
use crate::symbols::LanguageId;
use crate::{CONFIG_VERSION, PROTOCOL_VERSION};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppDirectories {
    pub runtime_root: PathBuf,
    pub state_dir: PathBuf,
    pub data_dir: PathBuf,
    pub manifests_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub temp_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    Stdio,
    UnixSocket,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportSettings {
    pub kind: TransportKind,
    pub socket_path: PathBuf,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub max_frame_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepoRegistryBackend {
    Sqlite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoRegistrySettings {
    pub backend: RepoRegistryBackend,
    pub sqlite_path: PathBuf,
    pub manifests_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WatchBackend {
    Poll,
    Notify,
    Stub,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchConfig {
    pub backend: WatchBackend,
    pub poll_interval_ms: u64,
    pub debounce_ms: u64,
    pub batch_max_events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchedulerDefaults {
    pub max_concurrent_repos: usize,
    pub coalesce_window_ms: u64,
    pub idle_flush_ms: u64,
    pub job_lease_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogVerbosity {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoggingSettings {
    pub verbosity: LogVerbosity,
    pub format: LogFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IgnoreSettings {
    pub global_patterns: Vec<String>,
    pub repo_patterns: Vec<String>,
    pub exclude_dot_git: bool,
    pub exclude_node_modules: bool,
    pub exclude_target: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseCacheMode {
    MemoryOnly,
    Persistent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguagePackConfig {
    pub pack_id: String,
    pub enabled: bool,
    pub languages: Vec<LanguageId>,
    pub include_globs: Vec<String>,
    pub grammar_version: Option<String>,
}

impl Default for LanguagePackConfig {
    fn default() -> Self {
        Self {
            pack_id: "ts_js_core".to_string(),
            enabled: true,
            languages: vec![
                LanguageId::Typescript,
                LanguageId::Tsx,
                LanguageId::Javascript,
                LanguageId::Jsx,
                LanguageId::Mts,
                LanguageId::Cts,
            ],
            include_globs: vec![
                "**/*.ts".to_string(),
                "**/*.tsx".to_string(),
                "**/*.js".to_string(),
                "**/*.jsx".to_string(),
                "**/*.mts".to_string(),
                "**/*.cts".to_string(),
            ],
            grammar_version: Some("tree-sitter-typescript@phase4-contract".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParserConfig {
    pub enabled: bool,
    pub max_file_bytes: usize,
    pub diagnostics_max_per_file: usize,
    pub cache_mode: ParseCacheMode,
    pub artifact_dir: PathBuf,
    pub language_packs: Vec<LanguagePackConfig>,
}

impl Default for ParserConfig {
    fn default() -> Self {
        let runtime_root = PathBuf::from(".hyperindex");
        Self {
            enabled: true,
            max_file_bytes: 2_097_152,
            diagnostics_max_per_file: 32,
            cache_mode: ParseCacheMode::Persistent,
            artifact_dir: runtime_root.join("data").join("parse-artifacts"),
            language_packs: vec![LanguagePackConfig::default()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolIndexConfig {
    pub enabled: bool,
    pub store_dir: PathBuf,
    pub default_search_limit: usize,
    pub max_search_limit: usize,
    pub persist_occurrences: bool,
}

impl Default for SymbolIndexConfig {
    fn default() -> Self {
        let runtime_root = PathBuf::from(".hyperindex");
        Self {
            enabled: true,
            store_dir: runtime_root.join("data").join("symbols"),
            default_search_limit: 25,
            max_search_limit: 200,
            persist_occurrences: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactConfig {
    pub enabled: bool,
    pub store_dir: PathBuf,
    pub materialization_mode: ImpactMaterializationMode,
    pub default_limit: usize,
    pub max_limit: usize,
    pub default_include_transitive: bool,
    pub default_include_reason_paths: bool,
    pub max_reason_paths_per_hit: usize,
    pub max_transitive_depth: u32,
    pub include_possible_results: bool,
}

impl Default for ImpactConfig {
    fn default() -> Self {
        let runtime_root = PathBuf::from(".hyperindex");
        Self {
            enabled: true,
            store_dir: runtime_root.join("data").join("impact"),
            materialization_mode: ImpactMaterializationMode::PreferPersisted,
            default_limit: 20,
            max_limit: 200,
            default_include_transitive: true,
            default_include_reason_paths: true,
            max_reason_paths_per_hit: 4,
            max_transitive_depth: 8,
            include_possible_results: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticQueryConfig {
    pub default_search_limit: usize,
    pub max_search_limit: usize,
    pub default_rerank_mode: SemanticRerankMode,
    pub default_path_globs: Vec<String>,
}

impl Default for SemanticQueryConfig {
    fn default() -> Self {
        Self {
            default_search_limit: 20,
            max_search_limit: 200,
            default_rerank_mode: SemanticRerankMode::Hybrid,
            default_path_globs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SemanticEmbeddingRuntimeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticConfig {
    pub enabled: bool,
    pub store_dir: PathBuf,
    pub chunk_schema_version: u32,
    pub embedding_provider: SemanticEmbeddingProviderConfig,
    #[serde(default)]
    pub embedding_runtime: SemanticEmbeddingRuntimeConfig,
    pub chunk_text: SemanticChunkTextConfig,
    pub query: SemanticQueryConfig,
}

impl Default for SemanticConfig {
    fn default() -> Self {
        let runtime_root = PathBuf::from(".hyperindex");
        Self {
            enabled: true,
            store_dir: runtime_root.join("data").join("semantic"),
            chunk_schema_version: 1,
            embedding_provider: SemanticEmbeddingProviderConfig {
                provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
                model_id: "phase6-deterministic-fixture".to_string(),
                model_digest: "phase6-deterministic-fixture-v1".to_string(),
                vector_dimensions: 384,
                normalized: true,
                max_input_bytes: 8_192,
                max_batch_size: 32,
            },
            embedding_runtime: SemanticEmbeddingRuntimeConfig::default(),
            chunk_text: SemanticChunkTextConfig {
                serializer_id: "phase6-structured-text".to_string(),
                format_version: 1,
                includes_path_header: true,
                includes_symbol_context: true,
                normalized_newlines: true,
            },
            query: SemanticQueryConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerRoutesConfig {
    pub exact_enabled: bool,
    pub symbol_enabled: bool,
    pub semantic_enabled: bool,
    pub impact_enabled: bool,
}

impl Default for PlannerRoutesConfig {
    fn default() -> Self {
        Self {
            exact_enabled: true,
            symbol_enabled: true,
            semantic_enabled: true,
            impact_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerConfig {
    pub enabled: bool,
    pub default_mode: PlannerMode,
    pub default_limit: usize,
    pub max_limit: usize,
    pub default_include_trace: bool,
    #[serde(default)]
    pub routes: PlannerRoutesConfig,
    pub budgets: PlannerBudgetPolicy,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_mode: PlannerMode::Auto,
            default_limit: 10,
            max_limit: 50,
            default_include_trace: false,
            routes: PlannerRoutesConfig::default(),
            budgets: PlannerBudgetPolicy {
                total_timeout_ms: 1_500,
                max_groups: 10,
                route_budgets: vec![
                    PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Exact,
                        max_candidates: 25,
                        max_groups: 10,
                        timeout_ms: 150,
                    },
                    PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Symbol,
                        max_candidates: 20,
                        max_groups: 10,
                        timeout_ms: 250,
                    },
                    PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Semantic,
                        max_candidates: 16,
                        max_groups: 8,
                        timeout_ms: 400,
                    },
                    PlannerRouteBudget {
                        route_kind: PlannerRouteKind::Impact,
                        max_candidates: 8,
                        max_groups: 4,
                        timeout_ms: 600,
                    },
                ],
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub version: u32,
    pub protocol_version: String,
    pub directories: AppDirectories,
    pub transport: TransportSettings,
    pub repo_registry: RepoRegistrySettings,
    pub watch: WatchConfig,
    pub scheduler: SchedulerDefaults,
    pub logging: LoggingSettings,
    pub ignores: IgnoreSettings,
    #[serde(default)]
    pub parser: ParserConfig,
    #[serde(default)]
    pub symbol_index: SymbolIndexConfig,
    #[serde(default)]
    pub impact: ImpactConfig,
    #[serde(default)]
    pub semantic: SemanticConfig,
    #[serde(default)]
    pub planner: PlannerConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let runtime_root = PathBuf::from(".hyperindex");
        let state_dir = runtime_root.join("state");
        let data_dir = runtime_root.join("data");
        let manifests_dir = data_dir.join("manifests");
        let logs_dir = runtime_root.join("logs");
        let temp_dir = runtime_root.join("tmp");
        Self {
            version: CONFIG_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            directories: AppDirectories {
                runtime_root: runtime_root.clone(),
                state_dir: state_dir.clone(),
                data_dir: data_dir.clone(),
                manifests_dir: manifests_dir.clone(),
                logs_dir,
                temp_dir,
            },
            transport: TransportSettings {
                kind: TransportKind::UnixSocket,
                socket_path: runtime_root.join("hyperd.sock"),
                connect_timeout_ms: 2_000,
                request_timeout_ms: 30_000,
                max_frame_bytes: 1_048_576,
            },
            repo_registry: RepoRegistrySettings {
                backend: RepoRegistryBackend::Sqlite,
                sqlite_path: state_dir.join("runtime.sqlite3"),
                manifests_dir,
            },
            watch: WatchConfig {
                backend: WatchBackend::Poll,
                poll_interval_ms: 250,
                debounce_ms: 100,
                batch_max_events: 256,
            },
            scheduler: SchedulerDefaults {
                max_concurrent_repos: 1,
                coalesce_window_ms: 150,
                idle_flush_ms: 500,
                job_lease_ms: 30_000,
            },
            logging: LoggingSettings {
                verbosity: LogVerbosity::Info,
                format: LogFormat::Text,
            },
            ignores: IgnoreSettings {
                global_patterns: vec![
                    ".git/**".to_string(),
                    "node_modules/**".to_string(),
                    "target/**".to_string(),
                    ".next/**".to_string(),
                    "dist/**".to_string(),
                ],
                repo_patterns: Vec::new(),
                exclude_dot_git: true,
                exclude_node_modules: true,
                exclude_target: true,
            },
            parser: ParserConfig::default(),
            symbol_index: SymbolIndexConfig::default(),
            impact: ImpactConfig::default(),
            semantic: SemanticConfig::default(),
            planner: PlannerConfig::default(),
        }
    }
}
