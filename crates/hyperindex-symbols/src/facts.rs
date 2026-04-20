use std::collections::{BTreeMap, BTreeSet};

use hyperindex_parser::ParseArtifact;
use hyperindex_protocol::symbols::{
    FileFacts as ProtocolFileFacts, FileFactsSummary, FileParseArtifactMetadata, GraphEdge,
    GraphEdgeKind, GraphNodeRef, LanguageId, OccurrenceId, OccurrenceRole, ParseArtifactStage,
    ParseDiagnostic, ParseDiagnosticCode, ParseDiagnosticSeverity, SourceSpan, SymbolId,
    SymbolKind, SymbolOccurrence, SymbolRecord,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::debug;
use tree_sitter::Node;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolVisibility {
    Local,
    Exported,
    DefaultExport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolFactRecord {
    pub symbol: SymbolRecord,
    pub container: Option<SymbolId>,
    pub visibility: SymbolVisibility,
    pub file_path: String,
    pub signature_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportFactRecord {
    pub symbol_id: SymbolId,
    pub path: String,
    pub module_specifier: String,
    pub imported_name: Option<String>,
    pub local_name: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFactRecord {
    pub symbol_id: SymbolId,
    pub path: String,
    pub local_name: Option<String>,
    pub exported_name: String,
    pub module_specifier: Option<String>,
    pub is_default: bool,
    pub is_reexport: bool,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedFileFacts {
    pub artifact: FileParseArtifactMetadata,
    pub facts: ProtocolFileFacts,
    pub symbol_facts: Vec<SymbolFactRecord>,
    pub import_facts: Vec<ImportFactRecord>,
    pub export_facts: Vec<ExportFactRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FactsBatch {
    pub files: Vec<ExtractedFileFacts>,
}

impl FactsBatch {
    pub fn symbol_count(&self) -> usize {
        self.files.iter().map(|file| file.facts.symbols.len()).sum()
    }

    pub fn occurrence_count(&self) -> usize {
        self.files
            .iter()
            .map(|file| file.facts.occurrences.len())
            .sum()
    }

    pub fn edge_count(&self) -> usize {
        self.files.iter().map(|file| file.facts.edges.len()).sum()
    }

    pub fn diagnostic_count(&self) -> usize {
        self.files
            .iter()
            .map(|file| file.facts.diagnostics.len())
            .sum()
    }

    pub fn rebind_snapshot(&self, snapshot_id: &str) -> Self {
        Self {
            files: self
                .files
                .iter()
                .map(|file| file.rebind_snapshot(snapshot_id))
                .collect(),
        }
    }
}

impl ExtractedFileFacts {
    pub fn rebind_snapshot(&self, snapshot_id: &str) -> Self {
        let occurrences = self
            .facts
            .occurrences
            .iter()
            .map(|occurrence| rebound_occurrence(snapshot_id, occurrence))
            .collect::<Vec<_>>();
        let occurrence_ids = self
            .facts
            .occurrences
            .iter()
            .zip(occurrences.iter())
            .map(|(old, new)| (old.occurrence_id.0.clone(), new.occurrence_id.clone()))
            .collect::<BTreeMap<_, _>>();
        let edges = self
            .facts
            .edges
            .iter()
            .map(|edge| rebound_edge(edge, &occurrence_ids))
            .collect();

        Self {
            artifact: self.artifact.clone(),
            facts: ProtocolFileFacts {
                path: self.facts.path.clone(),
                language: self.facts.language.clone(),
                symbols: self.facts.symbols.clone(),
                occurrences,
                edges,
                diagnostics: self.facts.diagnostics.clone(),
            },
            symbol_facts: self.symbol_facts.clone(),
            import_facts: self.import_facts.clone(),
            export_facts: self.export_facts.clone(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct FactWorkspace;

impl FactWorkspace {
    pub fn extract(
        &self,
        repo_id: &str,
        snapshot_id: &str,
        artifacts: &[ParseArtifact],
    ) -> FactsBatch {
        let files = artifacts
            .iter()
            .map(|artifact| FileExtractor::new(repo_id, snapshot_id, artifact).extract())
            .collect();
        FactsBatch { files }
    }
}

#[derive(Debug, Clone)]
struct ContainerFrame {
    symbol_id: SymbolId,
    qualified_name: String,
    kind: SymbolKind,
}

#[derive(Debug, Clone, Copy, Default)]
struct ExportContext {
    exported: bool,
    default_export: bool,
}

#[derive(Debug, Clone)]
struct PendingExport {
    local_name: String,
    exported_name: String,
    module_specifier: Option<String>,
    is_default: bool,
    span: SourceSpan,
}

#[derive(Debug, Clone)]
struct ImportBinding {
    imported_name: Option<String>,
    local_name: String,
    span: SourceSpan,
}

#[derive(Debug, Clone)]
struct ExportBinding {
    local_name: Option<String>,
    exported_name: String,
    span: SourceSpan,
}

struct FileExtractor<'a> {
    repo_id: &'a str,
    snapshot_id: &'a str,
    artifact: &'a ParseArtifact,
    diagnostics: Vec<ParseDiagnostic>,
    symbols: Vec<SymbolRecord>,
    symbol_facts: Vec<SymbolFactRecord>,
    occurrences: Vec<SymbolOccurrence>,
    edges: Vec<GraphEdge>,
    import_facts: Vec<ImportFactRecord>,
    export_facts: Vec<ExportFactRecord>,
    pending_exports: Vec<PendingExport>,
    top_level_symbols_by_name: BTreeMap<String, Vec<SymbolId>>,
    seen_symbol_ids: BTreeSet<String>,
    seen_occurrence_ids: BTreeSet<String>,
    seen_edge_ids: BTreeSet<String>,
    module_container: Option<ContainerFrame>,
}

impl<'a> FileExtractor<'a> {
    fn new(repo_id: &'a str, snapshot_id: &'a str, artifact: &'a ParseArtifact) -> Self {
        Self {
            repo_id,
            snapshot_id,
            artifact,
            diagnostics: artifact.metadata().diagnostics.clone(),
            symbols: Vec::new(),
            symbol_facts: Vec::new(),
            occurrences: Vec::new(),
            edges: Vec::new(),
            import_facts: Vec::new(),
            export_facts: Vec::new(),
            pending_exports: Vec::new(),
            top_level_symbols_by_name: BTreeMap::new(),
            seen_symbol_ids: BTreeSet::new(),
            seen_occurrence_ids: BTreeSet::new(),
            seen_edge_ids: BTreeSet::new(),
            module_container: None,
        }
    }

    fn extract(mut self) -> ExtractedFileFacts {
        debug!(
            path = %self.artifact.candidate().path,
            "extracting phase4 file and symbol facts"
        );
        self.seed_module_symbol();
        let root = self.artifact.syntax().root_node();
        let containers = self
            .module_container
            .clone()
            .into_iter()
            .collect::<Vec<_>>();
        self.walk(root, &containers, ExportContext::default());
        if !self.artifact.metadata().diagnostics.is_empty() {
            self.recover_broken_export_bindings_from_source(&containers);
        }
        self.finalize_pending_exports();
        self.collect_reference_occurrences();

        let facts = ProtocolFileFacts {
            path: self.artifact.candidate().path.clone(),
            language: self.artifact.metadata().language.clone(),
            symbols: self.symbols.clone(),
            occurrences: self.occurrences.clone(),
            edges: self.edges.clone(),
            diagnostics: self.diagnostics.clone(),
        };
        let artifact = FileParseArtifactMetadata {
            artifact_id: self.artifact.metadata().artifact_id.clone(),
            path: self.artifact.metadata().path.clone(),
            language: self.artifact.metadata().language.clone(),
            source_kind: self.artifact.metadata().source_kind.clone(),
            stage: ParseArtifactStage::FactsExtracted,
            content_sha256: self.artifact.metadata().content_sha256.clone(),
            content_bytes: self.artifact.metadata().content_bytes,
            parser_pack_id: self.artifact.metadata().parser_pack_id.clone(),
            facts: FileFactsSummary {
                symbol_count: facts.symbols.len() as u64,
                occurrence_count: facts.occurrences.len() as u64,
                edge_count: facts.edges.len() as u64,
            },
            diagnostics: facts.diagnostics.clone(),
        };

        ExtractedFileFacts {
            artifact,
            facts,
            symbol_facts: self.symbol_facts,
            import_facts: self.import_facts,
            export_facts: self.export_facts,
        }
    }

    fn seed_module_symbol(&mut self) {
        let root_span = self.span_for(self.artifact.syntax().root_node());
        let path = self.artifact.candidate().path.clone();
        let symbol = self.push_symbol(
            SymbolKind::Module,
            path.clone(),
            root_span.clone(),
            root_span,
            None,
            SymbolVisibility::Local,
            format!(
                "module:{}:{}",
                path,
                language_name(&self.artifact.metadata().language)
            ),
        );
        self.module_container = Some(ContainerFrame {
            symbol_id: symbol.symbol_id.clone(),
            qualified_name: symbol
                .qualified_name
                .clone()
                .unwrap_or_else(|| path.clone()),
            kind: SymbolKind::Module,
        });
        self.push_define_occurrence(
            &symbol.symbol_id,
            self.span_for(self.artifact.syntax().root_node()),
            OccurrenceRole::Definition,
        );
    }

    fn walk(&mut self, node: Node<'_>, containers: &[ContainerFrame], export: ExportContext) {
        match node.kind() {
            "program" | "statement_block" | "class_body" | "interface_body" => {
                for child in children(node) {
                    self.walk(child, containers, export);
                }
            }
            "import_statement" => self.handle_import(node, containers),
            "export_statement" => self.handle_export_statement(node, containers),
            "function_declaration" => self.handle_function_declaration(node, containers, export),
            "class_declaration" => self.handle_class_declaration(node, containers, export),
            "interface_declaration" => {
                self.handle_named_declaration(node, containers, export, SymbolKind::Interface)
            }
            "type_alias_declaration" => {
                self.handle_named_declaration(node, containers, export, SymbolKind::TypeAlias)
            }
            "enum_declaration" => self.handle_enum_declaration(node, containers, export),
            "method_definition" | "method_signature" => self.handle_method_like(node, containers),
            "lexical_declaration" | "variable_declaration" => {
                self.handle_variable_declaration(node, containers, export)
            }
            _ => {
                for child in children(node) {
                    self.walk(child, containers, export);
                }
            }
        }
    }

    fn handle_import(&mut self, node: Node<'_>, containers: &[ContainerFrame]) {
        let Some(module_specifier) = string_child_text(self.contents(), node) else {
            self.push_diagnostic(
                self.span_for(node),
                ParseDiagnosticCode::PartialAnalysis,
                "skipped import without a recoverable module specifier".to_string(),
            );
            return;
        };

        for binding in self.collect_import_bindings(node) {
            let symbol = self.push_symbol(
                SymbolKind::ImportBinding,
                binding.local_name.clone(),
                binding.span.clone(),
                binding.span.clone(),
                containers.last(),
                SymbolVisibility::Local,
                format!(
                    "import:{}:{}:{}",
                    module_specifier,
                    binding
                        .imported_name
                        .clone()
                        .unwrap_or_else(|| "*".to_string()),
                    binding.local_name
                ),
            );
            self.push_define_occurrence(
                &symbol.symbol_id,
                binding.span.clone(),
                OccurrenceRole::Import,
            );
            self.import_facts.push(ImportFactRecord {
                symbol_id: symbol.symbol_id.clone(),
                path: self.artifact.candidate().path.clone(),
                module_specifier: module_specifier.clone(),
                imported_name: binding.imported_name,
                local_name: binding.local_name,
                span: binding.span,
            });
        }
    }

    fn handle_export_statement(&mut self, node: Node<'_>, containers: &[ContainerFrame]) {
        let default_export = node_text(self.contents(), node).contains("export default");
        let module_specifier = string_child_text(self.contents(), node);

        if let Some(child) = children(node).into_iter().find(|child| {
            matches!(
                child.kind(),
                "function_declaration"
                    | "function"
                    | "class_declaration"
                    | "class"
                    | "interface_declaration"
                    | "type_alias_declaration"
                    | "enum_declaration"
                    | "lexical_declaration"
                    | "variable_declaration"
            )
        }) {
            let export_context = ExportContext {
                exported: true,
                default_export,
            };
            match child.kind() {
                "function" | "function_declaration" => {
                    self.handle_function_declaration(child, containers, export_context)
                }
                "class" | "class_declaration" => {
                    self.handle_class_declaration(child, containers, export_context)
                }
                _ => self.walk(child, containers, export_context),
            }
            return;
        }

        if let Some(clause) = children(node)
            .into_iter()
            .find(|child| child.kind() == "export_clause")
        {
            for binding in self.collect_export_bindings(clause) {
                self.pending_exports.push(PendingExport {
                    local_name: binding
                        .local_name
                        .clone()
                        .unwrap_or_else(|| binding.exported_name.clone()),
                    exported_name: binding.exported_name,
                    module_specifier: module_specifier.clone(),
                    is_default: default_export,
                    span: binding.span,
                });
            }
            return;
        }

        if default_export {
            if let Some(identifier) = children(node)
                .into_iter()
                .find(|child| matches!(child.kind(), "identifier" | "property_identifier"))
            {
                self.pending_exports.push(PendingExport {
                    local_name: node_text(self.contents(), identifier),
                    exported_name: "default".to_string(),
                    module_specifier: None,
                    is_default: true,
                    span: self.span_for(identifier),
                });
                return;
            }

            if self.recover_default_export_from_text(node, containers) {
                return;
            }

            self.push_diagnostic(
                self.span_for(node),
                ParseDiagnosticCode::UnsupportedSyntax,
                "skipped default export expression without a declaration anchor".to_string(),
            );
            return;
        }

        if self.recover_exported_binding_from_text(node, containers) {
            return;
        }

        if node_text(self.contents(), node).contains("export *") {
            self.push_diagnostic(
                self.span_for(node),
                ParseDiagnosticCode::UnsupportedSyntax,
                "wildcard re-exports are not indexed in this Phase 4 slice".to_string(),
            );
        }
    }

    fn handle_function_declaration(
        &mut self,
        node: Node<'_>,
        containers: &[ContainerFrame],
        export: ExportContext,
    ) {
        let span = self.span_for(node);
        let name_node = child_by_field_name_or_kind(node, "name", &["identifier"]);
        let (name, anchor_span) = match name_node {
            Some(name_node) => (
                node_text(self.contents(), name_node),
                self.span_for(name_node),
            ),
            None if export.default_export => ("default".to_string(), span.clone()),
            None => {
                self.push_diagnostic(
                    span,
                    ParseDiagnosticCode::UnsupportedSyntax,
                    "skipped anonymous function declaration outside a default export".to_string(),
                );
                return;
            }
        };

        let symbol = self.push_symbol(
            SymbolKind::Function,
            name.clone(),
            span,
            anchor_span.clone(),
            containers.last(),
            visibility_for(export),
            format!("fn:{}:{}", name, self.function_signature(node)),
        );
        self.push_define_occurrence(
            &symbol.symbol_id,
            anchor_span.clone(),
            OccurrenceRole::Definition,
        );
        if export.exported {
            self.record_export(
                &symbol.symbol_id,
                Some(name.as_str()),
                if export.default_export {
                    "default"
                } else {
                    name.as_str()
                },
                None,
                export.default_export,
                false,
                anchor_span,
            );
        }
        if let Some(body) = node.child_by_field_name("body") {
            let mut next = containers.to_vec();
            next.push(self.container_frame_for(&symbol));
            self.walk(body, &next, ExportContext::default());
        }
    }

    fn handle_class_declaration(
        &mut self,
        node: Node<'_>,
        containers: &[ContainerFrame],
        export: ExportContext,
    ) {
        let span = self.span_for(node);
        let name_node =
            child_by_field_name_or_kind(node, "name", &["type_identifier", "identifier"]);
        let (name, anchor_span) = match name_node {
            Some(name_node) => (
                node_text(self.contents(), name_node),
                self.span_for(name_node),
            ),
            None if export.default_export => ("default".to_string(), span.clone()),
            None => {
                self.push_diagnostic(
                    span,
                    ParseDiagnosticCode::UnsupportedSyntax,
                    "skipped anonymous class outside a default export".to_string(),
                );
                return;
            }
        };

        let symbol = self.push_symbol(
            SymbolKind::Class,
            name.clone(),
            span,
            anchor_span.clone(),
            containers.last(),
            visibility_for(export),
            format!("class:{}:{}", name, self.class_signature(node)),
        );
        self.push_define_occurrence(
            &symbol.symbol_id,
            anchor_span.clone(),
            OccurrenceRole::Definition,
        );
        if export.exported {
            self.record_export(
                &symbol.symbol_id,
                Some(name.as_str()),
                if export.default_export {
                    "default"
                } else {
                    name.as_str()
                },
                None,
                export.default_export,
                false,
                anchor_span,
            );
        }
        if let Some(body) = node.child_by_field_name("body") {
            let mut next = containers.to_vec();
            next.push(self.container_frame_for(&symbol));
            for child in named_children(body) {
                self.walk(child, &next, ExportContext::default());
            }
        }
    }

    fn handle_named_declaration(
        &mut self,
        node: Node<'_>,
        containers: &[ContainerFrame],
        export: ExportContext,
        kind: SymbolKind,
    ) {
        let span = self.span_for(node);
        let Some(name_node) =
            child_by_field_name_or_kind(node, "name", &["type_identifier", "identifier"])
        else {
            self.push_diagnostic(
                span,
                ParseDiagnosticCode::UnsupportedSyntax,
                format!("skipped {:?} without a recoverable name", kind),
            );
            return;
        };
        let name = node_text(self.contents(), name_node);
        let anchor_span = self.span_for(name_node);
        let symbol = self.push_symbol(
            kind.clone(),
            name.clone(),
            span,
            anchor_span.clone(),
            containers.last(),
            visibility_for(export),
            format!(
                "decl:{}:{}",
                name,
                normalize_ws(&node_text(self.contents(), node))
            ),
        );
        self.push_define_occurrence(
            &symbol.symbol_id,
            anchor_span.clone(),
            OccurrenceRole::Definition,
        );
        if export.exported {
            self.record_export(
                &symbol.symbol_id,
                Some(name.as_str()),
                if export.default_export {
                    "default"
                } else {
                    name.as_str()
                },
                None,
                export.default_export,
                false,
                anchor_span,
            );
        }
        if kind == SymbolKind::Interface {
            let mut next = containers.to_vec();
            next.push(self.container_frame_for(&symbol));
            for child in named_children(node) {
                if child.kind() == "interface_body" {
                    self.walk(child, &next, ExportContext::default());
                }
            }
        }
    }

    fn handle_enum_declaration(
        &mut self,
        node: Node<'_>,
        containers: &[ContainerFrame],
        export: ExportContext,
    ) {
        let span = self.span_for(node);
        let Some(name_node) =
            child_by_field_name_or_kind(node, "name", &["identifier", "type_identifier"])
        else {
            self.push_diagnostic(
                span,
                ParseDiagnosticCode::UnsupportedSyntax,
                "skipped enum without a recoverable name".to_string(),
            );
            return;
        };
        let name = node_text(self.contents(), name_node);
        let anchor_span = self.span_for(name_node);
        let symbol = self.push_symbol(
            SymbolKind::Enum,
            name.clone(),
            span,
            anchor_span.clone(),
            containers.last(),
            visibility_for(export),
            format!(
                "enum:{}:{}",
                name,
                normalize_ws(&node_text(self.contents(), node))
            ),
        );
        self.push_define_occurrence(
            &symbol.symbol_id,
            anchor_span.clone(),
            OccurrenceRole::Definition,
        );
        if export.exported {
            self.record_export(
                &symbol.symbol_id,
                Some(name.as_str()),
                if export.default_export {
                    "default"
                } else {
                    name.as_str()
                },
                None,
                export.default_export,
                false,
                anchor_span,
            );
        }
    }

    fn handle_method_like(&mut self, node: Node<'_>, containers: &[ContainerFrame]) {
        let Some(container) = containers.last() else {
            return;
        };
        if !matches!(container.kind, SymbolKind::Class | SymbolKind::Interface) {
            return;
        }

        let span = self.span_for(node);
        let Some(name_node) = child_by_field_name_or_kind(
            node,
            "name",
            &[
                "property_identifier",
                "private_property_identifier",
                "identifier",
            ],
        ) else {
            self.push_diagnostic(
                span,
                ParseDiagnosticCode::UnsupportedSyntax,
                "skipped computed or anonymous method".to_string(),
            );
            return;
        };
        let name = node_text(self.contents(), name_node);
        let anchor_span = self.span_for(name_node);
        let kind = if name == "constructor" {
            SymbolKind::Constructor
        } else {
            SymbolKind::Method
        };
        let symbol = self.push_symbol(
            kind,
            name.clone(),
            span,
            anchor_span.clone(),
            Some(container),
            SymbolVisibility::Local,
            format!("method:{}:{}", name, self.function_signature(node)),
        );
        self.push_define_occurrence(&symbol.symbol_id, anchor_span, OccurrenceRole::Definition);
        if let Some(body) = node.child_by_field_name("body") {
            let mut next = containers.to_vec();
            next.push(self.container_frame_for(&symbol));
            self.walk(body, &next, ExportContext::default());
        }
    }

    fn handle_variable_declaration(
        &mut self,
        node: Node<'_>,
        containers: &[ContainerFrame],
        export: ExportContext,
    ) {
        for declarator in children(node)
            .into_iter()
            .filter(|child| child.kind() == "variable_declarator")
        {
            let Some(name_node) = declarator.child_by_field_name("name") else {
                continue;
            };
            if !matches!(name_node.kind(), "identifier" | "property_identifier") {
                self.push_diagnostic(
                    self.span_for(declarator),
                    ParseDiagnosticCode::UnsupportedSyntax,
                    "skipped destructuring declaration during symbol extraction".to_string(),
                );
                continue;
            }
            let Some(value_node) = declarator.child_by_field_name("value").or_else(|| {
                children(declarator)
                    .into_iter()
                    .find(|child| matches!(child.kind(), "arrow_function" | "function_expression"))
            }) else {
                continue;
            };
            if !matches!(value_node.kind(), "arrow_function" | "function_expression") {
                continue;
            }
            let name = node_text(self.contents(), name_node);
            let span = self.span_for(declarator);
            let anchor_span = self.span_for(name_node);
            let symbol = self.push_symbol(
                SymbolKind::Function,
                name.clone(),
                span,
                anchor_span.clone(),
                containers.last(),
                visibility_for(export),
                format!("var_fn:{}:{}", name, self.function_signature(value_node)),
            );
            self.push_define_occurrence(
                &symbol.symbol_id,
                anchor_span.clone(),
                OccurrenceRole::Definition,
            );
            if export.exported {
                self.record_export(
                    &symbol.symbol_id,
                    Some(name.as_str()),
                    if export.default_export {
                        "default"
                    } else {
                        name.as_str()
                    },
                    None,
                    export.default_export,
                    false,
                    anchor_span,
                );
            }
            if let Some(body) = value_node.child_by_field_name("body") {
                let mut next = containers.to_vec();
                next.push(self.container_frame_for(&symbol));
                self.walk(body, &next, ExportContext::default());
            }
        }
    }

    fn finalize_pending_exports(&mut self) {
        for pending in self.pending_exports.clone() {
            if let Some(module_specifier) = pending.module_specifier.clone() {
                let module_container = self.module_container.clone();
                let symbol = self.push_symbol(
                    SymbolKind::ImportBinding,
                    pending.exported_name.clone(),
                    pending.span.clone(),
                    pending.span.clone(),
                    module_container.as_ref(),
                    if pending.is_default {
                        SymbolVisibility::DefaultExport
                    } else {
                        SymbolVisibility::Exported
                    },
                    format!(
                        "reexport:{}:{}:{}",
                        module_specifier, pending.local_name, pending.exported_name
                    ),
                );
                self.push_occurrence_at_span(
                    &symbol.symbol_id,
                    pending.span.clone(),
                    OccurrenceRole::Export,
                );
                self.record_export(
                    &symbol.symbol_id,
                    Some(pending.local_name.as_str()),
                    &pending.exported_name,
                    Some(module_specifier),
                    pending.is_default,
                    true,
                    pending.span,
                );
                continue;
            }

            let Some(candidates) = self.top_level_symbols_by_name.get(&pending.local_name) else {
                self.push_diagnostic(
                    pending.span,
                    ParseDiagnosticCode::PartialAnalysis,
                    format!(
                        "skipped export for unresolved local symbol `{}`",
                        pending.local_name
                    ),
                );
                continue;
            };
            if candidates.len() != 1 {
                self.push_diagnostic(
                    pending.span,
                    ParseDiagnosticCode::DuplicateFact,
                    format!(
                        "skipped export for ambiguous local symbol `{}`",
                        pending.local_name
                    ),
                );
                continue;
            }
            let symbol_id = candidates[0].clone();
            self.push_occurrence_at_span(&symbol_id, pending.span.clone(), OccurrenceRole::Export);
            self.push_edge(
                GraphEdgeKind::Exports,
                GraphNodeRef::File {
                    path: self.artifact.candidate().path.clone(),
                },
                GraphNodeRef::Symbol {
                    symbol_id: symbol_id.clone(),
                },
            );
            self.export_facts.push(ExportFactRecord {
                symbol_id: symbol_id.clone(),
                path: self.artifact.candidate().path.clone(),
                local_name: Some(pending.local_name.clone()),
                exported_name: pending.exported_name.clone(),
                module_specifier: None,
                is_default: pending.is_default,
                is_reexport: false,
                span: pending.span,
            });
            self.promote_visibility(&symbol_id, pending.is_default);
        }
    }

    fn recover_default_export_from_text(
        &mut self,
        node: Node<'_>,
        containers: &[ContainerFrame],
    ) -> bool {
        let text = normalize_ws(&node_text(self.contents(), node));
        let span = self.span_for(node);
        let kind = if text.starts_with("export default function")
            || text.starts_with("export default async function")
        {
            Some(SymbolKind::Function)
        } else if text.starts_with("export default class") {
            Some(SymbolKind::Class)
        } else {
            None
        };
        let Some(kind) = kind else {
            return false;
        };

        let signature = match kind {
            SymbolKind::Function => format!("fn:default:{}", text),
            SymbolKind::Class => format!("class:default:{}", text),
            _ => text.clone(),
        };
        let symbol = self.push_symbol(
            kind,
            "default".to_string(),
            span.clone(),
            span.clone(),
            containers.last(),
            SymbolVisibility::DefaultExport,
            signature,
        );
        self.push_define_occurrence(&symbol.symbol_id, span.clone(), OccurrenceRole::Definition);
        self.record_export(
            &symbol.symbol_id,
            Some("default"),
            "default",
            None,
            true,
            false,
            span,
        );
        true
    }

    fn recover_exported_binding_from_text(
        &mut self,
        node: Node<'_>,
        containers: &[ContainerFrame],
    ) -> bool {
        let text = normalize_ws(&node_text(self.contents(), node));
        let prefixes = ["export const ", "export let ", "export var "];
        let Some(prefix) = prefixes.into_iter().find(|prefix| text.starts_with(prefix)) else {
            return false;
        };
        if !(text.contains("=>") || text.contains("function")) {
            return false;
        }
        let Some(name) = identifier_after_prefix(&text, prefix) else {
            return false;
        };
        let span = self.span_for(node);
        let symbol = self.push_symbol(
            SymbolKind::Function,
            name.clone(),
            span.clone(),
            span.clone(),
            containers.last(),
            SymbolVisibility::Exported,
            format!("var_fn:{}:{}", name, text),
        );
        self.push_define_occurrence(&symbol.symbol_id, span.clone(), OccurrenceRole::Definition);
        self.record_export(
            &symbol.symbol_id,
            Some(name.as_str()),
            &name,
            None,
            false,
            false,
            span,
        );
        true
    }

    fn recover_broken_export_bindings_from_source(&mut self, containers: &[ContainerFrame]) {
        for prefix in ["export const ", "export let ", "export var "] {
            let mut search_from = 0usize;
            while let Some(relative_start) = self.artifact.contents()[search_from..].find(prefix) {
                let start = search_from + relative_start;
                let identifier_start = start + prefix.len();
                let remainder = &self.artifact.contents()[identifier_start..];
                let identifier = remainder
                    .chars()
                    .take_while(|char| char.is_ascii_alphanumeric() || *char == '_' || *char == '$')
                    .collect::<String>();
                if identifier.is_empty() {
                    search_from = identifier_start;
                    continue;
                }
                if self.top_level_symbols_by_name.contains_key(&identifier) {
                    search_from = identifier_start + identifier.len();
                    continue;
                }

                let statement_end = find_statement_end(self.artifact.contents(), identifier_start);
                let statement_text = &self.artifact.contents()[start..statement_end];
                if !(statement_text.contains("=>") || statement_text.contains("function")) {
                    search_from = statement_end;
                    continue;
                }

                let name_span = self
                    .artifact
                    .line_index()
                    .byte_range_to_span(identifier_start, identifier_start + identifier.len());
                let declaration_span = self
                    .artifact
                    .line_index()
                    .byte_range_to_span(start, statement_end);
                let symbol = self.push_symbol(
                    SymbolKind::Function,
                    identifier.clone(),
                    declaration_span,
                    name_span.clone(),
                    containers.last(),
                    SymbolVisibility::Exported,
                    format!("var_fn:{}:{}", identifier, normalize_ws(statement_text)),
                );
                self.push_define_occurrence(
                    &symbol.symbol_id,
                    name_span.clone(),
                    OccurrenceRole::Definition,
                );
                self.record_export(
                    &symbol.symbol_id,
                    Some(identifier.as_str()),
                    &identifier,
                    None,
                    false,
                    false,
                    name_span,
                );
                search_from = statement_end;
            }
        }
    }

    fn collect_reference_occurrences(&mut self) {
        let root = self.artifact.syntax().root_node();
        let containers = self
            .module_container
            .clone()
            .into_iter()
            .collect::<Vec<_>>();
        self.walk_references(root, &containers);
    }

    fn walk_references(&mut self, node: Node<'_>, containers: &[ContainerFrame]) {
        match node.kind() {
            "program" | "statement_block" | "class_body" | "interface_body" => {
                for child in children(node) {
                    self.walk_references(child, containers);
                }
            }
            "import_statement" => {}
            "export_statement" => {
                if let Some(child) = children(node).into_iter().find(|child| {
                    matches!(
                        child.kind(),
                        "function_declaration"
                            | "function"
                            | "class_declaration"
                            | "class"
                            | "interface_declaration"
                            | "type_alias_declaration"
                            | "enum_declaration"
                            | "lexical_declaration"
                            | "variable_declaration"
                    )
                }) {
                    self.walk_references(child, containers);
                }
            }
            "function_declaration" | "function" => {
                self.walk_function_like_references(node, containers, SymbolKind::Function)
            }
            "class_declaration" | "class" => {
                self.walk_function_like_references(node, containers, SymbolKind::Class)
            }
            "interface_declaration" => {
                self.walk_function_like_references(node, containers, SymbolKind::Interface)
            }
            "type_alias_declaration" => {
                self.walk_function_like_references(node, containers, SymbolKind::TypeAlias)
            }
            "enum_declaration" => {
                self.walk_function_like_references(node, containers, SymbolKind::Enum)
            }
            "method_definition" | "method_signature" => {
                self.walk_method_references(node, containers)
            }
            "lexical_declaration" | "variable_declaration" => {
                self.walk_variable_declaration_references(node, containers)
            }
            kind if matches!(kind, "identifier" | "type_identifier") => {
                if is_reference_candidate(node) {
                    let name = node_text(self.contents(), node);
                    let span = self.span_for(node);
                    if let Some(symbol_id) =
                        self.resolve_reference_symbol(&name, node.kind(), containers, &span)
                    {
                        self.push_reference_occurrence(&symbol_id, span);
                    }
                }
            }
            _ => {
                for child in children(node) {
                    self.walk_references(child, containers);
                }
            }
        }
    }

    fn walk_function_like_references(
        &mut self,
        node: Node<'_>,
        containers: &[ContainerFrame],
        kind: SymbolKind,
    ) {
        let body = node.child_by_field_name("body");
        let name_node = child_by_field_name_or_kind(
            node,
            "name",
            &[
                "identifier",
                "property_identifier",
                "private_property_identifier",
                "type_identifier",
            ],
        );
        let display_name = name_node
            .map(|name| node_text(self.contents(), name))
            .or_else(|| {
                node_text(self.contents(), node)
                    .contains("export default")
                    .then_some("default".to_string())
            });

        for child in children(node) {
            if name_node
                .as_ref()
                .map(|name| name.id() == child.id())
                .unwrap_or(false)
            {
                continue;
            }
            if body
                .as_ref()
                .map(|body| body.id() == child.id())
                .unwrap_or(false)
            {
                continue;
            }
            self.walk_references(child, containers);
        }

        let Some(body) = body else {
            return;
        };
        let Some(display_name) = display_name else {
            return;
        };
        let Some(symbol) = self.find_owned_symbol(
            containers.last(),
            &display_name,
            &kind,
            node.start_byte() as u32,
        ) else {
            return;
        };
        let mut next = containers.to_vec();
        next.push(self.container_frame_for(&symbol));
        self.walk_references(body, &next);
    }

    fn walk_method_references(&mut self, node: Node<'_>, containers: &[ContainerFrame]) {
        let Some(container) = containers.last() else {
            return;
        };
        let body = node.child_by_field_name("body");
        let Some(name_node) = child_by_field_name_or_kind(
            node,
            "name",
            &[
                "property_identifier",
                "private_property_identifier",
                "identifier",
            ],
        ) else {
            for child in children(node) {
                self.walk_references(child, containers);
            }
            return;
        };
        let display_name = node_text(self.contents(), name_node);
        for child in children(node) {
            if child.id() == name_node.id()
                || body
                    .as_ref()
                    .map(|body| body.id() == child.id())
                    .unwrap_or(false)
            {
                continue;
            }
            self.walk_references(child, containers);
        }

        let Some(body) = body else {
            return;
        };
        let kind = if display_name == "constructor" {
            SymbolKind::Constructor
        } else {
            SymbolKind::Method
        };
        let Some(symbol) = self.find_owned_symbol(
            Some(container),
            &display_name,
            &kind,
            node.start_byte() as u32,
        ) else {
            return;
        };
        let mut next = containers.to_vec();
        next.push(self.container_frame_for(&symbol));
        self.walk_references(body, &next);
    }

    fn walk_variable_declaration_references(
        &mut self,
        node: Node<'_>,
        containers: &[ContainerFrame],
    ) {
        for declarator in children(node)
            .into_iter()
            .filter(|child| child.kind() == "variable_declarator")
        {
            let Some(value_node) = declarator.child_by_field_name("value") else {
                continue;
            };
            let name_node = declarator.child_by_field_name("name");
            if matches!(value_node.kind(), "arrow_function" | "function_expression") {
                for child in children(value_node) {
                    if value_node
                        .child_by_field_name("body")
                        .as_ref()
                        .map(|body| body.id() == child.id())
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    self.walk_references(child, containers);
                }
                let Some(name_node) = name_node else {
                    continue;
                };
                let display_name = node_text(self.contents(), name_node);
                let Some(body) = value_node.child_by_field_name("body") else {
                    continue;
                };
                let Some(symbol) = self.find_owned_symbol(
                    containers.last(),
                    &display_name,
                    &SymbolKind::Function,
                    declarator.start_byte() as u32,
                ) else {
                    continue;
                };
                let mut next = containers.to_vec();
                next.push(self.container_frame_for(&symbol));
                self.walk_references(body, &next);
                continue;
            }
            self.walk_references(value_node, containers);
        }
    }

    fn resolve_reference_symbol(
        &self,
        name: &str,
        node_kind: &str,
        containers: &[ContainerFrame],
        span: &SourceSpan,
    ) -> Option<SymbolId> {
        for container in containers.iter().rev() {
            if let Some(symbol) = self.lookup_symbol_by_id(&container.symbol_id) {
                if symbol.display_name == name
                    && reference_matches_kind(node_kind, &symbol.kind)
                    && symbol.span.bytes.start <= span.bytes.start
                {
                    return Some(symbol.symbol_id.clone());
                }
            }

            let mut candidates = self
                .symbol_facts
                .iter()
                .filter(|record| {
                    record.container.as_ref() == Some(&container.symbol_id)
                        && record.symbol.display_name == name
                        && reference_matches_kind(node_kind, &record.symbol.kind)
                        && record.symbol.span.bytes.start <= span.bytes.start
                })
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| {
                right
                    .symbol
                    .span
                    .bytes
                    .start
                    .cmp(&left.symbol.span.bytes.start)
                    .then_with(|| left.symbol.symbol_id.0.cmp(&right.symbol.symbol_id.0))
            });
            if let Some(candidate) = candidates.first() {
                return Some(candidate.symbol.symbol_id.clone());
            }
        }
        None
    }

    fn find_owned_symbol(
        &self,
        container: Option<&ContainerFrame>,
        display_name: &str,
        kind: &SymbolKind,
        declaration_start: u32,
    ) -> Option<SymbolRecord> {
        self.symbol_facts
            .iter()
            .find(|record| {
                record.container.as_ref() == container.map(|value| &value.symbol_id)
                    && record.symbol.display_name == display_name
                    && &record.symbol.kind == kind
                    && record.symbol.span.bytes.start == declaration_start
            })
            .map(|record| record.symbol.clone())
    }

    fn lookup_symbol_by_id(&self, symbol_id: &SymbolId) -> Option<&SymbolRecord> {
        self.symbol_facts
            .iter()
            .find(|record| record.symbol.symbol_id == *symbol_id)
            .map(|record| &record.symbol)
    }

    fn collect_import_bindings(&self, node: Node<'_>) -> Vec<ImportBinding> {
        let mut bindings = Vec::new();
        let Some(clause) = named_children(node)
            .into_iter()
            .find(|child| child.kind() == "import_clause")
        else {
            return bindings;
        };

        for child in named_children(clause) {
            match child.kind() {
                "identifier" => bindings.push(ImportBinding {
                    imported_name: Some("default".to_string()),
                    local_name: node_text(self.contents(), child),
                    span: self.span_for(child),
                }),
                "namespace_import" => {
                    if let Some(alias) = child_by_field_name_or_kind(child, "name", &["identifier"])
                    {
                        bindings.push(ImportBinding {
                            imported_name: Some("*".to_string()),
                            local_name: node_text(self.contents(), alias),
                            span: self.span_for(alias),
                        });
                    }
                }
                "named_imports" => {
                    for specifier in named_children(child)
                        .into_iter()
                        .filter(|specifier| specifier.kind() == "import_specifier")
                    {
                        let names = named_children(specifier)
                            .into_iter()
                            .filter(|candidate| {
                                matches!(
                                    candidate.kind(),
                                    "identifier" | "property_identifier" | "type_identifier"
                                )
                            })
                            .collect::<Vec<_>>();
                        if names.is_empty() {
                            continue;
                        }
                        let imported = child_by_field_name_or_kind(
                            specifier,
                            "name",
                            &["identifier", "property_identifier", "type_identifier"],
                        )
                        .map(|node| node_text(self.contents(), node))
                        .unwrap_or_else(|| node_text(self.contents(), names[0]));
                        let alias_node = child_by_field_name_or_kind(
                            specifier,
                            "alias",
                            &["identifier", "property_identifier"],
                        );
                        let local_node =
                            alias_node.unwrap_or_else(|| names.last().copied().unwrap());
                        bindings.push(ImportBinding {
                            imported_name: Some(imported),
                            local_name: node_text(self.contents(), local_node),
                            span: self.span_for(local_node),
                        });
                    }
                }
                _ => {}
            }
        }

        bindings
    }

    fn collect_export_bindings(&self, clause: Node<'_>) -> Vec<ExportBinding> {
        named_children(clause)
            .into_iter()
            .filter(|child| child.kind() == "export_specifier")
            .filter_map(|specifier| {
                let names = named_children(specifier)
                    .into_iter()
                    .filter(|candidate| {
                        matches!(
                            candidate.kind(),
                            "identifier" | "property_identifier" | "type_identifier"
                        )
                    })
                    .collect::<Vec<_>>();
                if names.is_empty() {
                    return None;
                }

                let local = child_by_field_name_or_kind(
                    specifier,
                    "name",
                    &["identifier", "property_identifier", "type_identifier"],
                )
                .map(|node| node_text(self.contents(), node));
                let alias = child_by_field_name_or_kind(
                    specifier,
                    "alias",
                    &["identifier", "property_identifier", "type_identifier"],
                );
                let alias_text = alias.map(|node| node_text(self.contents(), node));
                let exported_name = alias_text.clone().unwrap_or_else(|| {
                    local
                        .clone()
                        .unwrap_or_else(|| node_text(self.contents(), names[names.len() - 1]))
                });
                let local_name = local.or_else(|| {
                    if names.len() > 1 {
                        Some(node_text(self.contents(), names[0]))
                    } else {
                        Some(exported_name.clone())
                    }
                });
                let span = alias
                    .map(|node| self.span_for(node))
                    .unwrap_or_else(|| self.span_for(names[0]));
                Some(ExportBinding {
                    local_name,
                    exported_name,
                    span,
                })
            })
            .collect()
    }

    fn push_symbol(
        &mut self,
        kind: SymbolKind,
        display_name: String,
        span: SourceSpan,
        anchor_span: SourceSpan,
        container: Option<&ContainerFrame>,
        visibility: SymbolVisibility,
        signature_source: String,
    ) -> SymbolRecord {
        let signature_digest = digest_hex(&signature_source);
        let symbol_id = SymbolId(symbol_id(
            self.repo_id,
            &self.artifact.candidate().path,
            &kind,
            container.map(|frame| frame.symbol_id.0.as_str()),
            &display_name,
            &signature_digest,
        ));
        let symbol = SymbolRecord {
            symbol_id: symbol_id.clone(),
            display_name: display_name.clone(),
            qualified_name: Some(qualify_name(container, &display_name)),
            kind: kind.clone(),
            language: self.artifact.metadata().language.clone(),
            path: self.artifact.candidate().path.clone(),
            span,
        };
        if !self.seen_symbol_ids.insert(symbol_id.0.clone()) {
            self.push_diagnostic(
                anchor_span,
                ParseDiagnosticCode::DuplicateFact,
                format!("skipped duplicate symbol id `{}`", symbol_id.0),
            );
            return symbol;
        }
        if container
            .map(|frame| frame.kind == SymbolKind::Module)
            .unwrap_or(false)
        {
            self.top_level_symbols_by_name
                .entry(display_name)
                .or_default()
                .push(symbol_id.clone());
        }
        self.symbols.push(symbol.clone());
        self.symbol_facts.push(SymbolFactRecord {
            symbol: symbol.clone(),
            container: container.map(|frame| frame.symbol_id.clone()),
            visibility,
            file_path: self.artifact.candidate().path.clone(),
            signature_digest,
        });
        let from = match container {
            Some(container) => GraphNodeRef::Symbol {
                symbol_id: container.symbol_id.clone(),
            },
            None => GraphNodeRef::File {
                path: self.artifact.candidate().path.clone(),
            },
        };
        self.push_edge(
            GraphEdgeKind::Contains,
            from,
            GraphNodeRef::Symbol {
                symbol_id: symbol_id.clone(),
            },
        );
        symbol
    }

    fn push_occurrence_at_span(
        &mut self,
        symbol_id: &SymbolId,
        span: SourceSpan,
        role: OccurrenceRole,
    ) -> Option<OccurrenceId> {
        let occurrence_id = OccurrenceId(occurrence_id(
            self.snapshot_id,
            &self.artifact.candidate().path,
            symbol_id,
            &role,
            &span,
        ));
        if !self.seen_occurrence_ids.insert(occurrence_id.0.clone()) {
            return None;
        }
        self.occurrences.push(SymbolOccurrence {
            occurrence_id: occurrence_id.clone(),
            symbol_id: symbol_id.clone(),
            path: self.artifact.candidate().path.clone(),
            span,
            role,
        });
        Some(occurrence_id)
    }

    fn push_define_occurrence(
        &mut self,
        symbol_id: &SymbolId,
        span: SourceSpan,
        role: OccurrenceRole,
    ) {
        if let Some(occurrence_id) = self.push_occurrence_at_span(symbol_id, span, role) {
            self.push_edge(
                GraphEdgeKind::Defines,
                GraphNodeRef::Symbol {
                    symbol_id: symbol_id.clone(),
                },
                GraphNodeRef::Occurrence { occurrence_id },
            );
        }
    }

    fn push_reference_occurrence(&mut self, symbol_id: &SymbolId, span: SourceSpan) {
        if let Some(occurrence_id) =
            self.push_occurrence_at_span(symbol_id, span, OccurrenceRole::Reference)
        {
            self.push_edge(
                GraphEdgeKind::References,
                GraphNodeRef::Occurrence { occurrence_id },
                GraphNodeRef::Symbol {
                    symbol_id: symbol_id.clone(),
                },
            );
        }
    }

    fn push_edge(&mut self, kind: GraphEdgeKind, from: GraphNodeRef, to: GraphNodeRef) {
        let edge_id = edge_id(&kind, &from, &to);
        if !self.seen_edge_ids.insert(edge_id.clone()) {
            return;
        }
        self.edges.push(GraphEdge {
            edge_id,
            kind,
            from,
            to,
        });
    }

    fn record_export(
        &mut self,
        symbol_id: &SymbolId,
        local_name: Option<&str>,
        exported_name: &str,
        module_specifier: Option<String>,
        is_default: bool,
        is_reexport: bool,
        span: SourceSpan,
    ) {
        self.push_occurrence_at_span(symbol_id, span.clone(), OccurrenceRole::Export);
        self.push_edge(
            GraphEdgeKind::Exports,
            GraphNodeRef::File {
                path: self.artifact.candidate().path.clone(),
            },
            GraphNodeRef::Symbol {
                symbol_id: symbol_id.clone(),
            },
        );
        self.export_facts.push(ExportFactRecord {
            symbol_id: symbol_id.clone(),
            path: self.artifact.candidate().path.clone(),
            local_name: local_name.map(ToOwned::to_owned),
            exported_name: exported_name.to_string(),
            module_specifier,
            is_default,
            is_reexport,
            span,
        });
        self.promote_visibility(symbol_id, is_default);
    }

    fn promote_visibility(&mut self, symbol_id: &SymbolId, default_export: bool) {
        if let Some(symbol_fact) = self
            .symbol_facts
            .iter_mut()
            .find(|record| record.symbol.symbol_id == *symbol_id)
        {
            symbol_fact.visibility = if default_export {
                SymbolVisibility::DefaultExport
            } else if symbol_fact.visibility != SymbolVisibility::DefaultExport {
                SymbolVisibility::Exported
            } else {
                SymbolVisibility::DefaultExport
            };
        }
    }

    fn push_diagnostic(&mut self, span: SourceSpan, code: ParseDiagnosticCode, message: String) {
        self.diagnostics.push(ParseDiagnostic {
            severity: match code {
                ParseDiagnosticCode::UnsupportedSyntax | ParseDiagnosticCode::DuplicateFact => {
                    ParseDiagnosticSeverity::Warning
                }
                _ => ParseDiagnosticSeverity::Info,
            },
            code,
            message,
            path: Some(self.artifact.candidate().path.clone()),
            span: Some(span),
        });
    }

    fn container_frame_for(&self, symbol: &SymbolRecord) -> ContainerFrame {
        ContainerFrame {
            symbol_id: symbol.symbol_id.clone(),
            qualified_name: symbol.qualified_name.clone().unwrap_or_default(),
            kind: symbol.kind.clone(),
        }
    }

    fn function_signature(&self, node: Node<'_>) -> String {
        let parameters = node
            .child_by_field_name("parameters")
            .map(|params| normalize_ws(&node_text(self.contents(), params)))
            .unwrap_or_default();
        let return_type = node
            .child_by_field_name("return_type")
            .map(|return_type| normalize_ws(&node_text(self.contents(), return_type)))
            .unwrap_or_default();
        format!("{parameters}|{return_type}")
    }

    fn class_signature(&self, node: Node<'_>) -> String {
        named_children(node)
            .into_iter()
            .filter(|child| {
                matches!(
                    child.kind(),
                    "class_heritage" | "extends_clause" | "implements_clause"
                )
            })
            .map(|child| normalize_ws(&node_text(self.contents(), child)))
            .collect::<Vec<_>>()
            .join("|")
    }

    fn span_for(&self, node: Node<'_>) -> SourceSpan {
        self.artifact
            .line_index()
            .byte_range_to_span(node.start_byte(), node.end_byte())
    }

    fn contents(&self) -> &[u8] {
        self.artifact.contents().as_bytes()
    }
}

fn named_children<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn children<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .collect()
}

fn child_by_field_name_or_kind<'tree>(
    node: Node<'tree>,
    field: &str,
    kinds: &[&str],
) -> Option<Node<'tree>> {
    node.child_by_field_name(field).or_else(|| {
        named_children(node)
            .into_iter()
            .find(|child| kinds.contains(&child.kind()))
    })
}

fn child_field_name<'tree>(parent: Node<'tree>, needle: Node<'tree>) -> Option<&'static str> {
    (0..parent.child_count()).find_map(|index| {
        let child = parent.child(index)?;
        if child.id() == needle.id() {
            parent.field_name_for_child(index as u32)
        } else {
            None
        }
    })
}

fn is_reference_candidate(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind().starts_with("jsx_") {
        return false;
    }
    let field_name = child_field_name(parent, node);
    if matches!(field_name, Some("name" | "alias" | "key" | "label")) {
        return false;
    }
    !matches!(
        parent.kind(),
        "function_declaration"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "method_definition"
            | "method_signature"
            | "import_specifier"
            | "namespace_import"
            | "export_specifier"
            | "shorthand_property_identifier_pattern"
            | "required_parameter"
            | "optional_parameter"
            | "rest_parameter"
    )
}

fn reference_matches_kind(node_kind: &str, symbol_kind: &SymbolKind) -> bool {
    match node_kind {
        "type_identifier" => matches!(
            symbol_kind,
            SymbolKind::Class
                | SymbolKind::Interface
                | SymbolKind::TypeAlias
                | SymbolKind::Enum
                | SymbolKind::ImportBinding
        ),
        "identifier" => !matches!(symbol_kind, SymbolKind::Module | SymbolKind::Namespace),
        _ => false,
    }
}

fn string_child_text(contents: &[u8], node: Node<'_>) -> Option<String> {
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == "string")
        .map(|child| trim_quotes(&node_text(contents, child)))
}

fn node_text(contents: &[u8], node: Node<'_>) -> String {
    node.utf8_text(contents).unwrap_or("").to_string()
}

fn trim_quotes(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn normalize_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn digest_hex(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn sanitize_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "anon".to_string()
    } else {
        sanitized
    }
}

fn identifier_after_prefix(text: &str, prefix: &str) -> Option<String> {
    let remainder = text.strip_prefix(prefix)?;
    let identifier = remainder
        .chars()
        .take_while(|char| char.is_ascii_alphanumeric() || *char == '_' || *char == '$')
        .collect::<String>();
    (!identifier.is_empty()).then_some(identifier)
}

fn find_statement_end(contents: &str, start: usize) -> usize {
    let remainder = &contents[start..];
    let semicolon = remainder.find(';').map(|offset| start + offset + 1);
    let newline = remainder.find('\n').map(|offset| start + offset);
    match (semicolon, newline) {
        (Some(semicolon), Some(newline)) => semicolon.min(newline),
        (Some(semicolon), None) => semicolon,
        (None, Some(newline)) => newline,
        (None, None) => contents.len(),
    }
}

fn language_name(language: &LanguageId) -> &'static str {
    match language {
        LanguageId::Typescript => "typescript",
        LanguageId::Tsx => "tsx",
        LanguageId::Javascript => "javascript",
        LanguageId::Jsx => "jsx",
        LanguageId::Mts => "mts",
        LanguageId::Cts => "cts",
    }
}

fn symbol_id(
    repo_id: &str,
    path: &str,
    kind: &SymbolKind,
    container: Option<&str>,
    name: &str,
    signature_digest: &str,
) -> String {
    let kind_name = symbol_kind_name(kind);
    let digest = digest_hex(&format!(
        "{repo_id}\n{path}\n{kind_name}\n{}\n{name}\n{signature_digest}",
        container.unwrap_or("-")
    ));
    format!(
        "sym.{}.{}.{}",
        kind_name,
        sanitize_name(name),
        &digest[..12]
    )
}

fn occurrence_id(
    snapshot_id: &str,
    path: &str,
    symbol_id: &SymbolId,
    role: &OccurrenceRole,
    span: &SourceSpan,
) -> String {
    let digest = digest_hex(&format!(
        "{snapshot_id}\n{path}\n{}\n{}\n{}:{}-{}:{}",
        symbol_id.0,
        occurrence_role_name(role),
        span.start.line,
        span.start.column,
        span.end.line,
        span.end.column
    ));
    format!("occ.{}.{}", occurrence_role_name(role), &digest[..12])
}

fn rebound_occurrence(snapshot_id: &str, occurrence: &SymbolOccurrence) -> SymbolOccurrence {
    let mut rebound = occurrence.clone();
    rebound.occurrence_id = OccurrenceId(occurrence_id(
        snapshot_id,
        &occurrence.path,
        &occurrence.symbol_id,
        &occurrence.role,
        &occurrence.span,
    ));
    rebound
}

fn rebound_edge(edge: &GraphEdge, occurrence_ids: &BTreeMap<String, OccurrenceId>) -> GraphEdge {
    let from = rebound_node_ref(&edge.from, occurrence_ids);
    let to = rebound_node_ref(&edge.to, occurrence_ids);
    GraphEdge {
        edge_id: edge_id(&edge.kind, &from, &to),
        kind: edge.kind.clone(),
        from,
        to,
    }
}

fn rebound_node_ref(
    node: &GraphNodeRef,
    occurrence_ids: &BTreeMap<String, OccurrenceId>,
) -> GraphNodeRef {
    match node {
        GraphNodeRef::Occurrence { occurrence_id } => GraphNodeRef::Occurrence {
            occurrence_id: occurrence_ids
                .get(&occurrence_id.0)
                .cloned()
                .unwrap_or_else(|| occurrence_id.clone()),
        },
        GraphNodeRef::Symbol { symbol_id } => GraphNodeRef::Symbol {
            symbol_id: symbol_id.clone(),
        },
        GraphNodeRef::File { path } => GraphNodeRef::File { path: path.clone() },
    }
}

fn edge_id(kind: &GraphEdgeKind, from: &GraphNodeRef, to: &GraphNodeRef) -> String {
    let digest = digest_hex(&format!("{:?}\n{:?}\n{:?}", kind, from, to));
    format!("edge.{}.{}", graph_edge_name(kind), &digest[..12])
}

fn qualify_name(container: Option<&ContainerFrame>, display_name: &str) -> String {
    match container {
        Some(container) => format!("{}::{}", container.qualified_name, display_name),
        None => display_name.to_string(),
    }
}

fn visibility_for(export: ExportContext) -> SymbolVisibility {
    if export.default_export {
        SymbolVisibility::DefaultExport
    } else if export.exported {
        SymbolVisibility::Exported
    } else {
        SymbolVisibility::Local
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

fn occurrence_role_name(role: &OccurrenceRole) -> &'static str {
    match role {
        OccurrenceRole::Definition => "definition",
        OccurrenceRole::Declaration => "declaration",
        OccurrenceRole::Reference => "reference",
        OccurrenceRole::Import => "import",
        OccurrenceRole::Export => "export",
    }
}

fn graph_edge_name(kind: &GraphEdgeKind) -> &'static str {
    match kind {
        GraphEdgeKind::Contains => "contains",
        GraphEdgeKind::Defines => "defines",
        GraphEdgeKind::References => "references",
        GraphEdgeKind::Imports => "imports",
        GraphEdgeKind::Exports => "exports",
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_parser::ParseCore;
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, SnapshotFile, WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::{
        GraphEdgeKind, GraphNodeRef, OccurrenceRole, ParseArtifactStage, SymbolKind,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use super::{FactWorkspace, FactsBatch, SymbolVisibility};

    fn extract(path: &str, contents: &str) -> FactsBatch {
        let snapshot = ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: "snap-1".to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "abc123".to_string(),
                digest: "base".to_string(),
                file_count: 1,
                files: vec![SnapshotFile {
                    path: path.to_string(),
                    content_sha256: format!("sha-{path}"),
                    content_bytes: contents.len(),
                    contents: contents.to_string(),
                }],
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        };
        let mut parser = ParseCore::default();
        let artifact = parser
            .parse_file_from_snapshot(&snapshot, path)
            .unwrap()
            .unwrap();
        FactWorkspace.extract("repo-1", "snap-1", &[artifact])
    }

    #[test]
    fn extracts_realistic_typescript_declarations_imports_and_exports() {
        let batch = extract(
            "src/module.ts",
            include_str!("../../hyperindex-parser/tests/fixtures/valid/module.ts"),
        );
        let file = &batch.files[0];
        let symbol_names = file
            .symbol_facts
            .iter()
            .map(|record| {
                (
                    record.symbol.display_name.clone(),
                    record.symbol.kind.clone(),
                )
            })
            .collect::<Vec<_>>();

        assert!(symbol_names.contains(&("src/module.ts".to_string(), SymbolKind::Module)));
        assert!(symbol_names.contains(&("createSession".to_string(), SymbolKind::ImportBinding)));
        assert!(symbol_names.contains(&("SessionContext".to_string(), SymbolKind::Interface)));
        assert!(symbol_names.contains(&("SessionState".to_string(), SymbolKind::TypeAlias)));
        assert!(symbol_names.contains(&("SessionService".to_string(), SymbolKind::Class)));
        assert!(symbol_names.contains(&("constructor".to_string(), SymbolKind::Constructor)));
        assert!(symbol_names.contains(&("invalidateSession".to_string(), SymbolKind::Method)));
        assert!(symbol_names.contains(&("invalidateLater".to_string(), SymbolKind::Function)));
        assert_eq!(file.import_facts.len(), 1);
        assert!(
            file.export_facts
                .iter()
                .any(|fact| fact.exported_name == "invalidateLater")
        );
        assert!(file.symbol_facts.iter().any(|record| {
            record.symbol.display_name == "SessionService"
                && record.visibility == SymbolVisibility::Exported
        }));
        assert_eq!(file.artifact.stage, ParseArtifactStage::FactsExtracted);
    }

    #[test]
    fn extracts_nested_declarations_with_container_chain() {
        let batch = extract(
            "src/nested.ts",
            r#"
            export function outer() {
              function inner() {}
              const helper = () => {
                function deeper() {}
                return deeper;
              };
              return helper;
            }
            "#,
        );
        let file = &batch.files[0];
        let outer = file
            .symbol_facts
            .iter()
            .find(|record| record.symbol.display_name == "outer")
            .unwrap();
        let inner = file
            .symbol_facts
            .iter()
            .find(|record| record.symbol.display_name == "inner")
            .unwrap();
        let helper = file
            .symbol_facts
            .iter()
            .find(|record| record.symbol.display_name == "helper")
            .unwrap();
        let deeper = file
            .symbol_facts
            .iter()
            .find(|record| record.symbol.display_name == "deeper")
            .unwrap();

        assert_eq!(inner.container.as_ref(), Some(&outer.symbol.symbol_id));
        assert_eq!(helper.container.as_ref(), Some(&outer.symbol.symbol_id));
        assert_eq!(deeper.container.as_ref(), Some(&helper.symbol.symbol_id));
        assert_ne!(inner.symbol.symbol_id, deeper.symbol.symbol_id);
    }

    #[test]
    fn handles_export_clauses_reexports_and_default_anonymous_exports() {
        let batch = extract(
            "src/exports.ts",
            r#"
            function helper() {}
            export { helper as exposed };
            export { upstream as remoteName } from "./dep";
            export default function () {}
            "#,
        );
        let file = &batch.files[0];

        assert!(
            file.export_facts
                .iter()
                .any(|fact| fact.exported_name == "exposed" && !fact.is_reexport)
        );
        assert!(file.export_facts.iter().any(|fact| {
            fact.exported_name == "remoteName"
                && fact.is_reexport
                && fact.module_specifier.as_deref() == Some("./dep")
        }));
        assert!(file.symbol_facts.iter().any(|record| {
            record.symbol.display_name == "default"
                && record.symbol.kind == SymbolKind::Function
                && record.visibility == SymbolVisibility::DefaultExport
        }));
        assert!(
            file.facts
                .occurrences
                .iter()
                .any(|occurrence| occurrence.role == OccurrenceRole::Export)
        );
    }

    #[test]
    fn broken_files_keep_partial_facts_and_parser_diagnostics() {
        let batch = extract(
            "src/editing.tsx",
            include_str!("../../hyperindex-parser/tests/fixtures/broken/editing.tsx"),
        );
        let file = &batch.files[0];

        assert!(!file.facts.diagnostics.is_empty());
        assert!(
            file.symbol_facts
                .iter()
                .any(|record| record.symbol.display_name == "App")
        );
        assert!(file.artifact.facts.symbol_count >= 2);
    }

    #[test]
    fn keeps_reference_linkage_conservative_for_plain_identifiers() {
        let batch = extract(
            "src/reference.ts",
            r#"
            export function createSession() {
              return 1;
            }

            export function run() {
              const fn = createSession;
              const registry = { createSession };
              registry.createSession();
              return fn();
            }
            "#,
        );
        let file = &batch.files[0];
        let create_session = file
            .symbol_facts
            .iter()
            .find(|record| record.symbol.display_name == "createSession")
            .unwrap();
        let references = file
            .facts
            .occurrences
            .iter()
            .filter(|occurrence| {
                occurrence.symbol_id == create_session.symbol.symbol_id
                    && occurrence.role == OccurrenceRole::Reference
            })
            .collect::<Vec<_>>();

        assert_eq!(references.len(), 1);
        assert!(file.facts.edges.iter().any(|edge| {
            edge.kind == GraphEdgeKind::References
                && matches!(
                    edge.to,
                    GraphNodeRef::Symbol { ref symbol_id }
                    if *symbol_id == create_session.symbol.symbol_id
                )
        }));
    }
}
