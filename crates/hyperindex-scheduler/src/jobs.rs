use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobKind {
    RepoRefresh,
    WatchIngest,
    SnapshotCapture,
}

impl JobKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RepoRefresh => "repo_refresh",
            Self::WatchIngest => "watch_ingest",
            Self::SnapshotCapture => "snapshot_capture",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub job_id: String,
    pub repo_id: Option<String>,
    pub kind: JobKind,
    pub state: JobState,
}

impl JobRecord {
    pub fn label(&self) -> String {
        match &self.repo_id {
            Some(repo_id) => format!(
                "{}:{}:{}:{}",
                self.job_id,
                self.kind.as_str(),
                self.state.as_str(),
                repo_id
            ),
            None => format!(
                "{}:{}:{}",
                self.job_id,
                self.kind.as_str(),
                self.state.as_str()
            ),
        }
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
