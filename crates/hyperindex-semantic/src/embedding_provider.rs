use std::io::Write;
use std::process::{Command, Stdio};

use hyperindex_protocol::config::{SemanticConfig, SemanticEmbeddingRuntimeConfig};
use hyperindex_protocol::semantic::{
    SemanticEmbeddingInputKind, SemanticEmbeddingProviderConfig, SemanticEmbeddingProviderKind,
};
use serde::{Deserialize, Serialize};

use crate::common::stable_digest;
use crate::{SemanticError, SemanticResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProviderIdentity {
    pub provider_kind: SemanticEmbeddingProviderKind,
    pub provider_id: String,
    pub provider_version: String,
    pub model_id: String,
    pub model_digest: String,
}

pub trait EmbeddingProvider {
    fn identity(&self) -> &EmbeddingProviderIdentity;
    fn config(&self) -> &SemanticEmbeddingProviderConfig;
    fn config_digest(&self) -> &str;

    fn embed_documents(&self, texts: &[String]) -> SemanticResult<Vec<Vec<f32>>> {
        self.embed(SemanticEmbeddingInputKind::Document, texts)
    }

    fn embed_queries(&self, texts: &[String]) -> SemanticResult<Vec<Vec<f32>>> {
        self.embed(SemanticEmbeddingInputKind::Query, texts)
    }

    fn embed(
        &self,
        input_kind: SemanticEmbeddingInputKind,
        texts: &[String],
    ) -> SemanticResult<Vec<Vec<f32>>>;
}

#[derive(Debug, Clone)]
pub struct DeterministicFixtureEmbeddingProvider {
    identity: EmbeddingProviderIdentity,
    config: SemanticEmbeddingProviderConfig,
    config_digest: String,
}

impl DeterministicFixtureEmbeddingProvider {
    pub fn new(config: SemanticEmbeddingProviderConfig) -> Self {
        let identity = EmbeddingProviderIdentity {
            provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
            provider_id: "deterministic-fixture".to_string(),
            provider_version: "v1".to_string(),
            model_id: config.model_id.clone(),
            model_digest: config.model_digest.clone(),
        };
        let config_digest = provider_config_digest(&identity, &config);
        Self {
            identity,
            config,
            config_digest,
        }
    }
}

impl EmbeddingProvider for DeterministicFixtureEmbeddingProvider {
    fn identity(&self) -> &EmbeddingProviderIdentity {
        &self.identity
    }

    fn config(&self) -> &SemanticEmbeddingProviderConfig {
        &self.config
    }

    fn config_digest(&self) -> &str {
        &self.config_digest
    }

    fn embed(
        &self,
        input_kind: SemanticEmbeddingInputKind,
        texts: &[String],
    ) -> SemanticResult<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| deterministic_vector(&self.identity, &self.config, &input_kind, text))
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct ExternalProcessEmbeddingProvider {
    identity: EmbeddingProviderIdentity,
    config: SemanticEmbeddingProviderConfig,
    runtime: SemanticEmbeddingRuntimeConfig,
    config_digest: String,
}

impl ExternalProcessEmbeddingProvider {
    pub fn new(
        identity: EmbeddingProviderIdentity,
        config: SemanticEmbeddingProviderConfig,
        runtime: SemanticEmbeddingRuntimeConfig,
    ) -> SemanticResult<Self> {
        if runtime
            .command
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            return Err(SemanticError::EmbeddingProviderMisconfigured(format!(
                "provider {} requires semantic.embedding_runtime.command",
                identity.provider_id
            )));
        }
        let config_digest = provider_config_digest(&identity, &config);
        Ok(Self {
            identity,
            config,
            runtime,
            config_digest,
        })
    }
}

impl EmbeddingProvider for ExternalProcessEmbeddingProvider {
    fn identity(&self) -> &EmbeddingProviderIdentity {
        &self.identity
    }

    fn config(&self) -> &SemanticEmbeddingProviderConfig {
        &self.config
    }

    fn config_digest(&self) -> &str {
        &self.config_digest
    }

