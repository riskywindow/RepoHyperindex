pub mod api;
pub mod buffers;
pub mod config;
pub mod errors;
pub mod impact;
pub mod repo;
pub mod semantic;
pub mod snapshot;
pub mod status;
pub mod symbols;
pub mod watch;

pub const PROTOCOL_VERSION: &str = "repo-hyperindex.local/v1";
pub const CONFIG_VERSION: u32 = 1;
pub const STORAGE_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    use crate::api::{DaemonRequest, DaemonResponse};
    use crate::config::RuntimeConfig;
    use crate::errors::ProtocolError;
    use crate::semantic::{
        SemanticQueryFilters, SemanticQueryParams, SemanticQueryText, SemanticRerankMode,
    };

    #[derive(Debug, Serialize, Deserialize)]
    struct ProtocolFixtures {
        requests: Vec<DaemonRequest>,
        responses: Vec<DaemonResponse>,
    }

    #[test]
    fn runtime_config_toml_roundtrip_keeps_versions() {
        let config = RuntimeConfig::default();
        let raw = toml::to_string_pretty(&config).unwrap();
        let decoded: RuntimeConfig = toml::from_str(&raw).unwrap();
        assert_eq!(decoded.version, crate::CONFIG_VERSION);
        assert_eq!(decoded.protocol_version, crate::PROTOCOL_VERSION);
    }

    #[test]
    fn config_fixture_roundtrips_cleanly() {
        let fixture_path = fixture_path("fixtures/config/default-config.toml");
        let raw = fs::read_to_string(fixture_path).unwrap();
        let decoded: RuntimeConfig = toml::from_str(&raw).unwrap();
        assert_eq!(decoded.version, crate::CONFIG_VERSION);
        assert_eq!(decoded.protocol_version, crate::PROTOCOL_VERSION);
        let encoded = toml::to_string_pretty(&decoded).unwrap();
        let reparsed: RuntimeConfig = toml::from_str(&encoded).unwrap();
        assert_eq!(reparsed, decoded);
    }

    #[test]
    fn protocol_example_catalog_roundtrips_cleanly() {
        let fixture_path = fixture_path("fixtures/api/examples.json");
        let raw = fs::read_to_string(fixture_path).unwrap();
        let original: Value = serde_json::from_str(&raw).unwrap();
        let decoded: ProtocolFixtures = serde_json::from_str(&raw).unwrap();
        assert!(!decoded.requests.is_empty());
        assert!(!decoded.responses.is_empty());
        let encoded = serde_json::to_value(&decoded).unwrap();
        assert_eq!(encoded, original);
    }

    #[test]
    fn semantic_protocol_fixture_roundtrips_cleanly() {
        let fixture_path = fixture_path("fixtures/api/semantic-examples.json");
        let raw = fs::read_to_string(fixture_path).unwrap();
        let original: Value = serde_json::from_str(&raw).unwrap();
        let decoded: ProtocolFixtures = serde_json::from_str(&raw).unwrap();
        assert!(!decoded.requests.is_empty());
        assert!(!decoded.responses.is_empty());
        let encoded = serde_json::to_value(&decoded).unwrap();
        assert_eq!(encoded, original);
    }

    #[test]
    fn protocol_error_payload_roundtrips_cleanly() {
        let original = ProtocolError::invalid_field(
            "selector.line",
            "line must be greater than zero",
            Some("positive 1-based line number".to_string()),
        );
        let encoded = serde_json::to_string_pretty(&original).unwrap();
        let decoded: ProtocolError = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn semantic_query_params_roundtrip_cleanly() {
        let original = SemanticQueryParams {
            repo_id: "repo-123".to_string(),
            snapshot_id: "snap-123".to_string(),
            query: SemanticQueryText {
                text: "invalidate sessions".to_string(),
            },
            filters: SemanticQueryFilters {
                path_globs: vec!["apps/**".to_string(), "packages/**".to_string()],
                package_names: vec!["@hyperindex/auth".to_string()],
                package_roots: vec!["packages/auth".to_string()],
                workspace_roots: Vec::new(),
                languages: Vec::new(),
                extensions: vec!["ts".to_string()],
                symbol_kinds: Vec::new(),
            },
            limit: 10,
            rerank_mode: SemanticRerankMode::Hybrid,
        };
        let encoded = serde_json::to_string_pretty(&original).unwrap();
        let decoded: SemanticQueryParams = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    fn fixture_path(relative: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
    }
}
