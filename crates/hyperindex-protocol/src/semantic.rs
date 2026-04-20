use serde::{Deserialize, Serialize};

use crate::symbols::{LanguageId, SourceSpan, SymbolId, SymbolIndexBuildId, SymbolKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SemanticBuildId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SemanticChunkId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct EmbeddingCacheKey(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRerankMode {
    Off,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticAnalysisState {
    Disabled,
    NotReady,
    Ready,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticBuildState {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticStorageFormat {
    Sqlite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticEmbeddingProviderKind {
    DeterministicFixture,
    LocalOnnx,
    ExternalProcess,
    Placeholder,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticEmbeddingInputKind {
    Document,
    Query,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticChunkKind {
    SymbolBody,
    FileHeader,
    RouteFile,
    ConfigFile,
    TestFile,
    FallbackWindow,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticChunkSourceKind {
    Symbol,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDiagnosticSeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticDiagnostic {
    pub severity: SemanticDiagnosticSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticCapabilities {
    pub status: bool,
    pub build: bool,
    pub query: bool,
    pub inspect_chunk: bool,
    pub local_rebuild: bool,
    pub local_stats: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticEmbeddingProviderConfig {
    pub provider_kind: SemanticEmbeddingProviderKind,
    pub model_id: String,
    pub model_digest: String,
    pub vector_dimensions: u32,
    pub normalized: bool,
    pub max_input_bytes: u32,
    pub max_batch_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticEmbeddingCacheMetadata {
    pub cache_key: EmbeddingCacheKey,
    pub input_kind: SemanticEmbeddingInputKind,
    pub model_digest: String,
    pub text_digest: String,
    pub provider_config_digest: String,
    pub vector_dimensions: u32,
    pub normalized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stored_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticEmbeddingCacheManifest {
    pub key_algorithm: String,
    pub entry_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticChunkTextConfig {
    pub serializer_id: String,
    pub format_version: u32,
    pub includes_path_header: bool,
    pub includes_symbol_context: bool,
    pub normalized_newlines: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticChunkTextMetadata {
    pub serializer_id: String,
    pub format_version: u32,
    pub text_digest: String,
    pub text_bytes: u32,
    pub token_count_estimate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticChunkMetadata {
    pub chunk_id: SemanticChunkId,
    pub chunk_kind: SemanticChunkKind,
    pub source_kind: SemanticChunkSourceKind,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<SymbolId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<SymbolKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_is_exported: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_is_default_export: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    pub content_sha256: String,
    pub text: SemanticChunkTextMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticChunkRecord {
    pub metadata: SemanticChunkMetadata,
    pub serialized_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_cache: Option<SemanticEmbeddingCacheMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticQueryText {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SemanticQueryFilters {
    #[serde(default)]
    pub path_globs: Vec<String>,
    #[serde(default)]
    pub package_names: Vec<String>,
    #[serde(default)]
    pub package_roots: Vec<String>,
    #[serde(default)]
    pub workspace_roots: Vec<String>,
    #[serde(default)]
    pub languages: Vec<LanguageId>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub symbol_kinds: Vec<SymbolKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticQueryStats {
    pub limit_requested: u32,
    pub candidate_chunk_count: u32,
    pub filtered_chunk_count: u32,
    pub hits_returned: u32,
    pub rerank_applied: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticRefreshStats {
    pub files_touched: u64,
    pub chunks_rebuilt: u64,
    pub embeddings_regenerated: u64,
    pub vector_entries_added: u64,
    pub vector_entries_updated: u64,
    pub vector_entries_removed: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticIndexStorage {
    pub format: SemanticStorageFormat,
    pub path: String,
    pub schema_version: u32,
    pub manifest_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticIndexManifest {
    pub build_id: SemanticBuildId,
    pub repo_id: String,
    pub snapshot_id: String,
    pub semantic_config_digest: String,
    pub chunk_schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_index_build_id: Option<SymbolIndexBuildId>,
    pub embedding_provider: SemanticEmbeddingProviderConfig,
    pub chunk_text: SemanticChunkTextConfig,
    pub storage: SemanticIndexStorage,
    pub embedding_cache: SemanticEmbeddingCacheManifest,
    pub indexed_chunk_count: u64,
    pub indexed_file_count: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticBuildParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticBuildRecord {
    pub build_id: SemanticBuildId,
    pub state: SemanticBuildState,
    pub requested_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub manifest: Option<SemanticIndexManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_stats: Option<SemanticRefreshStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub diagnostics: Vec<SemanticDiagnostic>,
    #[serde(default)]
    pub loaded_from_existing_build: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticBuildResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub build: SemanticBuildRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticStatusParams {
    pub repo_id: String,
    pub snapshot_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_id: Option<SemanticBuildId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticStatusResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub state: SemanticAnalysisState,
    pub capabilities: SemanticCapabilities,
    #[serde(default)]
    pub builds: Vec<SemanticBuildRecord>,
    #[serde(default)]
    pub diagnostics: Vec<SemanticDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticQueryParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub query: SemanticQueryText,
    #[serde(default)]
    pub filters: SemanticQueryFilters,
    pub limit: u32,
    pub rerank_mode: SemanticRerankMode,
}

pub type SemanticSearchParams = SemanticQueryParams;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticRerankSignal {
    pub label: String,
    pub points: i32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SemanticRetrievalExplanation {
    #[serde(default)]
    pub query_terms: Vec<String>,
    #[serde(default)]
    pub text_term_hits: Vec<String>,
    #[serde(default)]
    pub path_term_hits: Vec<String>,
    #[serde(default)]
    pub symbol_term_hits: Vec<String>,
    #[serde(default)]
    pub package_term_hits: Vec<String>,
    #[serde(default)]
    pub signals: Vec<SemanticRerankSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticRetrievalHit {
    pub rank: u32,
    pub score: u32,
    pub semantic_score: u32,
    pub rerank_score: u32,
    pub chunk: SemanticChunkMetadata,
    pub reason: String,
    pub snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<SemanticRetrievalExplanation>,
}

pub type SemanticHit = SemanticRetrievalHit;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticQueryResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub query: SemanticQueryText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<SemanticIndexManifest>,
    #[serde(default)]
    pub hits: Vec<SemanticRetrievalHit>,
    pub stats: SemanticQueryStats,
    #[serde(default)]
    pub diagnostics: Vec<SemanticDiagnostic>,
}

pub type SemanticSearchResponse = SemanticQueryResponse;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticInspectChunkParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub chunk_id: SemanticChunkId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_id: Option<SemanticBuildId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticInspectChunkResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<SemanticIndexManifest>,
    pub chunk: SemanticChunkRecord,
    #[serde(default)]
    pub diagnostics: Vec<SemanticDiagnostic>,
}
