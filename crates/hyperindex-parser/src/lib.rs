pub mod language_pack_ts_js;
pub mod line_index;
pub mod parse_cache;
pub mod parse_core;
pub mod parse_manager;
pub mod snapshot_catalog;

use thiserror::Error;

pub use language_pack_ts_js::{LanguagePack, TsJsLanguage, TsJsLanguagePack};
pub use line_index::LineIndex;
pub use parse_cache::{ParseCache, ParseCacheKey};
pub use parse_core::{
    AstNodeHandle, ParseArtifact, ParseArtifactInspection, ParseBatchPlan, ParseCandidate,
    ParseCore, ParseCoreSettings, ParsedSyntaxTree,
};
pub use parse_manager::{
    ParseBuildStats, ParseBuildStatus, ParseManager, ParseManagerOptions, ParseStoreFileRecord,
};
pub use snapshot_catalog::{
    ParseEligibilityRules, ParseSkipReason, ResolvedParseFile, SkippedParseFile,
    SnapshotFileCatalog,
};

#[derive(Debug, Error)]
pub enum ParserError {
    #[error("{0}")]
    Message(String),
}

pub type ParserResult<T> = Result<T, ParserError>;
