use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use hyperindex_parser::LineIndex;
use hyperindex_protocol::semantic::{
    SemanticChunkId, SemanticChunkKind, SemanticChunkMetadata, SemanticChunkRecord,
    SemanticChunkSourceKind, SemanticChunkTextConfig, SemanticChunkTextMetadata,
    SemanticDiagnostic, SemanticDiagnosticSeverity,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::symbols::{SourceSpan, SymbolKind, SymbolRecord};
use hyperindex_snapshot::SnapshotAssembler;
use hyperindex_symbols::{ExtractedFileFacts, SymbolGraph, SymbolVisibility, SymbolWorkspace};
use tracing::info;

use crate::SemanticResult;
use crate::common::{sha256_hex, stable_digest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkingPlan {
    pub chunk_schema_version: u32,
    pub chunks: Vec<SemanticChunkRecord>,
    pub diagnostics: Vec<SemanticDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldChunker {
    chunk_schema_version: u32,
    text_config: SemanticChunkTextConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PackageMetadata {
    package_name: Option<String>,
    package_root: Option<String>,
    workspace_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileChunkPayload {
    serialized_text: String,
    span: Option<SourceSpan>,
}

impl ScaffoldChunker {
    pub fn new(chunk_schema_version: u32, text_config: SemanticChunkTextConfig) -> Self {
        Self {
            chunk_schema_version,
            text_config,
        }
    }

    pub fn build(&self, snapshot: &ComposedSnapshot) -> SemanticResult<ChunkingPlan> {
        let mut workspace = SymbolWorkspace::default();
        let prepared = workspace.prepare_snapshot(snapshot)?;
        let package_index = build_package_index(snapshot);
        let file_contents = build_resolved_file_contents(snapshot);
        let mut chunks = Vec::new();
        let mut fallback_file_count = 0usize;

        for file in &prepared.facts.files {
            let package = package_metadata_for(&package_index, &file.facts.path);
            let contents = file_contents
                .get(&file.facts.path)
                .map(String::as_str)
                .unwrap_or_default();
            let symbols = major_symbols_for_file(file, &prepared.graph);
            if symbols.is_empty() {
                fallback_file_count += 1;
                chunks.push(self.file_chunk(file, contents, &package));
                continue;
            }

            for symbol in symbols {
                chunks.push(self.symbol_chunk(file, contents, &prepared.graph, symbol, &package));
            }
        }

        let diagnostics = if chunks.is_empty() {
            vec![SemanticDiagnostic {
                severity: SemanticDiagnosticSeverity::Info,
                code: "semantic_chunks_empty".to_string(),
                message: "no semantic chunks were materialized for the snapshot".to_string(),
            }]
        } else {
            let mut diagnostics = vec![SemanticDiagnostic {
                severity: SemanticDiagnosticSeverity::Info,
                code: "semantic_chunks_materialized".to_string(),
                message: format!("materialized {} semantic chunks", chunks.len()),
            }];
            if fallback_file_count > 0 {
                diagnostics.push(SemanticDiagnostic {
                    severity: SemanticDiagnosticSeverity::Info,
                    code: "semantic_file_fallbacks_used".to_string(),
                    message: format!(
                        "used file-level fallback chunking for {} files without major symbols",
                        fallback_file_count
                    ),
                });
            }
            diagnostics
        };

        info!(
            snapshot_id = %snapshot.snapshot_id,
            repo_id = %snapshot.repo_id,
            chunk_schema_version = self.chunk_schema_version,
            chunk_count = chunks.len(),
            fallback_file_count,
            "materialized deterministic semantic chunks"
        );
        Ok(ChunkingPlan {
            chunk_schema_version: self.chunk_schema_version,
            chunks,
            diagnostics,
        })
    }

    fn symbol_chunk(
        &self,
        file: &ExtractedFileFacts,
        contents: &str,
        graph: &SymbolGraph,
        symbol: &SymbolRecord,
        package: &PackageMetadata,
    ) -> SemanticChunkRecord {
        let symbol_source = slice_for_span(contents, &symbol.span);
        let comment = extract_leading_comment(contents, symbol.span.start.line as usize);
        let import_context = import_context(file);
        let export_context = export_context_for_symbol(file, &symbol.symbol_id.0);
        let container_chain = container_chain(graph, &symbol.symbol_id.0);
        let direct_container = container_chain.last().cloned();

        let serialized_text = serialize_symbol_chunk(
            &self.text_config,
            &file.facts.path,
            language_name(&file.artifact.language),
            package,
            symbol,
            direct_container.as_deref(),
            &container_chain,
            comment.as_deref(),
            &import_context,
            &export_context,
            symbol_source,
        );
        let chunk_id = SemanticChunkId(format!(
            "chunk-{}",
            &stable_digest(&[
                &file.facts.path,
                &span_key(Some(&symbol.span)),
                chunk_kind_name(&SemanticChunkKind::SymbolBody),
                &symbol.symbol_id.0,
                &file.artifact.content_sha256,
                &self.chunk_schema_version.to_string(),
            ])[..16]
        ));

        SemanticChunkRecord {
            metadata: build_metadata(
                chunk_id,
                SemanticChunkKind::SymbolBody,
                SemanticChunkSourceKind::Symbol,
                file,
                package,
                Some(symbol),
                Some(symbol.span.clone()),
                &serialized_text,
                &self.text_config,
            ),
            serialized_text,
            embedding_cache: None,
        }
    }

    fn file_chunk(
        &self,
        file: &ExtractedFileFacts,
        contents: &str,
        package: &PackageMetadata,
    ) -> SemanticChunkRecord {
        let chunk_kind = fallback_chunk_kind(&file.facts.path);
        let payload = file_chunk_payload(contents, &chunk_kind);
        let import_context = import_context(file);
        let export_context = export_context_for_file(file);
        let serialized_text = serialize_file_chunk(
            &self.text_config,
            &file.facts.path,
            language_name(&file.artifact.language),
            package,
            &chunk_kind,
            payload.span.as_ref(),
            &import_context,
            &export_context,
            &payload.serialized_text,
        );
        let chunk_id = SemanticChunkId(format!(
            "chunk-{}",
            &stable_digest(&[
                &file.facts.path,
                &span_key(payload.span.as_ref()),
                chunk_kind_name(&chunk_kind),
                "-",
                &file.artifact.content_sha256,
                &self.chunk_schema_version.to_string(),
            ])[..16]
        ));

        SemanticChunkRecord {
            metadata: build_metadata(
                chunk_id,
                chunk_kind,
                SemanticChunkSourceKind::File,
                file,
                package,
                None,
                payload.span,
                &serialized_text,
                &self.text_config,
            ),
            serialized_text,
            embedding_cache: None,
        }
    }
}

fn build_metadata(
    chunk_id: SemanticChunkId,
    chunk_kind: SemanticChunkKind,
    source_kind: SemanticChunkSourceKind,
    file: &ExtractedFileFacts,
    package: &PackageMetadata,
    symbol: Option<&SymbolRecord>,
    span: Option<SourceSpan>,
    serialized_text: &str,
    text_config: &SemanticChunkTextConfig,
) -> SemanticChunkMetadata {
    let text_digest = sha256_hex(serialized_text.as_bytes());
    let (symbol_is_exported, symbol_is_default_export) = symbol
        .and_then(|symbol| {
            file.symbol_facts
                .iter()
                .find(|fact| fact.symbol.symbol_id == symbol.symbol_id)
        })
        .map(|fact| match fact.visibility {
            SymbolVisibility::Local => (Some(false), Some(false)),
            SymbolVisibility::Exported => (Some(true), Some(false)),
            SymbolVisibility::DefaultExport => (Some(true), Some(true)),
        })
        .unwrap_or((None, None));
    SemanticChunkMetadata {
        chunk_id,
        chunk_kind,
        source_kind,
        path: file.facts.path.clone(),
        language: Some(file.artifact.language.clone()),
        extension: Path::new(&file.facts.path)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_string()),
        package_name: package.package_name.clone(),
        package_root: package.package_root.clone(),
        workspace_root: package.workspace_root.clone(),
        symbol_id: symbol.map(|value| value.symbol_id.clone()),
        symbol_display_name: symbol.map(|value| value.display_name.clone()),
        symbol_kind: symbol.map(|value| value.kind.clone()),
        symbol_is_exported,
        symbol_is_default_export,
        span,
        content_sha256: file.artifact.content_sha256.clone(),
        text: SemanticChunkTextMetadata {
            serializer_id: text_config.serializer_id.clone(),
            format_version: text_config.format_version,
            text_digest,
            text_bytes: serialized_text.len() as u32,
            token_count_estimate: serialized_text.split_whitespace().count() as u32,
        },
    }
}

fn major_symbols_for_file<'a>(
    file: &'a ExtractedFileFacts,
    graph: &'a SymbolGraph,
) -> Vec<&'a SymbolRecord> {
    let mut symbols = file
        .facts
        .symbols
        .iter()
        .filter(|symbol| is_major_symbol(symbol, graph))
        .collect::<Vec<_>>();
    symbols.sort_by(|left, right| {
        left.span
            .bytes
            .start
            .cmp(&right.span.bytes.start)
            .then_with(|| left.span.bytes.end.cmp(&right.span.bytes.end))
            .then_with(|| left.symbol_id.0.cmp(&right.symbol_id.0))
    });
    symbols
}

fn is_major_symbol(symbol: &SymbolRecord, graph: &SymbolGraph) -> bool {
    match symbol.kind {
        SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::TypeAlias
        | SymbolKind::Enum
        | SymbolKind::Function
        | SymbolKind::Method
        | SymbolKind::Constructor => true,
        SymbolKind::Property | SymbolKind::Field => container_kind(graph, &symbol.symbol_id.0)
            .map(|kind| matches!(kind, SymbolKind::Class | SymbolKind::Interface))
            .unwrap_or(false),
        SymbolKind::Variable | SymbolKind::Constant => container_kind(graph, &symbol.symbol_id.0)
            .map(|kind| {
                matches!(
                    kind,
                    SymbolKind::Module
                        | SymbolKind::Namespace
                        | SymbolKind::Class
                        | SymbolKind::Interface
                )
            })
            .unwrap_or(true),
        SymbolKind::File
        | SymbolKind::Module
        | SymbolKind::Namespace
        | SymbolKind::EnumMember
        | SymbolKind::Parameter
        | SymbolKind::ImportBinding => false,
    }
}

fn container_kind(graph: &SymbolGraph, symbol_id: &str) -> Option<SymbolKind> {
    let container = graph
        .symbol_facts
        .get(symbol_id)
        .and_then(|record| record.container.clone())?;
    graph
        .symbols
        .get(&container.0)
        .map(|symbol| symbol.kind.clone())
}

fn container_chain(graph: &SymbolGraph, symbol_id: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = graph
        .symbol_facts
        .get(symbol_id)
        .and_then(|record| record.container.clone());
    while let Some(container) = current {
        let Some(symbol) = graph.symbols.get(&container.0) else {
            break;
        };
        if symbol.kind != SymbolKind::Module {
            chain.push(symbol.display_name.clone());
        }
        current = graph
            .symbol_facts
            .get(&container.0)
            .and_then(|record| record.container.clone());
    }
    chain.reverse();
    chain
}

fn file_chunk_payload(contents: &str, chunk_kind: &SemanticChunkKind) -> FileChunkPayload {
    let line_index = LineIndex::new(contents);
    match chunk_kind {
        SemanticChunkKind::RouteFile
        | SemanticChunkKind::ConfigFile
        | SemanticChunkKind::TestFile => FileChunkPayload {
            serialized_text: contents.to_string(),
            span: Some(line_index.byte_range_to_span(0, contents.len())),
        },
        _ => {
            let end_byte = header_end_byte(contents, 80, 4096);
            FileChunkPayload {
                serialized_text: contents[..end_byte].to_string(),
                span: Some(line_index.byte_range_to_span(0, end_byte)),
            }
        }
    }
}

fn header_end_byte(contents: &str, max_lines: usize, max_bytes: usize) -> usize {
    if contents.is_empty() {
        return 0;
    }
    let mut lines = 0usize;
    let mut end = 0usize;
    for (index, ch) in contents.char_indices() {
        if index > max_bytes {
            break;
        }
        end = index + ch.len_utf8();
        if ch == '\n' {
            lines += 1;
            if lines >= max_lines {
                break;
            }
        }
    }
    end.max(contents.len().min(max_bytes)).min(contents.len())
}

fn fallback_chunk_kind(path: &str) -> SemanticChunkKind {
    let lower = path.to_ascii_lowercase();
    let file_name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    if lower.contains("/__tests__/")
        || lower.contains("/tests/")
        || lower.contains("/test/")
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.starts_with("setup")
    {
        SemanticChunkKind::TestFile
    } else if file_name.contains(".config.")
        || matches!(
            file_name.as_str(),
            "vite.config.ts"
                | "vite.config.js"
                | "vitest.config.ts"
                | "vitest.config.js"
                | "next.config.js"
                | "next.config.mjs"
                | "eslint.config.js"
                | "playwright.config.ts"
                | "tailwind.config.ts"
                | "tailwind.config.js"
        )
    {
        SemanticChunkKind::ConfigFile
    } else if lower.contains("/routes/")
        || lower.contains("/route/")
        || lower.contains("/pages/")
        || lower.contains("/api/")
        || file_name.starts_with("route.")
        || file_name.contains(".route.")
    {
        SemanticChunkKind::RouteFile
    } else {
        SemanticChunkKind::FileHeader
    }
}

fn serialize_symbol_chunk(
    config: &SemanticChunkTextConfig,
    path: &str,
    language: &str,
    package: &PackageMetadata,
    symbol: &SymbolRecord,
    direct_container: Option<&str>,
    container_chain: &[String],
    comment: Option<&str>,
    import_context: &[String],
    export_context: &[String],
    source_text: &str,
) -> String {
    let mut sections = Vec::new();
    if config.includes_path_header {
        sections.push(header_lines(
            path,
            language,
            package,
            "symbol_body",
            "symbol",
            Some(&symbol.span),
        ));
    }

    let mut symbol_lines = vec![
        format!("symbol_name: {}", symbol.display_name),
        format!("symbol_kind: {}", symbol_kind_name(&symbol.kind)),
    ];
    if let Some(qualified_name) = &symbol.qualified_name {
        symbol_lines.push(format!("qualified_name: {qualified_name}"));
    }
    if let Some(container) = direct_container {
        symbol_lines.push(format!("container: {container}"));
    }
    if !container_chain.is_empty() {
        symbol_lines.push(format!("container_chain: {}", container_chain.join(" -> ")));
    }
    sections.push(symbol_lines.join("\n"));

    if config.includes_symbol_context {
        if let Some(comment) = comment {
            sections.push(format!("comment:\n{comment}"));
        }
        if !import_context.is_empty() {
            sections.push(list_section("imports", import_context));
        }
        if !export_context.is_empty() {
            sections.push(list_section("exports", export_context));
        }
    }
    sections.push(format!(
        "source:\n{}",
        normalize_source_text(source_text, config.normalized_newlines)
    ));
    normalize_serialized_text(&sections.join("\n\n"), config.normalized_newlines)
}

fn serialize_file_chunk(
    config: &SemanticChunkTextConfig,
    path: &str,
    language: &str,
    package: &PackageMetadata,
    chunk_kind: &SemanticChunkKind,
    span: Option<&SourceSpan>,
    import_context: &[String],
    export_context: &[String],
    source_text: &str,
) -> String {
    let mut sections = Vec::new();
    if config.includes_path_header {
        sections.push(header_lines(
            path,
            language,
            package,
            chunk_kind_name(chunk_kind),
            "file",
            span,
        ));
    }
    if config.includes_symbol_context {
        if !import_context.is_empty() {
            sections.push(list_section("imports", import_context));
        }
        if !export_context.is_empty() {
            sections.push(list_section("exports", export_context));
        }
    }
    sections.push(format!(
        "source:\n{}",
        normalize_source_text(source_text, config.normalized_newlines)
    ));
    normalize_serialized_text(&sections.join("\n\n"), config.normalized_newlines)
}

fn header_lines(
    path: &str,
    language: &str,
    package: &PackageMetadata,
    chunk_kind: &str,
    source_kind: &str,
    span: Option<&SourceSpan>,
) -> String {
    let mut lines = vec![
        format!("path: {path}"),
        format!("language: {language}"),
        format!("chunk_kind: {chunk_kind}"),
        format!("source_kind: {source_kind}"),
    ];
    if let Some(package_name) = &package.package_name {
        lines.push(format!("package_name: {package_name}"));
    }
    if let Some(package_root) = &package.package_root {
        lines.push(format!("package_root: {package_root}"));
    }
    if let Some(workspace_root) = &package.workspace_root {
        lines.push(format!("workspace_root: {workspace_root}"));
    }
    if let Some(span) = span {
        lines.push(format!("source_span: {}", render_span(span)));
    }
    lines.join("\n")
}

fn render_span(span: &SourceSpan) -> String {
    format!(
        "{}:{}-{}:{} bytes {}-{}",
        span.start.line,
        span.start.column,
        span.end.line,
        span.end.column,
        span.bytes.start,
        span.bytes.end
    )
}

fn list_section(name: &str, values: &[String]) -> String {
    let mut lines = vec![format!("{name}:")];
    lines.extend(values.iter().map(|value| format!("- {value}")));
    lines.join("\n")
}

fn import_context(file: &ExtractedFileFacts) -> Vec<String> {
    let mut values = file
        .import_facts
        .iter()
        .map(|record| record.module_specifier.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    values.truncate(8);
    values
}

fn export_context_for_symbol(file: &ExtractedFileFacts, symbol_id: &str) -> Vec<String> {
    let mut values = file
        .export_facts
        .iter()
        .filter(|record| record.symbol_id.0 == symbol_id)
        .map(render_export_fact)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    values.truncate(8);
    values
}

fn export_context_for_file(file: &ExtractedFileFacts) -> Vec<String> {
    let mut values = file
        .export_facts
        .iter()
        .map(render_export_fact)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    values.truncate(8);
    values
}

fn render_export_fact(record: &hyperindex_symbols::ExportFactRecord) -> String {
    let local = record
        .local_name
        .clone()
        .unwrap_or_else(|| record.exported_name.clone());
    let base = if record.is_default {
        if record.exported_name == "default" {
            format!("default ({local})")
        } else {
            format!("default as {}", record.exported_name)
        }
    } else if local == record.exported_name {
        record.exported_name.clone()
    } else {
        format!("{local} as {}", record.exported_name)
    };
    match &record.module_specifier {
        Some(specifier) => format!("{base} from {specifier}"),
        None => base,
    }
}

fn extract_leading_comment(contents: &str, start_line: usize) -> Option<String> {
    if start_line <= 1 {
        return None;
    }
    let normalized = normalize_line_endings(contents);
    let lines = normalized.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    let mut index = start_line.saturating_sub(2);
    while lines.get(index).is_some_and(|line| line.trim().is_empty()) {
        if index == 0 {
            return None;
        }
        index -= 1;
    }

    let line = lines.get(index)?.trim();
    if line.starts_with("//") {
        let mut start = index;
        while start > 0 && lines[start - 1].trim().starts_with("//") {
            start -= 1;
        }
        return Some(clean_comment_lines(&lines[start..=index]));
    }
    if line.ends_with("*/") {
        let mut start = index;
        while start > 0 && !lines[start].contains("/*") {
            start -= 1;
        }
        if lines[start].contains("/*") {
            return Some(clean_comment_lines(&lines[start..=index]));
        }
    }
    None
}

fn clean_comment_lines(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| {
            line.trim()
                .trim_start_matches("/*")
                .trim_start_matches('*')
                .trim_start_matches("//")
                .trim_end_matches("*/")
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_resolved_file_contents(snapshot: &ComposedSnapshot) -> BTreeMap<String, String> {
    let assembler = SnapshotAssembler;
    let mut paths = BTreeSet::new();
    for file in &snapshot.base.files {
        paths.insert(file.path.clone());
    }
    for entry in &snapshot.working_tree.entries {
        paths.insert(entry.path.clone());
    }
    for buffer in &snapshot.buffers {
        paths.insert(buffer.path.clone());
    }

    let mut contents = BTreeMap::new();
    for path in paths {
        if let Some(resolved) = assembler.resolve_file(snapshot, &path) {
            contents.insert(path, resolved.contents);
        }
    }
    contents
}

fn slice_for_span<'a>(contents: &'a str, span: &SourceSpan) -> &'a str {
    let start = span.bytes.start as usize;
    let end = span.bytes.end as usize;
    contents.get(start..end).unwrap_or("")
}

fn span_key(span: Option<&SourceSpan>) -> String {
    match span {
        Some(span) => format!("{}-{}", span.bytes.start, span.bytes.end),
        None => "-".to_string(),
    }
}

fn chunk_kind_name(kind: &SemanticChunkKind) -> &'static str {
    match kind {
        SemanticChunkKind::SymbolBody => "symbol_body",
        SemanticChunkKind::FileHeader => "file_header",
        SemanticChunkKind::RouteFile => "route_file",
        SemanticChunkKind::ConfigFile => "config_file",
        SemanticChunkKind::TestFile => "test_file",
        SemanticChunkKind::FallbackWindow => "fallback_window",
    }
}

fn symbol_kind_name(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::File => "file",
        SymbolKind::Module => "module",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "interface",
        SymbolKind::TypeAlias => "type_alias",
        SymbolKind::Enum => "enum",
        SymbolKind::EnumMember => "enum_member",
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Constructor => "constructor",
        SymbolKind::Property => "property",
        SymbolKind::Field => "field",
        SymbolKind::Variable => "variable",
        SymbolKind::Constant => "constant",
        SymbolKind::Parameter => "parameter",
        SymbolKind::ImportBinding => "import_binding",
    }
}

fn language_name(language: &hyperindex_protocol::symbols::LanguageId) -> &'static str {
    match language {
        hyperindex_protocol::symbols::LanguageId::Typescript => "typescript",
        hyperindex_protocol::symbols::LanguageId::Tsx => "tsx",
        hyperindex_protocol::symbols::LanguageId::Javascript => "javascript",
        hyperindex_protocol::symbols::LanguageId::Jsx => "jsx",
        hyperindex_protocol::symbols::LanguageId::Mts => "mts",
        hyperindex_protocol::symbols::LanguageId::Cts => "cts",
    }
}

