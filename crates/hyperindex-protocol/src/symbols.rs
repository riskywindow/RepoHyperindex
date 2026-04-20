use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SymbolId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct OccurrenceId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct ParseBuildId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SymbolIndexBuildId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LanguageId {
    Typescript,
    Tsx,
    Javascript,
    Jsx,
    Mts,
    Cts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinePosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: LinePosition,
    pub end: LinePosition,
    pub bytes: ByteRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseInputSourceKind {
    BaseSnapshot,
    WorkingTreeOverlay,
    BufferOverlay,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseBuildState {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolIndexBuildState {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseArtifactStage {
    Planned,
    Parsed,
    FactsExtracted,
    Indexed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseDiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseDiagnosticCode {
    SyntaxError,
    UnsupportedLanguage,
    UnsupportedSyntax,
    TruncatedInput,
    PartialAnalysis,
    DuplicateFact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseDiagnostic {
    pub severity: ParseDiagnosticSeverity,
    pub code: ParseDiagnosticCode,
    pub message: String,
    pub path: Option<String>,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseBuildCounts {
    pub planned_file_count: u64,
    pub parsed_file_count: u64,
    #[serde(default)]
    pub reused_file_count: u64,
    pub skipped_file_count: u64,
    pub diagnostic_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseArtifactManifest {
    pub build_id: ParseBuildId,
    pub repo_id: String,
    pub snapshot_id: String,
    pub parser_config_digest: String,
    pub artifact_root: String,
    pub file_count: u64,
    pub diagnostic_count: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseBuildRecord {
    pub build_id: ParseBuildId,
    pub state: ParseBuildState,
    pub requested_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub counts: ParseBuildCounts,
    pub manifest: Option<ParseArtifactManifest>,
    #[serde(default)]
    pub loaded_from_existing_build: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileFactsSummary {
    pub symbol_count: u64,
    pub occurrence_count: u64,
    pub edge_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileParseArtifactMetadata {
    pub artifact_id: String,
    pub path: String,
    pub language: LanguageId,
    pub source_kind: ParseInputSourceKind,
    pub stage: ParseArtifactStage,
    pub content_sha256: String,
    pub content_bytes: u64,
    pub parser_pack_id: String,
    pub facts: FileFactsSummary,
    pub diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    File,
    Module,
    Namespace,
    Class,
    Interface,
    TypeAlias,
    Enum,
    EnumMember,
    Function,
    Method,
    Constructor,
    Property,
    Field,
    Variable,
    Constant,
    Parameter,
    ImportBinding,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceRole {
    Definition,
    Declaration,
    Reference,
    Import,
    Export,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeKind {
    Contains,
    #[serde(alias = "declares")]
    Defines,
    References,
    Imports,
    Exports,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolRecord {
    pub symbol_id: SymbolId,
    pub display_name: String,
    pub qualified_name: Option<String>,
    pub kind: SymbolKind,
    pub language: LanguageId,
    pub path: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolOccurrence {
    pub occurrence_id: OccurrenceId,
    pub symbol_id: SymbolId,
    pub path: String,
    pub span: SourceSpan,
    pub role: OccurrenceRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "node_kind", rename_all = "snake_case")]
pub enum GraphNodeRef {
    Symbol { symbol_id: SymbolId },
    Occurrence { occurrence_id: OccurrenceId },
    File { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub edge_id: String,
    pub kind: GraphEdgeKind,
    pub from: GraphNodeRef,
    pub to: GraphNodeRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileFacts {
    pub path: String,
    pub language: LanguageId,
    pub symbols: Vec<SymbolRecord>,
    pub occurrences: Vec<SymbolOccurrence>,
    pub edges: Vec<GraphEdge>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolIndexStats {
    pub file_count: u64,
    pub symbol_count: u64,
    pub occurrence_count: u64,
    pub edge_count: u64,
    pub diagnostic_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolIndexStorageFormat {
    Sqlite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolIndexStorage {
    pub format: SymbolIndexStorageFormat,
    pub path: String,
    pub schema_version: u32,
    pub manifest_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolIndexManifest {
    pub build_id: SymbolIndexBuildId,
    pub repo_id: String,
    pub snapshot_id: String,
    pub parser_build_id: ParseBuildId,
    pub created_at: String,
    pub stats: SymbolIndexStats,
    pub storage: SymbolIndexStorage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolIndexBuildRecord {
    pub build_id: SymbolIndexBuildId,
    pub state: SymbolIndexBuildState,
    pub requested_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub parser_build_id: ParseBuildId,
    pub stats: SymbolIndexStats,
    pub manifest: Option<SymbolIndexManifest>,
    #[serde(default)]
    pub refresh_mode: Option<String>,
    #[serde(default)]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub loaded_from_existing_build: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolSearchMode {
    Exact,
    Prefix,
    Substring,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolSearchQuery {
    pub text: String,
    pub mode: SymbolSearchMode,
    pub kinds: Vec<SymbolKind>,
    pub path_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolMatchKind {
    Exact,
    Prefix,
    Substring,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolSearchHit {
    pub symbol: SymbolRecord,
    pub match_kind: SymbolMatchKind,
    pub score: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "selector", rename_all = "snake_case")]
pub enum SymbolLocationSelector {
    LineColumn {
        path: String,
        line: u32,
        column: u32,
    },
    ByteOffset {
        path: String,
        offset: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedSymbol {
    pub symbol: SymbolRecord,
    pub occurrence: Option<SymbolOccurrence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseBuildParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseBuildResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub build: ParseBuildRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseStatusParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub build_id: Option<ParseBuildId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseStatusResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub builds: Vec<ParseBuildRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseInspectFileParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub path: String,
    pub include_facts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseInspectFileResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub artifact: FileParseArtifactMetadata,
    pub facts: Option<FileFacts>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolIndexBuildParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolIndexBuildResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub build: SymbolIndexBuildRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolIndexStatusParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub build_id: Option<SymbolIndexBuildId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolIndexStatusResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub builds: Vec<SymbolIndexBuildRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolSearchParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub query: SymbolSearchQuery,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolSearchResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub manifest: Option<SymbolIndexManifest>,
    pub hits: Vec<SymbolSearchHit>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolShowParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub symbol_id: SymbolId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolShowResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub manifest: Option<SymbolIndexManifest>,
    pub symbol: SymbolRecord,
    pub definitions: Vec<SymbolOccurrence>,
    pub related_edges: Vec<GraphEdge>,
    pub file: Option<FileParseArtifactMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefinitionLookupParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub symbol_id: SymbolId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefinitionLookupResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub symbol_id: SymbolId,
    pub manifest: Option<SymbolIndexManifest>,
    pub definitions: Vec<SymbolOccurrence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceLookupParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub symbol_id: SymbolId,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceLookupResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub symbol_id: SymbolId,
    pub manifest: Option<SymbolIndexManifest>,
    pub references: Vec<SymbolOccurrence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolResolveParams {
    pub repo_id: String,
    pub snapshot_id: String,
    pub selector: SymbolLocationSelector,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolResolveResponse {
    pub repo_id: String,
    pub snapshot_id: String,
    pub selector: SymbolLocationSelector,
    pub resolution: Option<ResolvedSymbol>,
    pub diagnostics: Vec<ParseDiagnostic>,
}
