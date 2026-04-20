use std::fs;
use std::path::Path;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::buffers::{BufferClearParams, BufferListParams, BufferSetParams};

use crate::client::DaemonClient;

pub fn set_from_file(
    config_path: Option<&Path>,
    repo_id: &str,
    buffer_id: &str,
    path: &str,
    from_file: &Path,
    version: u64,
    language: Option<String>,
    json_output: bool,
) -> HyperindexResult<String> {
    let contents = fs::read_to_string(from_file).map_err(|error| {
        HyperindexError::Message(format!("failed to read {}: {error}", from_file.display()))
    })?;
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::BuffersSet(BufferSetParams {
        repo_id: repo_id.to_string(),
        buffer_id: buffer_id.to_string(),
        path: path.to_string(),
        version,
        language,
        contents,
    }))? {
        SuccessPayload::BuffersSet(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected buffers set response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else {
        Ok(format!(
            "buffer_id: {}\npath: {}\nversion: {}\ncontent_bytes: {}",
            response.buffer.buffer_id,
            response.buffer.path,
            response.buffer.version,
            response.buffer.content_bytes
        ))
    }
}

pub fn clear(
    config_path: Option<&Path>,
    repo_id: &str,
    buffer_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::BuffersClear(BufferClearParams {
        repo_id: repo_id.to_string(),
        buffer_id: buffer_id.to_string(),
    }))? {
        SuccessPayload::BuffersClear(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected buffers clear response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else {
        Ok(format!(
            "Cleared buffer {} for repo {}: {}",
            response.buffer_id, response.repo_id, response.cleared
        ))
    }
}

pub fn list(
    config_path: Option<&Path>,
    repo_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    let response = match client.send(RequestBody::BuffersList(BufferListParams {
        repo_id: repo_id.to_string(),
    }))? {
        SuccessPayload::BuffersList(response) => response,
        other => {
            return Err(HyperindexError::Message(format!(
                "unexpected buffers list response: {other:?}"
            )));
        }
    };

    if json_output {
        Ok(serde_json::to_string_pretty(&response).unwrap())
    } else if response.buffers.is_empty() {
        Ok(format!("No buffers registered for repo {}.", repo_id))
    } else {
        Ok(response
            .buffers
            .iter()
            .map(|buffer| {
                format!(
                    "{} | {} | v{} | {} bytes",
                    buffer.buffer_id, buffer.path, buffer.version, buffer.content_bytes
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
