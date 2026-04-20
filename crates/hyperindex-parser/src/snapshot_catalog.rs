use std::collections::{BTreeSet, HashSet};

use hyperindex_protocol::config::RuntimeConfig;
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::symbols::{LanguageId, ParseInputSourceKind};
use hyperindex_snapshot::{ResolvedFrom, SnapshotAssembler};
use hyperindex_watcher::IgnoreMatcher;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::language_pack_ts_js::{LanguagePack, TsJsLanguage, TsJsLanguagePack};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseEligibilityRules {
    pub max_file_bytes: usize,
    pub ignore_patterns: Vec<String>,
    pub enabled_languages: Vec<LanguageId>,
    pub exclude_vendor_paths: bool,
    pub exclude_generated_paths: bool,
    pub exclude_binary_like_contents: bool,
}

impl Default for ParseEligibilityRules {
    fn default() -> Self {
        Self {
            max_file_bytes: 2_097_152,
            ignore_patterns: vec![
                ".git/**".to_string(),
                "node_modules/**".to_string(),
                "target/**".to_string(),
                ".next/**".to_string(),
                "dist/**".to_string(),
            ],
            enabled_languages: vec![
                LanguageId::Typescript,
                LanguageId::Tsx,
                LanguageId::Javascript,
                LanguageId::Jsx,
                LanguageId::Mts,
                LanguageId::Cts,
            ],
            exclude_vendor_paths: true,
            exclude_generated_paths: true,
            exclude_binary_like_contents: true,
        }
    }
}

impl ParseEligibilityRules {
    pub fn from_runtime_config(config: &RuntimeConfig) -> Self {
        let mut enabled_languages = Vec::new();
        for pack in &config.parser.language_packs {
            if pack.enabled {
                enabled_languages.extend(pack.languages.iter().cloned());
            }
        }
        if enabled_languages.is_empty() {
            enabled_languages = ParseEligibilityRules::default().enabled_languages;
        }

        let mut ignore_patterns = config.ignores.global_patterns.clone();
        ignore_patterns.extend(config.ignores.repo_patterns.clone());

        Self {
            max_file_bytes: config.parser.max_file_bytes,
            ignore_patterns,
            enabled_languages,
            exclude_vendor_paths: true,
            exclude_generated_paths: true,
            exclude_binary_like_contents: true,
        }
    }

