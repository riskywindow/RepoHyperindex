use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Validation,
    Config,
    Transport,
    Storage,
    Repo,
    Watch,
    Snapshot,
    Buffer,
    Scheduler,
    Parse,
    Symbol,
    Impact,
    Semantic,
    Daemon,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedProtocolVersion,
    ConfigNotFound,
    ConfigInvalid,
    RepoAlreadyExists,
    RepoNotFound,
    RepoStateUnavailable,
    WatchNotConfigured,
    WatchNotRunning,
    SnapshotNotFound,
    SnapshotConflict,
    SnapshotMismatch,
    BufferNotFound,
    SchedulerBusy,
    ShutdownInProgress,
    Timeout,
    UnsupportedLanguage,
    InvalidPosition,
    ParseBuildNotFound,
    ParseArtifactNotFound,
    IndexNotReady,
    SymbolIndexNotFound,
    SymbolNotFound,
    ResolutionNotFound,
    ImpactNotReady,
    ImpactTargetNotFound,
    ImpactTargetUnsupported,
    ImpactResultNotFound,
    SemanticNotReady,
    SemanticBuildNotFound,
    SemanticChunkNotFound,
    SemanticFilterUnsupported,
    NotImplemented,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSubjectKind {
    Repo,
    Snapshot,
    Buffer,
    ParseBuild,
    ParseArtifact,
    SymbolIndex,
    Symbol,
    ImpactBuild,
    ImpactTarget,
    ImpactResult,
    SemanticBuild,
    SemanticChunk,
    SemanticQuery,
    Location,
    Config,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorSubject {
    pub kind: ErrorSubjectKind,
    pub id: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationIssue {
    pub field: String,
    pub issue: String,
    pub expected: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ErrorPayload {
    pub subject: Option<ErrorSubject>,
    pub validation: Vec<ValidationIssue>,
    pub retry_after_ms: Option<u64>,
    pub supported_protocol_versions: Vec<String>,
    pub context: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolError {
    pub category: ErrorCategory,
    pub code: ErrorCode,
    pub message: String,
    pub retriable: bool,
    pub payload: Option<ErrorPayload>,
}

impl ProtocolError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            category: ErrorCategory::Validation,
            code: ErrorCode::InvalidRequest,
            message: message.into(),
            retriable: false,
            payload: None,
        }
    }

    pub fn invalid_field(
        field: impl Into<String>,
        issue: impl Into<String>,
        expected: Option<String>,
    ) -> Self {
        Self {
            category: ErrorCategory::Validation,
            code: ErrorCode::InvalidRequest,
            message: "request validation failed".to_string(),
            retriable: false,
            payload: Some(ErrorPayload {
                validation: vec![ValidationIssue {
                    field: field.into(),
                    issue: issue.into(),
                    expected,
                }],
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn unsupported_protocol_version(found: impl Into<String>) -> Self {
        let found = found.into();
        let mut context = BTreeMap::new();
        context.insert("protocol_version".to_string(), found.clone());
        Self {
            category: ErrorCategory::Validation,
            code: ErrorCode::UnsupportedProtocolVersion,
            message: format!("unsupported protocol version: {found}"),
            retriable: false,
            payload: Some(ErrorPayload {
                supported_protocol_versions: vec!["repo-hyperindex.local/v1".to_string()],
                context,
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn config_invalid(message: impl Into<String>) -> Self {
        Self {
            category: ErrorCategory::Config,
            code: ErrorCode::ConfigInvalid,
            message: message.into(),
            retriable: false,
            payload: None,
        }
    }

    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            category: ErrorCategory::Transport,
            code: ErrorCode::Internal,
            message: message.into(),
            retriable: true,
            payload: None,
        }
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self {
            category: ErrorCategory::Storage,
            code: ErrorCode::Internal,
            message: message.into(),
            retriable: true,
            payload: None,
        }
    }

    pub fn repo_already_exists(message: impl Into<String>) -> Self {
        Self {
            category: ErrorCategory::Repo,
            code: ErrorCode::RepoAlreadyExists,
            message: message.into(),
            retriable: false,
            payload: None,
        }
    }

    pub fn not_implemented(area: &'static str) -> Self {
        let mut context = BTreeMap::new();
        context.insert("area".to_string(), area.to_string());
        Self {
            category: ErrorCategory::Daemon,
            code: ErrorCode::NotImplemented,
            message: format!("{area} is not implemented in the current scaffold"),
            retriable: false,
            payload: Some(ErrorPayload {
                context,
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn repo_not_found(repo_id: impl Into<String>) -> Self {
        let repo_id = repo_id.into();
        Self {
            category: ErrorCategory::Repo,
            code: ErrorCode::RepoNotFound,
            message: format!("repo {repo_id} was not found"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::Repo,
                    id: Some(repo_id),
                    path: None,
                }),
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn repo_state_unavailable(
        repo_id: impl Into<String>,
        repo_root: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let repo_id = repo_id.into();
        let repo_root = repo_root.into();
        let mut context = BTreeMap::new();
        context.insert("repo_root".to_string(), repo_root.clone());
        Self {
            category: ErrorCategory::Repo,
            code: ErrorCode::RepoStateUnavailable,
            message: message.into(),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::Repo,
                    id: Some(repo_id),
                    path: Some(repo_root),
                }),
                context,
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn snapshot_not_found(snapshot_id: impl Into<String>) -> Self {
        let snapshot_id = snapshot_id.into();
        Self {
            category: ErrorCategory::Snapshot,
            code: ErrorCode::SnapshotNotFound,
            message: format!("snapshot {snapshot_id} was not found"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::Snapshot,
                    id: Some(snapshot_id),
                    path: None,
                }),
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn buffer_not_found(repo_id: impl Into<String>, buffer_id: impl Into<String>) -> Self {
        let repo_id = repo_id.into();
        let buffer_id = buffer_id.into();
        let mut context = BTreeMap::new();
        context.insert("repo_id".to_string(), repo_id);
        Self {
            category: ErrorCategory::Buffer,
            code: ErrorCode::BufferNotFound,
            message: format!("buffer {buffer_id} was not found"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::Buffer,
                    id: Some(buffer_id),
                    path: None,
                }),
                context,
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn parse_artifact_not_found(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            category: ErrorCategory::Parse,
            code: ErrorCode::ParseArtifactNotFound,
            message: format!("parse artifact for {path} was not found"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::ParseArtifact,
                    id: None,
                    path: Some(path),
                }),
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn symbol_not_found(symbol_id: impl Into<String>) -> Self {
        let symbol_id = symbol_id.into();
        Self {
            category: ErrorCategory::Symbol,
            code: ErrorCode::SymbolNotFound,
            message: format!("symbol {symbol_id} was not found"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::Symbol,
                    id: Some(symbol_id),
                    path: None,
                }),
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn semantic_not_ready(repo_id: impl Into<String>, snapshot_id: impl Into<String>) -> Self {
        let repo_id = repo_id.into();
        let snapshot_id = snapshot_id.into();
        let mut context = BTreeMap::new();
        context.insert("repo_id".to_string(), repo_id.clone());
        Self {
            category: ErrorCategory::Semantic,
            code: ErrorCode::SemanticNotReady,
            message: format!("semantic retrieval is not ready for snapshot {snapshot_id}"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::Snapshot,
                    id: Some(snapshot_id),
                    path: None,
                }),
                context,
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn semantic_build_not_found(build_id: impl Into<String>) -> Self {
        let build_id = build_id.into();
        Self {
            category: ErrorCategory::Semantic,
            code: ErrorCode::SemanticBuildNotFound,
            message: format!("semantic build {build_id} was not found"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::SemanticBuild,
                    id: Some(build_id),
                    path: None,
                }),
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn semantic_chunk_not_found(chunk_id: impl Into<String>) -> Self {
        let chunk_id = chunk_id.into();
        Self {
            category: ErrorCategory::Semantic,
            code: ErrorCode::SemanticChunkNotFound,
            message: format!("semantic chunk {chunk_id} was not found"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::SemanticChunk,
                    id: Some(chunk_id),
                    path: None,
                }),
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn semantic_filter_unsupported(field: impl Into<String>, issue: impl Into<String>) -> Self {
        let field = field.into();
        let issue = issue.into();
        Self {
            category: ErrorCategory::Semantic,
            code: ErrorCode::SemanticFilterUnsupported,
            message: "semantic query filter is not supported".to_string(),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::SemanticQuery,
                    id: None,
                    path: None,
                }),
                validation: vec![ValidationIssue {
                    field,
                    issue,
                    expected: None,
                }],
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn impact_not_ready(
        repo_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let repo_id = repo_id.into();
        let snapshot_id = snapshot_id.into();
        let mut context = BTreeMap::new();
        context.insert("repo_id".to_string(), repo_id.clone());
        context.insert("snapshot_id".to_string(), snapshot_id.clone());
        Self {
            category: ErrorCategory::Impact,
            code: ErrorCode::ImpactNotReady,
            message: message.into(),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::ImpactBuild,
                    id: None,
                    path: None,
                }),
                context,
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn impact_target_not_found(target_id: impl Into<String>) -> Self {
        let target_id = target_id.into();
        Self {
            category: ErrorCategory::Impact,
            code: ErrorCode::ImpactTargetNotFound,
            message: format!("impact target {target_id} was not found"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::ImpactTarget,
                    id: Some(target_id),
                    path: None,
                }),
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn impact_result_not_found(result_id: impl Into<String>) -> Self {
        let result_id = result_id.into();
        Self {
            category: ErrorCategory::Impact,
            code: ErrorCode::ImpactResultNotFound,
            message: format!("impact result {result_id} was not found"),
            retriable: false,
            payload: Some(ErrorPayload {
                subject: Some(ErrorSubject {
                    kind: ErrorSubjectKind::ImpactResult,
                    id: Some(result_id),
                    path: None,
                }),
                ..ErrorPayload::default()
            }),
        }
    }

    pub fn shutdown_in_progress() -> Self {
        Self {
            category: ErrorCategory::Daemon,
            code: ErrorCode::ShutdownInProgress,
            message: "daemon shutdown is already in progress".to_string(),
            retriable: false,
            payload: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            category: ErrorCategory::Internal,
            code: ErrorCode::Internal,
            message: message.into(),
            retriable: false,
            payload: None,
        }
    }
}
