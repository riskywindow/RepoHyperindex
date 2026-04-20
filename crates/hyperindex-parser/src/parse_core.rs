use std::collections::{BTreeSet, VecDeque};

use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::symbols::{
    FileFactsSummary, FileParseArtifactMetadata, ParseArtifactStage, ParseDiagnostic,
    ParseDiagnosticCode, ParseDiagnosticSeverity, ParseInputSourceKind, SourceSpan,
};
use hyperindex_snapshot::{ResolvedFrom, SnapshotAssembler};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use tree_sitter::{Node, Tree};

use crate::language_pack_ts_js::{LanguagePack, TsJsLanguage, TsJsLanguagePack};
use crate::line_index::LineIndex;
use crate::parse_cache::{ParseCache, ParseCacheKey};
use crate::{ParserError, ParserResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCandidate {
    pub path: String,
    pub language: TsJsLanguage,
    pub source_kind: ParseInputSourceKind,
    pub content_sha256: String,
    pub content_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct ParsedSyntaxTree {
    tree: Tree,
}

impl ParsedSyntaxTree {
    pub fn new(tree: Tree) -> Self {
        Self { tree }
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn root_node(&self) -> Node<'_> {
        self.tree.root_node()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstNodeHandle {
    pub kind: String,
    pub span: SourceSpan,
    pub named_child_count: u32,
    pub child_count: u32,
    pub has_error: bool,
    pub is_error: bool,
    pub is_missing: bool,
}

impl AstNodeHandle {
    pub fn from_node(node: Node<'_>, line_index: &LineIndex) -> Self {
        Self::from_byte_range(
            node.kind(),
            line_index,
            node.start_byte(),
            node.end_byte(),
            node.has_error(),
            node.is_error(),
            node.is_missing(),
            node.named_child_count() as u32,
            node.child_count() as u32,
        )
    }

    pub fn from_byte_range(
        kind: &str,
        line_index: &LineIndex,
        start_byte: usize,
        end_byte: usize,
        has_error: bool,
        is_error: bool,
        is_missing: bool,
        named_child_count: u32,
        child_count: u32,
    ) -> Self {
        Self {
            kind: kind.to_string(),
            span: line_index.byte_range_to_span(start_byte, end_byte),
            named_child_count,
            child_count,
            has_error,
            is_error,
            is_missing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseArtifactInspection {
    pub artifact: FileParseArtifactMetadata,
    pub parse_succeeded: bool,
    pub has_recoverable_errors: bool,
    pub reused_incremental_tree: bool,
    pub line_count: u32,
    pub root: AstNodeHandle,
}

#[derive(Debug, Clone)]
pub struct ParseArtifact {
    candidate: ParseCandidate,
    cache_key: ParseCacheKey,
    metadata: FileParseArtifactMetadata,
    parse_succeeded: bool,
    reused_incremental_tree: bool,
    contents: String,
    line_index: LineIndex,
    syntax: ParsedSyntaxTree,
    root: AstNodeHandle,
}

impl ParseArtifact {
    pub fn candidate(&self) -> &ParseCandidate {
        &self.candidate
    }

    pub fn cache_key(&self) -> &ParseCacheKey {
        &self.cache_key
    }

    pub fn metadata(&self) -> &FileParseArtifactMetadata {
        &self.metadata
    }

    pub fn parse_succeeded(&self) -> bool {
        self.parse_succeeded
    }

    pub fn has_recoverable_errors(&self) -> bool {
        !self.metadata.diagnostics.is_empty()
    }

    pub fn reused_incremental_tree(&self) -> bool {
        self.reused_incremental_tree
    }

    pub fn contents(&self) -> &str {
        &self.contents
    }

    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    pub fn syntax(&self) -> &ParsedSyntaxTree {
        &self.syntax
    }

    pub fn root(&self) -> &AstNodeHandle {
        &self.root
    }

    pub fn inspection(&self) -> ParseArtifactInspection {
        ParseArtifactInspection {
            artifact: self.metadata.clone(),
            parse_succeeded: self.parse_succeeded,
            has_recoverable_errors: self.has_recoverable_errors(),
            reused_incremental_tree: self.reused_incremental_tree,
            line_count: self.line_index.line_count() as u32,
            root: self.root.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(
        candidate: ParseCandidate,
        cache_key: ParseCacheKey,
        metadata: FileParseArtifactMetadata,
        parse_succeeded: bool,
        reused_incremental_tree: bool,
        contents: String,
        line_index: LineIndex,
        syntax: ParsedSyntaxTree,
        root: AstNodeHandle,
    ) -> Self {
        Self {
            candidate,
            cache_key,
            metadata,
            parse_succeeded,
            reused_incremental_tree,
            contents,
            line_index,
            syntax,
            root,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParseBatchPlan {
    pub snapshot_id: String,
    pub artifacts: Vec<ParseArtifact>,
    pub skipped_paths: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ParseCore {
    language_pack: TsJsLanguagePack,
    cache: ParseCache,
    settings: ParseCoreSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCoreSettings {
    pub diagnostics_max_per_file: usize,
}

impl Default for ParseCoreSettings {
    fn default() -> Self {
        Self {
            diagnostics_max_per_file: 32,
        }
    }
}

impl ParseCore {
    pub fn with_settings(settings: ParseCoreSettings) -> Self {
        Self {
            language_pack: TsJsLanguagePack,
            cache: ParseCache::default(),
            settings,
        }
    }

    pub fn parse_snapshot(&mut self, snapshot: &ComposedSnapshot) -> ParserResult<ParseBatchPlan> {
        let assembler = SnapshotAssembler;
        let mut paths = BTreeSet::new();
        let mut skipped_paths = Vec::new();
        let mut artifacts = Vec::new();

        for file in &snapshot.base.files {
            paths.insert(file.path.clone());
        }
        for entry in &snapshot.working_tree.entries {
            paths.insert(entry.path.clone());
        }
        for buffer in &snapshot.buffers {
            paths.insert(buffer.path.clone());
        }

        for path in paths {
            let Some(language) = self.language_pack.detect_path(&path) else {
                skipped_paths.push(path);
                continue;
            };

            let Some(resolved) = assembler.resolve_file(snapshot, &path) else {
                debug!(snapshot_id = %snapshot.snapshot_id, path = %path, "skipping deleted path during parse");
                continue;
            };

            let artifact = self.parse_contents(
                ParseCandidate {
                    path: resolved.path.clone(),
                    language,
                    source_kind: source_from(&resolved.resolved_from),
                    content_sha256: ParseCacheKey::from_contents(
                        &resolved.path,
                        &resolved.contents,
                    )
                    .content_sha256,
                    content_bytes: resolved.contents.len(),
                },
                resolved.contents,
            )?;
            artifacts.push(artifact);
        }

        info!(
            snapshot_id = %snapshot.snapshot_id,
            parsed_files = artifacts.len(),
            skipped_paths = skipped_paths.len(),
            "parsed snapshot with tree-sitter ts/js pack"
        );

        Ok(ParseBatchPlan {
            snapshot_id: snapshot.snapshot_id.clone(),
            artifacts,
            skipped_paths,
        })
    }

    pub fn parse_file_from_snapshot(
        &mut self,
        snapshot: &ComposedSnapshot,
        path: &str,
    ) -> ParserResult<Option<ParseArtifact>> {
        let Some(language) = self.language_pack.detect_path(path) else {
            return Ok(None);
        };
        let assembler = SnapshotAssembler;
        let Some(resolved) = assembler.resolve_file(snapshot, path) else {
            return Ok(None);
        };
        self.parse_contents(
            ParseCandidate {
                path: resolved.path.clone(),
                language,
                source_kind: source_from(&resolved.resolved_from),
                content_sha256: ParseCacheKey::from_contents(&resolved.path, &resolved.contents)
                    .content_sha256,
                content_bytes: resolved.contents.len(),
            },
            resolved.contents,
        )
        .map(Some)
    }

    pub fn reparse(
        &mut self,
        prior: &ParseArtifact,
        new_contents: &str,
        source_kind: ParseInputSourceKind,
    ) -> ParserResult<ParseArtifact> {
        self.parse_from_contents(
            ParseCandidate {
                path: prior.candidate.path.clone(),
                language: prior.candidate.language,
                source_kind,
                content_sha256: ParseCacheKey::from_contents(&prior.candidate.path, new_contents)
                    .content_sha256,
                content_bytes: new_contents.len(),
            },
            new_contents.to_string(),
            Some(prior),
        )
    }

    pub fn parse_contents(
        &mut self,
        candidate: ParseCandidate,
        contents: String,
    ) -> ParserResult<ParseArtifact> {
        let previous = self.cache.latest_for_path(&candidate.path).cloned();
        let artifact = self.parse_from_contents(candidate, contents, previous.as_ref())?;
        self.cache.upsert(artifact.clone());
        Ok(artifact)
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    fn parse_from_contents(
        &self,
        candidate: ParseCandidate,
        contents: String,
        previous: Option<&ParseArtifact>,
    ) -> ParserResult<ParseArtifact> {
        let cache_key = ParseCacheKey::from_contents(&candidate.path, &contents);
        let line_index = LineIndex::new(&contents);
        let mut parser = self.language_pack.new_parser(candidate.language)?;

        let (tree, reused_incremental_tree) = match previous {
            Some(previous) if previous.candidate.language == candidate.language => {
                if previous.contents == contents {
                    (previous.syntax.tree().clone(), true)
                } else {
                    let mut edited_tree = previous.syntax.tree().clone();
                    let edit =
                        previous
                            .line_index
                            .edit_from(previous.contents(), &contents, &line_index);
                    edited_tree.edit(&edit);
                    let reparsed =
                        parser.parse(&contents, Some(&edited_tree)).ok_or_else(|| {
                            ParserError::Message(format!(
                                "tree-sitter failed to reparse {}",
                                candidate.path
                            ))
                        })?;
                    (reparsed, true)
                }
            }
            _ => (
                parser.parse(&contents, None).ok_or_else(|| {
                    ParserError::Message(format!("tree-sitter failed to parse {}", candidate.path))
                })?,
                false,
            ),
        };

        let syntax = ParsedSyntaxTree::new(tree);
        let root = AstNodeHandle::from_node(syntax.root_node(), &line_index);
        let diagnostics = collect_diagnostics(
            &candidate.path,
            &line_index,
            syntax.root_node(),
            self.settings.diagnostics_max_per_file,
        );
        let metadata = FileParseArtifactMetadata {
            artifact_id: format!("parse:{}:{}", candidate.path, cache_key.content_sha256),
            path: candidate.path.clone(),
            language: candidate.language.to_protocol_language(),
            source_kind: candidate.source_kind.clone(),
            stage: ParseArtifactStage::Parsed,
            content_sha256: cache_key.content_sha256.clone(),
            content_bytes: candidate.content_bytes as u64,
            parser_pack_id: self.language_pack.pack_id().to_string(),
            facts: FileFactsSummary {
                symbol_count: 0,
                occurrence_count: 0,
                edge_count: 0,
            },
            diagnostics: diagnostics.clone(),
        };

        Ok(ParseArtifact {
            candidate,
            cache_key,
            metadata,
            parse_succeeded: true,
            reused_incremental_tree,
            contents,
            line_index,
            syntax,
            root,
        })
    }
}

fn source_from(value: &ResolvedFrom) -> ParseInputSourceKind {
    match value {
        ResolvedFrom::BufferOverlay(_) => ParseInputSourceKind::BufferOverlay,
        ResolvedFrom::WorkingTreeOverlay => ParseInputSourceKind::WorkingTreeOverlay,
        ResolvedFrom::BaseSnapshot => ParseInputSourceKind::BaseSnapshot,
    }
}

fn collect_diagnostics(
    path: &str,
    line_index: &LineIndex,
    root: Node<'_>,
    diagnostics_max_per_file: usize,
) -> Vec<ParseDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([root]);

    while let Some(node) = queue.pop_front() {
        if diagnostics.len() >= diagnostics_max_per_file {
            break;
        }
        if node.is_error() || node.is_missing() {
            let span = line_index.byte_range_to_span(node.start_byte(), node.end_byte());
            let message = if node.is_missing() {
                format!("missing syntax node near {}", node.kind())
            } else {
                format!("syntax error near {}", node.kind())
            };
            let key = (
                span.bytes.start,
                span.bytes.end,
                node.kind().to_string(),
                node.is_missing(),
            );
            if seen.insert(key) {
                diagnostics.push(ParseDiagnostic {
                    severity: ParseDiagnosticSeverity::Error,
                    code: ParseDiagnosticCode::SyntaxError,
                    message,
                    path: Some(path.to_string()),
                    span: Some(span),
                });
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            queue.push_back(child);
        }
    }

    if diagnostics.is_empty() && root.has_error() {
        diagnostics.push(ParseDiagnostic {
            severity: ParseDiagnosticSeverity::Error,
            code: ParseDiagnosticCode::PartialAnalysis,
            message: "tree-sitter reported parse recovery without a concrete error node"
                .to_string(),
            path: Some(path.to_string()),
            span: Some(line_index.byte_range_to_span(root.start_byte(), root.end_byte())),
        });
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, BufferOverlay, ComposedSnapshot, OverlayEntryKind,
        SnapshotFile, WorkingTreeEntry, WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::ParseInputSourceKind;
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use super::ParseCore;

    #[test]
    fn parse_snapshot_prefers_overlay_order_and_only_keeps_supported_files() {
        let mut core = ParseCore::default();
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
                file_count: 2,
                files: vec![
                    SnapshotFile {
                        path: "src/app.ts".to_string(),
                        content_sha256: "sha-base".to_string(),
                        content_bytes: 23,
                        contents: "export const value = 1;".to_string(),
                    },
                    SnapshotFile {
                        path: "README.md".to_string(),
                        content_sha256: "sha-readme".to_string(),
                        content_bytes: 4,
                        contents: "read".to_string(),
                    },
                ],
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: vec![WorkingTreeEntry {
                    path: "src/app.ts".to_string(),
                    kind: OverlayEntryKind::Upsert,
                    content_sha256: Some("sha-work".to_string()),
                    content_bytes: Some(30),
                    contents: Some("export const value = work();".to_string()),
                }],
            },
            buffers: vec![BufferOverlay {
                buffer_id: "buffer-1".to_string(),
                path: "src/app.ts".to_string(),
                version: 3,
                content_sha256: "sha-buffer".to_string(),
                content_bytes: 32,
                contents: "export const value = buffer();".to_string(),
            }],
        };

        let batch = core.parse_snapshot(&snapshot).unwrap();
        assert_eq!(batch.artifacts.len(), 1);
        assert_eq!(batch.skipped_paths, vec!["README.md".to_string()]);
        assert_eq!(batch.artifacts[0].candidate().path, "src/app.ts");
        assert_eq!(
            batch.artifacts[0].metadata().source_kind,
            ParseInputSourceKind::BufferOverlay
        );
        assert!(batch.artifacts[0].parse_succeeded());
        assert_eq!(batch.artifacts[0].root().kind, "program");
        assert_eq!(core.cache_len(), 1);
    }

    #[test]
    fn reparse_reuses_prior_tree_and_updates_diagnostics() {
        let mut core = ParseCore::default();
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
                    path: "src/app.tsx".to_string(),
                    content_sha256: "sha-base".to_string(),
                    content_bytes: 37,
                    contents: "export const App = () => <div />;".to_string(),
                }],
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        };

        let parsed = core
            .parse_file_from_snapshot(&snapshot, "src/app.tsx")
            .unwrap()
            .unwrap();
        let reparsed = core
            .reparse(
                &parsed,
                "export const App = () => <div>{label</div>;",
                ParseInputSourceKind::BufferOverlay,
            )
            .unwrap();

        assert!(reparsed.reused_incremental_tree());
        assert!(reparsed.has_recoverable_errors());
        assert_eq!(
            reparsed.metadata().source_kind,
            ParseInputSourceKind::BufferOverlay
        );
    }
}
