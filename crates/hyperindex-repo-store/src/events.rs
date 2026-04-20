use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::watch::WatchBatch;

use crate::db::RepoStore;

impl RepoStore {
    pub fn append_watch_batch_stub(&self, _batch: &WatchBatch) -> HyperindexResult<()> {
        Err(HyperindexError::NotImplemented(
            "repo_store.append_watch_batch",
        ))
    }
}