    fn embed(
        &self,
        input_kind: SemanticEmbeddingInputKind,
        texts: &[String],
    ) -> SemanticResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let command = self.runtime.command.as_deref().unwrap_or_default();
        let mut child = Command::new(command)
            .args(&self.runtime.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                SemanticError::EmbeddingProviderFailed(format!(
                    "failed to start embedding command {command}: {error}"
                ))
            })?;

        let request = ExternalEmbeddingRequest {
            input_kind,
            texts: texts.to_vec(),
            model_id: self.identity.model_id.clone(),
            model_digest: self.identity.model_digest.clone(),
            vector_dimensions: self.config.vector_dimensions,
            normalized: self.config.normalized,
        };

        child
            .stdin
            .as_mut()
            .ok_or_else(|| {
                SemanticError::EmbeddingProviderFailed(
                    "embedding provider stdin was not available".to_string(),
                )
            })?
            .write_all(
                &serde_json::to_vec(&request)
                    .map_err(|error| SemanticError::EmbeddingProviderFailed(error.to_string()))?,
            )
            .map_err(|error| {
                SemanticError::EmbeddingProviderFailed(format!(
                    "failed to write embedding request: {error}"
                ))
            })?;

        let output = child.wait_with_output().map_err(|error| {
            SemanticError::EmbeddingProviderFailed(format!(
                "failed to wait for embedding command {command}: {error}"
            ))
        })?;
        if !output.status.success() {
            return Err(SemanticError::EmbeddingProviderFailed(format!(
                "embedding command {command} exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        let response: ExternalEmbeddingResponse =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                SemanticError::EmbeddingOutputInvalid(format!(
                    "embedding command returned invalid json: {error}"
                ))
            })?;
        validate_vectors(
            texts.len(),
            self.config.vector_dimensions as usize,
            &response.vectors,
        )?;
        Ok(response.vectors)
    }
}

pub fn provider_from_config(config: &SemanticConfig) -> SemanticResult<Box<dyn EmbeddingProvider>> {
    let identity = provider_identity_from_config(config);
    match config.embedding_provider.provider_kind {
        SemanticEmbeddingProviderKind::DeterministicFixture
        | SemanticEmbeddingProviderKind::Placeholder => Ok(Box::new(
            DeterministicFixtureEmbeddingProvider::new(config.embedding_provider.clone()),
        )),
        SemanticEmbeddingProviderKind::LocalOnnx => {
            Ok(Box::new(ExternalProcessEmbeddingProvider::new(
                identity,
                config.embedding_provider.clone(),
                config.embedding_runtime.clone(),
            )?))
        }
        SemanticEmbeddingProviderKind::ExternalProcess => {
            Ok(Box::new(ExternalProcessEmbeddingProvider::new(
                identity,
                config.embedding_provider.clone(),
                config.embedding_runtime.clone(),
            )?))
        }
    }
}

pub fn provider_identity_from_config(config: &SemanticConfig) -> EmbeddingProviderIdentity {
    let provider_kind = config.embedding_provider.provider_kind.clone();
    let provider_id = match provider_kind {
        SemanticEmbeddingProviderKind::DeterministicFixture => "deterministic-fixture",
        SemanticEmbeddingProviderKind::LocalOnnx => "local-onnx-process",
        SemanticEmbeddingProviderKind::ExternalProcess => "external-process",
        SemanticEmbeddingProviderKind::Placeholder => "placeholder-fixture",
    };
    EmbeddingProviderIdentity {
        provider_kind,
        provider_id: provider_id.to_string(),
        provider_version: "v1".to_string(),
        model_id: config.embedding_provider.model_id.clone(),
        model_digest: config.embedding_provider.model_digest.clone(),
    }
}

pub fn provider_config_digest(
    identity: &EmbeddingProviderIdentity,
    config: &SemanticEmbeddingProviderConfig,
) -> String {
    stable_digest(&[
        embedding_provider_kind_name(&identity.provider_kind),
        &identity.provider_id,
        &identity.provider_version,
        &identity.model_id,
        &identity.model_digest,
        &config.vector_dimensions.to_string(),
        &config.normalized.to_string(),
        &config.max_input_bytes.to_string(),
        &config.max_batch_size.to_string(),
    ])
}

fn deterministic_vector(
    identity: &EmbeddingProviderIdentity,
    config: &SemanticEmbeddingProviderConfig,
    input_kind: &SemanticEmbeddingInputKind,
    text: &str,
) -> Vec<f32> {
    let dimensions = config.vector_dimensions as usize;
    let mut values = Vec::with_capacity(dimensions);
    let mut block = 0usize;
    while values.len() < dimensions {
        let digest = stable_digest(&[
            embedding_provider_kind_name(&identity.provider_kind),
            &identity.provider_id,
            &identity.provider_version,
            &identity.model_digest,
            embedding_input_kind_name(input_kind),
            text,
            &block.to_string(),
        ]);
        for chunk in digest.as_bytes().chunks(4) {
            if values.len() == dimensions {
                break;
            }
            let mut bytes = [0u8; 4];
            for (index, value) in chunk.iter().enumerate() {
                bytes[index] = *value;
            }
            let raw = u32::from_le_bytes(bytes);
            let scaled = (raw as f32 / u32::MAX as f32) * 2.0 - 1.0;
            values.push(scaled);
        }
        block += 1;
    }
    if config.normalized {
        normalize(values)
    } else {
        values
    }
}

fn normalize(values: Vec<f32>) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        return values;
    }
    values.into_iter().map(|value| value / norm).collect()
}

