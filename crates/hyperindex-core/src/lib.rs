pub mod errors;
pub mod paths;

use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;

pub use errors::{HyperindexError, HyperindexResult};
pub use paths::normalize_repo_relative_path;

static TRACING_INIT: OnceLock<()> = OnceLock::new();

pub const DEFAULT_LOG_FILTER: &str = "info";

pub fn init_tracing(component: &str) {
    TRACING_INIT.get_or_init(|| {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("{component}={DEFAULT_LOG_FILTER}")));
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .try_init();
    });
}
