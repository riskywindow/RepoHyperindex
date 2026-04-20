use std::path::Path;

use hyperindex_core::HyperindexResult;

use crate::commands::daemon;

pub fn render_status(config_path: Option<&Path>, json_output: bool) -> HyperindexResult<String> {
    daemon::status(config_path, json_output)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use hyperindex_config::write_default_config;

    use super::render_status;

    #[test]
    fn status_command_renders_json() {
        let tempdir = tempdir().unwrap();
        let config_path = tempdir.path().join("config.toml");
        write_default_config(Some(&config_path), false).unwrap();
        let output = render_status(Some(&config_path), true).unwrap();
        assert!(output.contains("\"runtime\""));
        assert!(output.contains("\"reachable\""));
    }
}