fn validate_vectors(
    expected_count: usize,
    expected_dimensions: usize,
    vectors: &[Vec<f32>],
) -> SemanticResult<()> {
    if vectors.len() != expected_count {
        return Err(SemanticError::EmbeddingOutputInvalid(format!(
            "embedding provider returned {} vectors for {} inputs",
            vectors.len(),
            expected_count
        )));
    }
    for (index, vector) in vectors.iter().enumerate() {
        if vector.len() != expected_dimensions {
            return Err(SemanticError::EmbeddingOutputInvalid(format!(
                "embedding vector {index} had {} dimensions; expected {expected_dimensions}",
                vector.len()
            )));
        }
    }
    Ok(())
}

fn embedding_provider_kind_name(kind: &SemanticEmbeddingProviderKind) -> &'static str {
    match kind {
        SemanticEmbeddingProviderKind::DeterministicFixture => "deterministic_fixture",
        SemanticEmbeddingProviderKind::LocalOnnx => "local_onnx",
        SemanticEmbeddingProviderKind::ExternalProcess => "external_process",
        SemanticEmbeddingProviderKind::Placeholder => "placeholder",
    }
}

fn embedding_input_kind_name(kind: &SemanticEmbeddingInputKind) -> &'static str {
    match kind {
        SemanticEmbeddingInputKind::Document => "document",
        SemanticEmbeddingInputKind::Query => "query",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExternalEmbeddingRequest {
    input_kind: SemanticEmbeddingInputKind,
    texts: Vec<String>,
    model_id: String,
    model_digest: String,
    vector_dimensions: u32,
    normalized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExternalEmbeddingResponse {
    vectors: Vec<Vec<f32>>,
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::config::{SemanticConfig, SemanticEmbeddingRuntimeConfig};
    use hyperindex_protocol::semantic::{
        SemanticEmbeddingInputKind, SemanticEmbeddingProviderKind,
    };

    use super::{
        DeterministicFixtureEmbeddingProvider, EmbeddingProvider, EmbeddingProviderIdentity,
        ExternalProcessEmbeddingProvider, provider_config_digest,
    };

    #[test]
    fn deterministic_fixture_provider_is_stable_for_same_input() {
        let provider = DeterministicFixtureEmbeddingProvider::new(
            SemanticConfig::default().embedding_provider,
        );
        let left = provider
            .embed_documents(&["invalidate sessions".to_string()])
            .unwrap();
        let right = provider
            .embed_documents(&["invalidate sessions".to_string()])
            .unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn deterministic_fixture_distinguishes_query_and_document_paths() {
        let provider = DeterministicFixtureEmbeddingProvider::new(
            SemanticConfig::default().embedding_provider,
        );
        let document = provider
            .embed_documents(&["invalidate sessions".to_string()])
            .unwrap();
        let query = provider
            .embed_queries(&["invalidate sessions".to_string()])
            .unwrap();
        assert_ne!(document, query);
    }

    #[test]
    fn provider_config_digest_changes_when_version_changes() {
        let config = SemanticConfig::default().embedding_provider;
        let left = provider_config_digest(
            &EmbeddingProviderIdentity {
                provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
                provider_id: "fixture".to_string(),
                provider_version: "v1".to_string(),
                model_id: config.model_id.clone(),
                model_digest: config.model_digest.clone(),
            },
            &config,
        );
        let right = provider_config_digest(
            &EmbeddingProviderIdentity {
                provider_kind: SemanticEmbeddingProviderKind::DeterministicFixture,
                provider_id: "fixture".to_string(),
                provider_version: "v2".to_string(),
                model_id: config.model_id.clone(),
                model_digest: config.model_digest.clone(),
            },
            &config,
        );
        assert_ne!(left, right);
    }

    #[test]
    fn external_process_provider_parses_real_command_output() {
        let mut config = SemanticConfig::default().embedding_provider;
        config.vector_dimensions = 2;
        let provider = ExternalProcessEmbeddingProvider::new(
            EmbeddingProviderIdentity {
                provider_kind: SemanticEmbeddingProviderKind::ExternalProcess,
                provider_id: "external-process".to_string(),
                provider_version: "v1".to_string(),
                model_id: config.model_id.clone(),
                model_digest: config.model_digest.clone(),
            },
            config,
            SemanticEmbeddingRuntimeConfig {
                command: Some("/bin/sh".to_string()),
                args: vec![
                    "-c".to_string(),
                    "printf '{\"vectors\":[[1.0,0.0],[0.0,1.0]]}'".to_string(),
                ],
            },
        )
        .unwrap();
        let vectors = provider
            .embed(
                SemanticEmbeddingInputKind::Document,
                &["a".to_string(), "b".to_string()],
            )
            .unwrap();
        assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    }
}
