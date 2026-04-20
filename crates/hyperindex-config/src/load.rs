use std::fs;
use std::path::{Path, PathBuf};

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::{CONFIG_VERSION, PROTOCOL_VERSION, config::RuntimeConfig};

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config_path: PathBuf,
    pub config: RuntimeConfig,
}

pub fn default_config_path() -> PathBuf {
    RuntimeConfig::default()
        .directories
        .runtime_root
        .join("config.toml")
}

pub fn load_or_default(config_path: Option<&Path>) -> HyperindexResult<LoadedConfig> {
    let path = config_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_config_path);
    let config = if path.exists() {
        let raw = fs::read_to_string(&path)
            .map_err(|error| HyperindexError::Message(format!("failed to read config: {error}")))?;
        toml::from_str::<RuntimeConfig>(&raw).map_err(|error| {
            HyperindexError::InvalidConfig(format!("failed to parse {}: {error}", path.display()))
        })?
    } else {
        RuntimeConfig::default()
    };
    validate_versions(&config)?;
    Ok(LoadedConfig {
        config_path: path,
        config,
    })
}

pub fn write_default_config(
    config_path: Option<&Path>,
    force: bool,
) -> HyperindexResult<LoadedConfig> {
    let path = config_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_config_path);
    if path.exists() && !force {
        return Err(HyperindexError::Message(format!(
            "config already exists at {}",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            HyperindexError::Message(format!("failed to create {}: {error}", parent.display()))
        })?;
    }
    let config = RuntimeConfig::default();
    let raw = toml::to_string_pretty(&config).map_err(|error| {
        HyperindexError::Message(format!("failed to serialize config: {error}"))
    })?;
    fs::write(&path, raw).map_err(|error| {
        HyperindexError::Message(format!("failed to write {}: {error}", path.display()))
    })?;
    Ok(LoadedConfig {
        config_path: path,
        config,
    })
}

fn validate_versions(config: &RuntimeConfig) -> HyperindexResult<()> {
    if config.version != CONFIG_VERSION {
        return Err(HyperindexError::InvalidConfig(format!(
            "expected config version {CONFIG_VERSION}, found {}",
            config.version
        )));
    }
    if config.protocol_version != PROTOCOL_VERSION {
        return Err(HyperindexError::InvalidConfig(format!(
            "expected protocol version {PROTOCOL_VERSION}, found {}",
            config.protocol_version
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{load_or_default, write_default_config};

    #[test]
    fn write_and_load_default_config_roundtrips() {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("config.toml");
        let written = write_default_config(Some(&path), false).unwrap();
        let loaded = load_or_default(Some(&written.config_path)).unwrap();
        assert_eq!(loaded.config.version, written.config.version);
        assert_eq!(
            loaded.config.protocol_version,
            written.config.protocol_version
        );
    }
}
