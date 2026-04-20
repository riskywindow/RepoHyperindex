pub mod incremental;
pub mod migrations;
pub mod symbol_store;

use std::io;

use thiserror::Error;

pub use incremental::{
    IncrementalIndexOptions, IncrementalRefreshMode, IncrementalRefreshResult,
    IncrementalRefreshStats, IncrementalRefreshTrigger, IncrementalSymbolIndexer,
    RebuildFallbackReason,
};
pub use symbol_store::{
    ExtractedSnapshot, IndexedSnapshotState, RecordedSnapshot, SnapshotStorageStats, SymbolStore,
    SymbolStoreStatus,
};

#[derive(Debug, Error)]
pub enum SymbolStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("parser error: {0}")]
    Parser(#[from] hyperindex_parser::ParserError),
}

pub type SymbolStoreResult<T> = Result<T, SymbolStoreError>;
