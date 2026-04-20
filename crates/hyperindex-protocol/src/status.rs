use serde::{Deserialize, Serialize};

use crate::config::TransportKind;
use crate::impact::ImpactMaterializationMode;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EmptyParams {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Ok,
    Degraded,
    Starting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub status: HealthState,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionResponse {
    pub daemon_version: String,
    pub protocol_version: String,
    pub config_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DaemonStatusParams {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonLifecycleState {
    Starting,
    Running,
    Stopping,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportSummary {
    pub kind: TransportKind,
    pub socket_path: Option<String>,
    pub connected_clients: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchedulerSnapshot {
    pub mode: String,
    pub queue_depth: usize,
    pub active_jobs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ParseRuntimeStatus {
    pub enabled: bool,
    pub artifact_dir: String,
    pub repo_count: usize,
    pub build_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SymbolIndexRuntimeStatus {
    pub enabled: bool,
    pub store_dir: String,
    pub repo_count: usize,
    pub indexed_snapshot_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactRuntimeStatus {
    pub enabled: bool,
    pub store_dir: String,
    pub materialization_mode: ImpactMaterializationMode,
    pub repo_count: usize,
    pub materialized_snapshot_count: usize,
    pub ready_build_count: usize,
    pub stale_build_count: usize,
}

impl Default for ImpactRuntimeStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            store_dir: String::new(),
            materialization_mode: ImpactMaterializationMode::LiveOnly,
            repo_count: 0,
            materialized_snapshot_count: 0,
            ready_build_count: 0,
            stale_build_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticRuntimeStatus {
    pub enabled: bool,
    pub store_dir: String,
    pub embedding_model_id: String,
    pub chunk_schema_version: u32,
    pub repo_count: usize,
    pub materialized_snapshot_count: usize,
    pub ready_build_count: usize,
    pub stale_build_count: usize,
}

impl Default for SemanticRuntimeStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            store_dir: String::new(),
            embedding_model_id: String::new(),
            chunk_schema_version: 0,
            repo_count: 0,
            materialized_snapshot_count: 0,
            ready_build_count: 0,
            stale_build_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub protocol_version: String,
    pub config_version: u32,
    pub runtime_root: String,
    pub state_dir: String,
    pub socket_path: String,
    pub daemon_state: DaemonLifecycleState,
    pub pid: Option<u32>,
    pub transport: TransportSummary,
    pub repo_count: usize,
    pub manifest_count: usize,
    pub scheduler: SchedulerSnapshot,
    #[serde(default)]
    pub parser: Option<ParseRuntimeStatus>,
    #[serde(default)]
    pub symbol_index: Option<SymbolIndexRuntimeStatus>,
    #[serde(default)]
    pub impact: Option<ImpactRuntimeStatus>,
    #[serde(default)]
    pub semantic: Option<SemanticRuntimeStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShutdownParams {
    pub graceful: bool,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShutdownResponse {
    pub accepted: bool,
    pub message: Option<String>,
}