fn normalize_source_text(text: &str, normalized_newlines: bool) -> String {
    normalize_serialized_text(text, normalized_newlines)
}

fn normalize_serialized_text(text: &str, normalized_newlines: bool) -> String {
    let base = if normalized_newlines {
        normalize_line_endings(text)
    } else {
        text.to_string()
    };
    base.lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn build_package_index(snapshot: &ComposedSnapshot) -> BTreeMap<String, String> {
    let assembler = SnapshotAssembler;
    let mut package_paths = BTreeSet::new();
    for file in &snapshot.base.files {
        if file.path == "package.json" || file.path.ends_with("/package.json") {
            package_paths.insert(file.path.clone());
        }
    }
    for entry in &snapshot.working_tree.entries {
        if entry.path == "package.json" || entry.path.ends_with("/package.json") {
            package_paths.insert(entry.path.clone());
        }
    }
    for buffer in &snapshot.buffers {
        if buffer.path == "package.json" || buffer.path.ends_with("/package.json") {
            package_paths.insert(buffer.path.clone());
        }
    }

    let mut packages = BTreeMap::new();
    for path in package_paths {
        let Some(resolved) = assembler.resolve_file(snapshot, &path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&resolved.contents) else {
            continue;
        };
        let Some(name) = value
            .get("name")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
        else {
            continue;
        };
        let package_root = path
            .strip_suffix("/package.json")
            .unwrap_or(".")
            .to_string();
        packages.insert(package_root, name);
    }
    packages
}

fn package_metadata_for(package_index: &BTreeMap<String, String>, path: &str) -> PackageMetadata {
    let mut selected: Option<(String, String)> = None;
    for (root, name) in package_index {
        if path_matches_root(path, root) {
            match &selected {
                Some((current_root, _)) if current_root.len() >= root.len() => {}
                _ => selected = Some((root.clone(), name.clone())),
            }
        }
    }
    PackageMetadata {
        package_name: selected.as_ref().map(|(_, name)| name.clone()),
        package_root: selected.as_ref().map(|(root, _)| root.clone()),
        workspace_root: Some(".".to_string()),
    }
}

fn path_matches_root(path: &str, root: &str) -> bool {
    if root == "." {
        return true;
    }
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::semantic::{
        SemanticChunkKind, SemanticChunkTextConfig, SemanticChunkTextMetadata,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, BufferOverlay, ComposedSnapshot, WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::SymbolKind;
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use super::ScaffoldChunker;

    #[test]
    fn chunking_is_deterministic_for_same_snapshot() {
        let snapshot = semantic_fixture_snapshot();
        let chunker = default_chunker(1);

        let left = chunker.build(&snapshot).unwrap();
        let right = chunker.build(&snapshot).unwrap();

        assert_eq!(left, right);
        assert!(!left.chunks.is_empty());
    }

    #[test]
    fn chunk_ids_change_when_schema_version_changes() {
        let snapshot = semantic_fixture_snapshot();
        let first = default_chunker(1).build(&snapshot).unwrap();
        let second = default_chunker(2).build(&snapshot).unwrap();

        let first_id = first
            .chunks
            .iter()
            .find(|chunk| {
                chunk.metadata.symbol_display_name.as_deref() == Some("invalidateSession")
            })
            .unwrap()
            .metadata
            .chunk_id
            .0
            .clone();
        let second_id = second
            .chunks
            .iter()
            .find(|chunk| {
                chunk.metadata.symbol_display_name.as_deref() == Some("invalidateSession")
            })
            .unwrap()
            .metadata
            .chunk_id
            .0
            .clone();

        assert_ne!(first_id, second_id);
    }

    #[test]
    fn buffer_overlays_change_chunk_content() {
        let mut snapshot = semantic_fixture_snapshot();
        snapshot.buffers.push(BufferOverlay {
            buffer_id: "buffer-1".to_string(),
            path: "packages/auth/src/session.ts".to_string(),
            version: 1,
            content_sha256: "overlay-sha".to_string(),
            content_bytes: 172,
            contents: r#"import { cookies } from "./cookies";

// Overlay comment
export function invalidateSession(sessionId: string): string {
  return `overlay:${sessionId}`;
}
"#
            .to_string(),
        });

        let plan = default_chunker(1).build(&snapshot).unwrap();
        let chunk = plan
            .chunks
            .iter()
            .find(|chunk| {
                chunk.metadata.symbol_display_name.as_deref() == Some("invalidateSession")
            })
            .unwrap();

        assert!(chunk.serialized_text.contains("overlay:"));
        assert!(chunk.serialized_text.contains("Overlay comment"));
        assert!(!chunk.serialized_text.contains("base:"));
    }

    #[test]
    fn symbol_and_file_fallback_chunks_materialize_on_realistic_fixture() {
        let plan = default_chunker(1)
            .build(&semantic_fixture_snapshot())
            .unwrap();

        let symbol_chunk = plan
            .chunks
            .iter()
            .find(|chunk| {
                chunk.metadata.symbol_display_name.as_deref() == Some("invalidateSession")
            })
            .unwrap();
        assert_eq!(
            symbol_chunk.metadata.chunk_kind,
            SemanticChunkKind::SymbolBody
        );
        assert_eq!(
            symbol_chunk.metadata.symbol_kind,
            Some(SymbolKind::Function)
        );
        assert!(symbol_chunk.serialized_text.contains("imports:"));
        assert!(symbol_chunk.serialized_text.contains("exports:"));

        let config_chunk = plan
            .chunks
            .iter()
            .find(|chunk| chunk.metadata.path == "apps/web/vite.config.ts")
            .unwrap();
        assert_eq!(
            config_chunk.metadata.chunk_kind,
            SemanticChunkKind::ConfigFile
        );
        assert_eq!(
            config_chunk.metadata.source_kind,
            hyperindex_protocol::semantic::SemanticChunkSourceKind::File
        );
        assert!(config_chunk.serialized_text.contains("defineConfig"));
    }

    fn default_chunker(chunk_schema_version: u32) -> ScaffoldChunker {
        ScaffoldChunker::new(
            chunk_schema_version,
            SemanticChunkTextConfig {
                serializer_id: "phase6-structured-text".to_string(),
                format_version: 1,
                includes_path_header: true,
                includes_symbol_context: true,
                normalized_newlines: true,
            },
        )
    }

    fn semantic_fixture_snapshot() -> ComposedSnapshot {
        fn file(path: &str, contents: &str) -> hyperindex_protocol::snapshot::SnapshotFile {
            hyperindex_protocol::snapshot::SnapshotFile {
                path: path.to_string(),
                content_sha256: format!("sha-{}", path.replace('/', "-")),
                content_bytes: contents.len(),
                contents: contents.to_string(),
            }
        }

        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: "snap-semantic-fixture".to_string(),
            repo_id: "repo-semantic-fixture".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "deadbeef".to_string(),
                digest: "base-semantic".to_string(),
                file_count: 5,
                files: vec![
                    file("package.json", r#"{ "name": "repo-root" }"#),
                    file(
                        "packages/auth/package.json",
                        r#"{ "name": "@hyperindex/auth" }"#,
                    ),
                    file(
                        "packages/auth/src/session.ts",
                        r#"import { cookies } from "./cookies";

// Invalidates the active session.
export function invalidateSession(sessionId: string): string {
  return `base:${sessionId}`;
}
"#,
                    ),
                    file(
                        "apps/web/vite.config.ts",
                        r#"import { defineConfig } from "vite";

export default defineConfig({
  server: {
    port: 3000,
  },
});
"#,
                    ),
                    file(
                        "apps/web/src/routes/logout.ts",
                        r#"export { invalidateSession } from "../../../packages/auth/src/session";
"#,
                    ),
                ],
            },
            working_tree: WorkingTreeOverlay {
                digest: "working-tree".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    #[test]
    fn text_metadata_digest_tracks_serialized_text() {
        let plan = default_chunker(1)
            .build(&semantic_fixture_snapshot())
            .unwrap();
        let chunk = plan.chunks.first().unwrap();
        let text = &chunk.serialized_text;
        let metadata = &chunk.metadata.text;
        assert_eq!(metadata.text_bytes, text.len() as u32);
        assert_eq!(metadata.serializer_id, "phase6-structured-text");
        assert!(metadata.token_count_estimate > 0);
        assert_eq!(
            *metadata,
            SemanticChunkTextMetadata {
                serializer_id: "phase6-structured-text".to_string(),
                format_version: 1,
                text_digest: metadata.text_digest.clone(),
                text_bytes: text.len() as u32,
                token_count_estimate: metadata.token_count_estimate,
            }
        );
    }
}
