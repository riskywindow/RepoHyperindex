use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BaseSnapshotKind {
    GitCommit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotFile {
    pub path: String,
    pub content_sha256: String,
    pub content_bytes: usize,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaseSnapshot {
    pub kind: BaseSnapshotKind,
    pub commit: String,
    pub digest: String,
    pub file_count: usize,
    pub files: Vec<SnapshotFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlayEntryKind {
    Upsert,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkingTreeEntry {
    pub path: String,
    pub kind: OverlayEntryKind,
    pub content_sha256: Option<String>,
    pub content_bytes: Option<usize>,
    pub contents: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkingTreeOverlay {
    pub digest: String,
    pub entries: Vec<WorkingTreeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferOverlay {
    pub buffer_id: String,
    pub path: String,
    pub version: u64,
    pub content_sha256: String,
    pub content_bytes: usize,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposedSnapshot {
    pub version: u32,
    pub protocol_version: String,
    pub snapshot_id: String,
    pub repo_id: String,
    pub repo_root: String,
    pub base: BaseSnapshot,
    pub working_tree: WorkingTreeOverlay,
    pub buffers: Vec<BufferOverlay>,
}

pub type SnapshotManifest = ComposedSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotSummary {
    pub snapshot_id: String,
    pub repo_id: String,
    pub base_commit: String,
    pub working_tree_digest: String,
    pub has_working_tree: bool,
    pub buffer_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotCreateParams {
    pub repo_id: String,
    pub include_working_tree: bool,
    pub buffer_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotCreateResponse {
    pub snapshot: ComposedSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotShowParams {
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotShowResponse {
    pub snapshot: ComposedSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotListParams {
    pub repo_id: String,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotListResponse {
    pub repo_id: String,
    pub snapshots: Vec<SnapshotSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotDiffParams {
    pub left_snapshot_id: String,
    pub right_snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotDiffResponse {
    pub left_snapshot_id: String,
    pub right_snapshot_id: String,
    pub changed_paths: Vec<String>,
    pub added_paths: Vec<String>,
    pub deleted_paths: Vec<String>,
    pub buffer_only_changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotReadFileParams {
    pub snapshot_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotResolvedFileSourceKind {
    BufferOverlay,
    WorkingTreeOverlay,
    BaseSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotResolvedFileSource {
    pub kind: SnapshotResolvedFileSourceKind,
    pub buffer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotReadFileResponse {
    pub snapshot_id: String,
    pub path: String,
    pub resolved_from: SnapshotResolvedFileSource,
    pub contents: String,
}
