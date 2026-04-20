use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RepoIgnoreSettings {
    pub patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepoRenamedPath {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorkingTreeSummary {
    pub dirty_tracked_files: Vec<String>,
    pub untracked_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub renamed_files: Vec<RepoRenamedPath>,
    pub ignored_files: Vec<String>,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoRecord {
    pub repo_id: String,
    pub repo_root: String,
    pub display_name: String,
    pub created_at: String,
    pub updated_at: String,
    pub branch: Option<String>,
    pub head_commit: Option<String>,
    pub is_dirty: bool,
    pub last_snapshot_id: Option<String>,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
    pub ignore_settings: RepoIgnoreSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReposAddParams {
    pub repo_root: String,
    pub display_name: Option<String>,
    pub notes: Vec<String>,
    pub ignore_patterns: Vec<String>,
    pub watch_on_add: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReposAddResponse {
    pub repo: RepoRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReposListParams {
    pub include_removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoListResponse {
    pub protocol_version: String,
    pub repos: Vec<RepoRecord>,
}

pub type ReposListResponse = RepoListResponse;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReposRemoveParams {
    pub repo_id: String,
    pub purge_state: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReposRemoveResponse {
    pub repo_id: String,
    pub removed: bool,
    pub purged_state: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoShowParams {
    pub repo_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoShowResponse {
    pub repo: RepoRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoStatusParams {
    pub repo_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoStatusResponse {
    pub repo_id: String,
    pub repo_root: String,
    pub display_name: String,
    pub branch: Option<String>,
    pub head_commit: Option<String>,
    pub working_tree_digest: String,
    pub is_dirty: bool,
    pub watch_attached: bool,
    pub dirty_path_count: usize,
    pub dirty_tracked_files: Vec<String>,
    pub untracked_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub renamed_files: Vec<RepoRenamedPath>,
    pub ignored_files: Vec<String>,
    pub last_snapshot_id: Option<String>,
    pub active_job: Option<String>,
    pub last_error_code: Option<String>,
}
