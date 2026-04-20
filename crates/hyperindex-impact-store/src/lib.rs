pub mod impact_store;
pub mod migrations;

use std::path::{Path, PathBuf};

use thiserror::Error;

pub use impact_store::{ImpactBuildManifest, ImpactStore, ImpactStoreStatus, StoredImpactBuild};
pub use migrations::{IMPACT_STORE_SCHEMA_VERSION, ImpactStoreMigration, planned_migrations};

#[derive(Debug, Error)]
pub enum ImpactStoreError {
    #[error("impact store root {0} is not valid for the Phase 5 scaffold")]
    InvalidRoot(String),
    #[error("impact store io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("impact store sqlite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("impact store json failed: {0}")]
    Json(#[from] serde_json::Error),
}

pub type ImpactStoreResult<T> = Result<T, ImpactStoreError>;

pub fn default_store_path(runtime_root: &Path, repo_id: &str) -> PathBuf {
    runtime_root
        .join("data")
        .join("impact")
        .join(repo_id)
        .join("impact.sqlite3")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{IMPACT_STORE_SCHEMA_VERSION, default_store_path, planned_migrations};

    #[test]
    fn default_store_path_matches_phase5_layout() {
        let path = default_store_path(Path::new(".hyperindex"), "repo-123");
        assert_eq!(
            path,
            Path::new(".hyperindex/data/impact/repo-123/impact.sqlite3")
        );
    }

    #[test]
    fn planned_migrations_include_current_schema_version() {
        let migrations = planned_migrations();
        assert_eq!(
            migrations.last().unwrap().schema_version,
            IMPACT_STORE_SCHEMA_VERSION
        );
    }
}