    fn allows_language(&self, language: TsJsLanguage) -> bool {
        self.enabled_languages
            .iter()
            .any(|candidate| *candidate == language.to_protocol_language())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedParseFile {
    pub path: String,
    pub language: TsJsLanguage,
    pub source_kind: ParseInputSourceKind,
    pub content_sha256: String,
    pub content_bytes: usize,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseSkipReason {
    UnsupportedLanguage,
    IgnoredByPattern,
    FileTooLarge,
    BinaryLikeContent,
    VendorPath,
    GeneratedPath,
    DeletedInSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkippedParseFile {
    pub path: String,
    pub reason: ParseSkipReason,
    pub detail: String,
    pub content_bytes: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFileCatalog {
    pub eligible_files: Vec<ResolvedParseFile>,
    pub skipped_files: Vec<SkippedParseFile>,
}

impl SnapshotFileCatalog {
    pub fn build(snapshot: &ComposedSnapshot, rules: &ParseEligibilityRules) -> Self {
        let assembler = SnapshotAssembler;
        let language_pack = TsJsLanguagePack;
        let ignore_matcher = IgnoreMatcher::new(rules.ignore_patterns.clone());
        let mut paths = BTreeSet::new();
        let mut eligible_files = Vec::new();
        let mut skipped_files = Vec::new();

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
            let Some(resolved) = assembler.resolve_file(snapshot, &path) else {
                skipped_files.push(SkippedParseFile {
                    path,
                    reason: ParseSkipReason::DeletedInSnapshot,
                    detail: "path is deleted by the composed snapshot overlays".to_string(),
                    content_bytes: None,
                });
                continue;
            };

            if ignore_matcher.is_ignored(&resolved.path) {
                skipped_files.push(SkippedParseFile {
                    path: resolved.path,
                    reason: ParseSkipReason::IgnoredByPattern,
                    detail: "path matched configured ignore rules".to_string(),
                    content_bytes: Some(resolved.contents.len()),
                });
                continue;
            }

            if rules.exclude_vendor_paths && is_vendor_path(&resolved.path) {
                skipped_files.push(SkippedParseFile {
                    path: resolved.path,
                    reason: ParseSkipReason::VendorPath,
                    detail: "path matched the phase-appropriate vendor exclusion heuristic"
                        .to_string(),
                    content_bytes: Some(resolved.contents.len()),
                });
                continue;
            }

            if rules.exclude_generated_paths && is_generated_path(&resolved.path) {
                skipped_files.push(SkippedParseFile {
                    path: resolved.path,
                    reason: ParseSkipReason::GeneratedPath,
                    detail: "path matched the phase-appropriate generated-file exclusion heuristic"
                        .to_string(),
                    content_bytes: Some(resolved.contents.len()),
                });
                continue;
            }

            let Some(language) = language_pack.detect_path(&resolved.path) else {
                skipped_files.push(SkippedParseFile {
                    path: resolved.path,
                    reason: ParseSkipReason::UnsupportedLanguage,
                    detail: "path extension is not supported by the active TS/JS language pack"
                        .to_string(),
                    content_bytes: Some(resolved.contents.len()),
                });
                continue;
            };

            if !rules.allows_language(language) {
                skipped_files.push(SkippedParseFile {
                    path: resolved.path,
                    reason: ParseSkipReason::UnsupportedLanguage,
                    detail: "language is disabled by the active parser rules".to_string(),
                    content_bytes: Some(resolved.contents.len()),
                });
                continue;
            }

            if resolved.contents.len() > rules.max_file_bytes {
                skipped_files.push(SkippedParseFile {
                    path: resolved.path,
                    reason: ParseSkipReason::FileTooLarge,
                    detail: format!(
                        "file exceeds max parse size: {} > {} bytes",
                        resolved.contents.len(),
                        rules.max_file_bytes
                    ),
                    content_bytes: Some(resolved.contents.len()),
                });
                continue;
            }

            if rules.exclude_binary_like_contents && is_binary_like(&resolved.contents) {
                skipped_files.push(SkippedParseFile {
                    path: resolved.path,
                    reason: ParseSkipReason::BinaryLikeContent,
                    detail: "file contents look binary-like and were excluded".to_string(),
                    content_bytes: Some(resolved.contents.len()),
                });
                continue;
            }

            eligible_files.push(ResolvedParseFile {
                path: resolved.path,
                language,
                source_kind: source_from(&resolved.resolved_from),
                content_sha256: sha256_hex(resolved.contents.as_bytes()),
                content_bytes: resolved.contents.len(),
                contents: resolved.contents,
            });
        }

        eligible_files.sort_by(|left, right| left.path.cmp(&right.path));
        skipped_files.sort_by(|left, right| left.path.cmp(&right.path));

        Self {
            eligible_files,
            skipped_files,
        }
    }
}

fn source_from(value: &ResolvedFrom) -> ParseInputSourceKind {
    match value {
        ResolvedFrom::BufferOverlay(_) => ParseInputSourceKind::BufferOverlay,
        ResolvedFrom::WorkingTreeOverlay => ParseInputSourceKind::WorkingTreeOverlay,
        ResolvedFrom::BaseSnapshot => ParseInputSourceKind::BaseSnapshot,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn is_vendor_path(path: &str) -> bool {
    path_components(path).contains("vendor")
        || path_components(path).contains("vendors")
        || path_components(path).contains("third_party")
        || path_components(path).contains("third-party")
        || path_components(path).contains("node_modules")
}

fn is_generated_path(path: &str) -> bool {
    let components = path_components(path);
    let file_name = path.rsplit('/').next().unwrap_or(path);
    components.contains("__generated__")
        || components.contains("generated")
        || components.contains("gen")
        || file_name.contains(".generated.")
        || file_name.contains(".gen.")
        || file_name.ends_with(".min.js")
}

fn path_components(path: &str) -> HashSet<&str> {
    path.split('/').filter(|part| !part.is_empty()).collect()
}

fn is_binary_like(contents: &str) -> bool {
    if contents.contains('\0') {
        return true;
    }

    let control_bytes = contents
        .bytes()
        .filter(|byte| matches!(byte, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F))
        .count();
    control_bytes > 0
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, BufferOverlay, ComposedSnapshot, OverlayEntryKind,
        SnapshotFile, WorkingTreeEntry, WorkingTreeOverlay,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use super::{ParseEligibilityRules, ParseSkipReason, SnapshotFileCatalog};

    #[test]
    fn catalog_excludes_ignored_vendor_generated_and_binary_files() {
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
                file_count: 6,
                files: vec![
                    SnapshotFile {
                        path: "src/app.ts".to_string(),
                        content_sha256: "sha-app".to_string(),
                        content_bytes: 23,
                        contents: "export const value = 1;".to_string(),
                    },
                    SnapshotFile {
                        path: "ignored/drop.ts".to_string(),
                        content_sha256: "sha-ignore".to_string(),
                        content_bytes: 23,
                        contents: "export const ignored = 1;".to_string(),
                    },
                    SnapshotFile {
                        path: "vendor/lib.ts".to_string(),
                        content_sha256: "sha-vendor".to_string(),
                        content_bytes: 25,
                        contents: "export const vendor = 1;".to_string(),
                    },
                    SnapshotFile {
                        path: "src/__generated__/types.ts".to_string(),
                        content_sha256: "sha-generated".to_string(),
                        content_bytes: 28,
                        contents: "export const generated = 1;".to_string(),
                    },
                    SnapshotFile {
                        path: "src/blob.ts".to_string(),
                        content_sha256: "sha-binary".to_string(),
                        content_bytes: 6,
                        contents: "abc\0de".to_string(),
                    },
                    SnapshotFile {
                        path: "src/deleted.ts".to_string(),
                        content_sha256: "sha-deleted".to_string(),
                        content_bytes: 25,
                        contents: "export const deleted = 1;".to_string(),
                    },
                ],
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: vec![WorkingTreeEntry {
                    path: "src/deleted.ts".to_string(),
                    kind: OverlayEntryKind::Delete,
                    content_sha256: None,
                    content_bytes: None,
                    contents: None,
                }],
            },
            buffers: vec![BufferOverlay {
                buffer_id: "buffer-1".to_string(),
                path: "src/app.ts".to_string(),
                version: 2,
                content_sha256: "sha-buffer".to_string(),
                content_bytes: 30,
                contents: "export const value = buffer();".to_string(),
            }],
        };
        let mut rules = ParseEligibilityRules::default();
        rules.ignore_patterns.push("ignored/**".to_string());

        let catalog = SnapshotFileCatalog::build(&snapshot, &rules);

        assert_eq!(catalog.eligible_files.len(), 1);
        assert_eq!(catalog.eligible_files[0].path, "src/app.ts");
        assert_eq!(catalog.skipped_files.len(), 5);
        assert_eq!(
            catalog
                .skipped_files
                .iter()
                .map(|file| file.reason.clone())
                .collect::<Vec<_>>(),
            vec![
                ParseSkipReason::IgnoredByPattern,
                ParseSkipReason::GeneratedPath,
                ParseSkipReason::BinaryLikeContent,
                ParseSkipReason::DeletedInSnapshot,
                ParseSkipReason::VendorPath,
            ]
        );
    }
}
