use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferState {
    pub buffer_id: String,
    pub repo_id: String,
    pub path: String,
    pub version: u64,
    pub language: Option<String>,
    pub content_sha256: String,
    pub content_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferContents {
    pub state: BufferState,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferSetParams {
    pub repo_id: String,
    pub buffer_id: String,
    pub path: String,
    pub version: u64,
    pub language: Option<String>,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferSetResponse {
    pub buffer: BufferState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferClearParams {
    pub repo_id: String,
    pub buffer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferClearResponse {
    pub repo_id: String,
    pub buffer_id: String,
    pub cleared: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferListParams {
    pub repo_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferListResponse {
    pub repo_id: String,
    pub buffers: Vec<BufferState>,
}
