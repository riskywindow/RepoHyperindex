use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedEventKind {
    Created,
    Modified,
    Removed,
    Renamed,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedEvent {
    pub sequence: u64,
    pub kind: NormalizedEventKind,
    pub path: String,
    pub previous_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchBatch {
    pub repo_id: String,
    pub events: Vec<NormalizedEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatcherStatus {
    pub repo_id: String,
    pub attached: bool,
    pub backend: String,
    pub last_sequence: Option<u64>,
    pub dropped_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WatchStatusParams {
    pub repo_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchStatusResponse {
    pub watchers: Vec<WatcherStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchEventsParams {
    pub repo_id: String,
    pub cursor: Option<u64>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchEventsResponse {
    pub repo_id: String,
    pub next_cursor: Option<u64>,
    pub events: Vec<NormalizedEvent>,
}
