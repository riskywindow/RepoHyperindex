use hyperindex_core::{HyperindexError, HyperindexResult};

use crate::db::RepoStore;

impl RepoStore {
    pub fn enqueue_job_stub(&self, _repo_id: Option<&str>, _kind: &str) -> HyperindexResult<()> {
        Err(HyperindexError::NotImplemented("repo_store.enqueue_job"))
    }
}
