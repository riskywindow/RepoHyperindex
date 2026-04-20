use std::path::Path;

use hyperindex_config::write_default_config;
use hyperindex_core::HyperindexResult;
use serde_json::json;

pub fn init(config_path: Option<&Path>, force: bool) -> HyperindexResult<String> {
    let loaded = write_default_config(config_path, force)?;
    Ok(json!({
        "protocol_version": loaded.config.protocol_version,
        "config_path": loaded.config_path.display().to_string(),
        "runtime_root": loaded.config.directories.runtime_root.display().to_string(),
        "state_dir": loaded.config.directories.state_dir.display().to_string(),
        "socket_path": loaded.config.transport.socket_path.display().to_string(),
    })
    .to_string())
}
