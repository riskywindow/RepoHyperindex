use std::collections::BTreeSet;

use hyperindex_core::{HyperindexError, HyperindexResult, normalize_repo_relative_path};
use hyperindex_protocol::buffers::{
    BufferClearParams, BufferClearResponse, BufferContents, BufferListParams, BufferSetParams,
    BufferSetResponse, BufferState,
};
use rusqlite::{OptionalExtension, params};

use crate::db::RepoStore;

impl RepoStore {
    pub fn set_buffer(&self, params: &BufferSetParams) -> HyperindexResult<BufferSetResponse> {
        let normalized_path = normalize_repo_relative_path(&params.path, "buffer")?;
        let content_sha256 = sha256_hex(params.contents.as_bytes());
        let content_bytes = params.contents.len();
        self.connection()
            .execute(
                "
                INSERT INTO buffers (
                  repo_id,
                  buffer_id,
                  path,
                  version,
                  language,
                  content_sha256,
                  content_bytes,
                  contents,
                  updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, CURRENT_TIMESTAMP)
                ON CONFLICT(repo_id, buffer_id) DO UPDATE SET
                  path = excluded.path,
                  version = excluded.version,
                  language = excluded.language,
                  content_sha256 = excluded.content_sha256,
                  content_bytes = excluded.content_bytes,
                  contents = excluded.contents,
                  updated_at = CURRENT_TIMESTAMP
                ",
                params![
                    params.repo_id,
                    params.buffer_id,
                    normalized_path,
                    params.version as i64,
                    params.language,
                    content_sha256,
                    content_bytes as i64,
                    params.contents,
                ],
            )
            .map_err(|error| HyperindexError::Message(format!("buffer upsert failed: {error}")))?;

        Ok(BufferSetResponse {
            buffer: BufferState {
                buffer_id: params.buffer_id.clone(),
                repo_id: params.repo_id.clone(),
                path: normalize_repo_relative_path(&params.path, "buffer")?,
                version: params.version,
                language: params.language.clone(),
                content_sha256,
                content_bytes,
            },
        })
    }

    pub fn clear_buffer(
        &self,
        params: &BufferClearParams,
    ) -> HyperindexResult<BufferClearResponse> {
        let cleared = self
            .connection()
            .execute(
                "DELETE FROM buffers WHERE repo_id = ?1 AND buffer_id = ?2",
                params![params.repo_id, params.buffer_id],
            )
            .map_err(|error| HyperindexError::Message(format!("buffer delete failed: {error}")))?
            > 0;

        Ok(BufferClearResponse {
            repo_id: params.repo_id.clone(),
            buffer_id: params.buffer_id.clone(),
            cleared,
        })
    }

    pub fn list_buffers(&self, params: &BufferListParams) -> HyperindexResult<Vec<BufferState>> {
        let mut statement = self
            .connection()
            .prepare(
                "
                SELECT repo_id, buffer_id, path, version, language, content_sha256, content_bytes
                FROM buffers
                WHERE repo_id = ?1
                ORDER BY path, buffer_id
                ",
            )
            .map_err(|error| HyperindexError::Message(format!("prepare failed: {error}")))?;
        let rows = statement
            .query_map([params.repo_id.as_str()], |row| {
                Ok(BufferState {
                    repo_id: row.get(0)?,
                    buffer_id: row.get(1)?,
                    path: row.get(2)?,
                    version: row.get::<_, i64>(3)? as u64,
                    language: row.get(4)?,
                    content_sha256: row.get(5)?,
                    content_bytes: row.get::<_, i64>(6)? as usize,
                })
            })
            .map_err(|error| HyperindexError::Message(format!("query failed: {error}")))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| HyperindexError::Message(format!("row decode failed: {error}")))
    }

    pub fn load_buffers(
        &self,
        repo_id: &str,
        buffer_ids: &[String],
    ) -> HyperindexResult<Vec<BufferContents>> {
        let mut seen_buffer_ids = BTreeSet::new();
        let mut loaded = Vec::new();
        for buffer_id in buffer_ids {
            if !seen_buffer_ids.insert(buffer_id.clone()) {
                return Err(HyperindexError::Message(format!(
                    "buffer {buffer_id} was requested more than once for repo {repo_id}; remove duplicate --buffer-id values"
                )));
            }
            let buffer = self.load_buffer(repo_id, buffer_id)?.ok_or_else(|| {
                HyperindexError::Message(format!(
                    "buffer {buffer_id} was not found for repo {repo_id}; run `hyperctl buffers list --repo-id {repo_id}` or clear stale buffer references"
                ))
            })?;
            normalize_repo_relative_path(&buffer.state.path, "buffer overlay")?;
            loaded.push(buffer);
        }
        loaded.sort_by(|left, right| {
            left.state
                .path
                .cmp(&right.state.path)
                .then_with(|| left.state.buffer_id.cmp(&right.state.buffer_id))
        });
        Ok(loaded)
    }

    fn load_buffer(
        &self,
        repo_id: &str,
        buffer_id: &str,
    ) -> HyperindexResult<Option<BufferContents>> {
        let mut statement = self
            .connection()
            .prepare(
                "
                SELECT repo_id, buffer_id, path, version, language, content_sha256, content_bytes, contents
                FROM buffers
                WHERE repo_id = ?1 AND buffer_id = ?2
                ",
            )
            .map_err(|error| HyperindexError::Message(format!("prepare failed: {error}")))?;

        statement
            .query_row(params![repo_id, buffer_id], |row| {
                Ok(BufferContents {
                    state: BufferState {
                        repo_id: row.get(0)?,
                        buffer_id: row.get(1)?,
                        path: row.get(2)?,
                        version: row.get::<_, i64>(3)? as u64,
                        language: row.get(4)?,
                        content_sha256: row.get(5)?,
                        content_bytes: row.get::<_, i64>(6)? as usize,
                    },
                    contents: row.get(7)?,
                })
            })
            .optional()
            .map_err(|error| HyperindexError::Message(format!("query failed: {error}")))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::buffers::{BufferClearParams, BufferListParams, BufferSetParams};

    use crate::RepoStore;

    #[test]
    fn buffer_store_roundtrips_contents() {
        let store = RepoStore::open_in_memory().unwrap();
        let set = store
            .set_buffer(&BufferSetParams {
                repo_id: "repo-1".to_string(),
                buffer_id: "buffer-1".to_string(),
                path: "src/app.ts".to_string(),
                version: 7,
                language: Some("typescript".to_string()),
                contents: "export const answer = 42;\n".to_string(),
            })
            .unwrap();
        assert_eq!(set.buffer.path, "src/app.ts");

        let listed = store
            .list_buffers(&BufferListParams {
                repo_id: "repo-1".to_string(),
            })
            .unwrap();
        assert_eq!(listed.len(), 1);

        let loaded = store
            .load_buffers("repo-1", &["buffer-1".to_string()])
            .unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].contents, "export const answer = 42;\n");

        let cleared = store
            .clear_buffer(&BufferClearParams {
                repo_id: "repo-1".to_string(),
                buffer_id: "buffer-1".to_string(),
            })
            .unwrap();
        assert!(cleared.cleared);
    }
}
