use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use hyperindex_protocol::config::RuntimeConfig;
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::symbols::{
    FileFactsSummary, FileParseArtifactMetadata, LanguageId, ParseArtifactStage, ParseDiagnostic,
    ParseInputSourceKind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::parse_core::{AstNodeHandle, ParseArtifact, ParseArtifactInspection, ParseCandidate};
use crate::snapshot_catalog::{ParseEligibilityRules, SkippedParseFile, SnapshotFileCatalog};
use crate::{ParseCore, ParseCoreSettings, ParserError, ParserResult};

const BUILD_SCHEMA_VERSION: u32 = 1;
const CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseManagerOptions {
    pub store_root: PathBuf,
    pub rules: ParseEligibilityRules,
    pub diagnostics_max_per_file: usize,
}

impl ParseManagerOptions {
    pub fn from_runtime_config(config: &RuntimeConfig) -> Self {
        Self {
            store_root: config.parser.artifact_dir.clone(),
            rules: ParseEligibilityRules::from_runtime_config(config),
            diagnostics_max_per_file: config.parser.diagnostics_max_per_file,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseBuildStats {
    pub planned_file_count: u64,
    pub parsed_file_count: u64,
    pub reused_file_count: u64,
    pub skipped_file_count: u64,
    pub diagnostic_count: u64,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseStoreFileRecord {
    pub path: String,
    pub cache_id: String,
    pub reused_from_cache: bool,
    pub inspection: ParseArtifactInspection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseBuildStatus {
    pub schema_version: u32,
    pub build_id: String,
    pub repo_id: String,
    pub snapshot_id: String,
    pub parser_config_digest: String,
    pub artifact_root: String,
    pub cache_root: String,
    pub created_at_epoch_ms: u128,
    pub loaded_from_existing_build: bool,
    pub stats: ParseBuildStats,
    pub files: Vec<ParseStoreFileRecord>,
    pub skipped_files: Vec<SkippedParseFile>,
}

#[derive(Debug, Clone)]
pub struct ParseManager {
    core: ParseCore,
    options: ParseManagerOptions,
    parser_config_digest: String,
}

impl ParseManager {
    pub fn new(options: ParseManagerOptions) -> Self {
        let parser_config_digest = parser_config_digest(&options);
        Self {
            core: ParseCore::with_settings(ParseCoreSettings {
                diagnostics_max_per_file: options.diagnostics_max_per_file,
            }),
            options,
            parser_config_digest,
        }
    }

    pub fn from_runtime_config(config: &RuntimeConfig) -> Self {
        Self::new(ParseManagerOptions::from_runtime_config(config))
    }

    pub fn build_snapshot(
        &mut self,
        snapshot: &ComposedSnapshot,
        force: bool,
    ) -> ParserResult<ParseBuildStatus> {
        if !force {
            if let Some(mut existing) = self.load_build_status(snapshot)? {
                existing.loaded_from_existing_build = true;
                return Ok(existing);
            }
        }

        let started = Instant::now();
        let catalog = SnapshotFileCatalog::build(snapshot, &self.options.rules);
        let mut files = Vec::new();
        let mut parsed_file_count = 0u64;
        let mut reused_file_count = 0u64;
        let mut diagnostic_count = 0u64;

        for file in &catalog.eligible_files {
            let cache_id = cache_id_for(&self.parser_config_digest, file);
            let record = if !force {
                self.load_cached_unit(&cache_id)?
                    .map(|unit| unit.into_file_record(file.source_kind.clone()))
            } else {
                None
            };

            let record = match record {
                Some(mut record) => {
                    record.reused_from_cache = true;
                    reused_file_count += 1;
                    diagnostic_count += record.inspection.artifact.diagnostics.len() as u64;
                    record
                }
                None => {
                    let artifact = self.core.parse_contents(
                        ParseCandidate {
                            path: file.path.clone(),
                            language: file.language,
                            source_kind: file.source_kind.clone(),
                            content_sha256: file.content_sha256.clone(),
                            content_bytes: file.content_bytes,
                        },
                        file.contents.clone(),
                    )?;
                    let cache_unit = CachedParseUnit::from_artifact(
                        &cache_id,
                        &self.parser_config_digest,
                        &artifact,
                    );
                    self.persist_cached_unit(&cache_unit)?;
                    parsed_file_count += 1;
                    diagnostic_count += artifact.metadata().diagnostics.len() as u64;
                    file_record_from_artifact(&cache_id, &artifact)
                }
            };
            files.push(record);
        }

        let build_id = build_id_for(&snapshot.snapshot_id, &self.parser_config_digest);
        let build_root = self.build_root(snapshot, &build_id);
        let status = ParseBuildStatus {
            schema_version: BUILD_SCHEMA_VERSION,
            build_id,
            repo_id: snapshot.repo_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            parser_config_digest: self.parser_config_digest.clone(),
            artifact_root: build_root.display().to_string(),
            cache_root: self.cache_root().display().to_string(),
            created_at_epoch_ms: epoch_ms(),
            loaded_from_existing_build: false,
            stats: ParseBuildStats {
                planned_file_count: catalog.eligible_files.len() as u64,
                parsed_file_count,
                reused_file_count,
                skipped_file_count: catalog.skipped_files.len() as u64,
                diagnostic_count,
                elapsed_ms: started.elapsed().as_millis(),
            },
            files,
            skipped_files: catalog.skipped_files,
        };
        self.persist_build_status(snapshot, &status)?;
        Ok(status)
    }

    pub fn load_build_status(
        &self,
        snapshot: &ComposedSnapshot,
    ) -> ParserResult<Option<ParseBuildStatus>> {
        let build_id = build_id_for(&snapshot.snapshot_id, &self.parser_config_digest);
        let path = self.build_root(snapshot, &build_id).join("build.json");
        if !path.exists() {
            return Ok(None);
        }

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "discarding unreadable parse build status"
                );
                let _ = fs::remove_file(&path);
                return Ok(None);
            }
        };
        let status = match serde_json::from_str::<ParseBuildStatus>(&raw) {
            Ok(status) => status,
            Err(error) => {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "discarding corrupted parse build status"
                );
                let _ = fs::remove_file(&path);
                return Ok(None);
            }
        };
        if status.schema_version != BUILD_SCHEMA_VERSION {
            warn!(
                path = %path.display(),
                found = status.schema_version,
                expected = BUILD_SCHEMA_VERSION,
                "discarding incompatible parse build status"
            );
            let _ = fs::remove_file(&path);
            return Ok(None);
        }
        if status.parser_config_digest != self.parser_config_digest {
            return Ok(None);
        }
        Ok(Some(status))
    }

    pub fn inspect_file(
        &mut self,
        snapshot: &ComposedSnapshot,
        path: &str,
    ) -> ParserResult<Option<ParseStoreFileRecord>> {
        let build = self.build_snapshot(snapshot, false)?;
        Ok(build.files.into_iter().find(|file| file.path == path))
    }

    fn persist_build_status(
        &self,
        snapshot: &ComposedSnapshot,
        status: &ParseBuildStatus,
    ) -> ParserResult<()> {
        let build_root = self.build_root(snapshot, &status.build_id);
        fs::create_dir_all(&build_root).map_err(|error| {
            ParserError::Message(format!(
                "failed to create parse build dir {}: {error}",
                build_root.display()
            ))
        })?;
        let raw = serde_json::to_string_pretty(status)
            .map_err(|error| ParserError::Message(format!("failed to serialize build: {error}")))?;
        let path = build_root.join("build.json");
        fs::write(&path, raw).map_err(|error| {
            ParserError::Message(format!(
                "failed to write parse build {}: {error}",
                path.display()
            ))
        })
    }

    fn load_cached_unit(&self, cache_id: &str) -> ParserResult<Option<CachedParseUnit>> {
        let path = self.cache_path(cache_id);
        if !path.exists() {
            return Ok(None);
        }

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "discarding unreadable parse cache entry"
                );
                let _ = fs::remove_file(&path);
                return Ok(None);
            }
        };
        let unit = match serde_json::from_str::<CachedParseUnit>(&raw) {
            Ok(unit) => unit,
            Err(error) => {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "discarding corrupted parse cache entry"
                );
                let _ = fs::remove_file(&path);
                return Ok(None);
            }
        };
        if unit.schema_version != CACHE_SCHEMA_VERSION {
            warn!(
                path = %path.display(),
                found = unit.schema_version,
                expected = CACHE_SCHEMA_VERSION,
                "discarding incompatible parse cache entry"
            );
            let _ = fs::remove_file(&path);
            return Ok(None);
        }
        if unit.parser_config_digest != self.parser_config_digest {
            return Ok(None);
        }
        Ok(Some(unit))
    }

    fn persist_cached_unit(&self, unit: &CachedParseUnit) -> ParserResult<()> {
        let cache_root = self.cache_root();
        fs::create_dir_all(&cache_root).map_err(|error| {
            ParserError::Message(format!(
                "failed to create parse cache dir {}: {error}",
                cache_root.display()
            ))
        })?;
        let raw = serde_json::to_string(unit).map_err(|error| {
            ParserError::Message(format!("failed to serialize cache entry: {error}"))
        })?;
        let path = self.cache_path(&unit.cache_id);
        fs::write(&path, raw).map_err(|error| {
            ParserError::Message(format!(
                "failed to write parse cache entry {}: {error}",
                path.display()
            ))
        })
    }

    fn cache_root(&self) -> PathBuf {
        self.options.store_root.join("_cache")
    }

    fn cache_path(&self, cache_id: &str) -> PathBuf {
        self.cache_root().join(format!("{cache_id}.json"))
    }

    fn build_root(&self, snapshot: &ComposedSnapshot, build_id: &str) -> PathBuf {
        self.options
            .store_root
            .join(&snapshot.repo_id)
            .join(&snapshot.snapshot_id)
            .join(build_id)
    }
}

fn file_record_from_artifact(cache_id: &str, artifact: &ParseArtifact) -> ParseStoreFileRecord {
    ParseStoreFileRecord {
        path: artifact.metadata().path.clone(),
        cache_id: cache_id.to_string(),
        reused_from_cache: false,
        inspection: artifact.inspection(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CachedParseUnit {
    schema_version: u32,
    cache_id: String,
    parser_config_digest: String,
    path: String,
    language: LanguageId,
    content_sha256: String,
    content_bytes: u64,
    parser_pack_id: String,
    diagnostics: Vec<ParseDiagnostic>,
    parse_succeeded: bool,
    line_count: u32,
    root: AstNodeHandle,
}

impl CachedParseUnit {
    fn from_artifact(cache_id: &str, parser_config_digest: &str, artifact: &ParseArtifact) -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            cache_id: cache_id.to_string(),
            parser_config_digest: parser_config_digest.to_string(),
            path: artifact.metadata().path.clone(),
            language: artifact.metadata().language.clone(),
            content_sha256: artifact.metadata().content_sha256.clone(),
            content_bytes: artifact.metadata().content_bytes,
            parser_pack_id: artifact.metadata().parser_pack_id.clone(),
            diagnostics: artifact.metadata().diagnostics.clone(),
            parse_succeeded: artifact.parse_succeeded(),
            line_count: artifact.inspection().line_count,
            root: artifact.root().clone(),
        }
    }

    fn into_file_record(self, source_kind: ParseInputSourceKind) -> ParseStoreFileRecord {
        let has_recoverable_errors = !self.diagnostics.is_empty();
        let inspection = ParseArtifactInspection {
            artifact: FileParseArtifactMetadata {
                artifact_id: format!("parse:{}:{}", self.path, self.content_sha256),
                path: self.path.clone(),
                language: self.language,
                source_kind,
                stage: ParseArtifactStage::Parsed,
                content_sha256: self.content_sha256,
                content_bytes: self.content_bytes,
                parser_pack_id: self.parser_pack_id,
                facts: FileFactsSummary {
                    symbol_count: 0,
                    occurrence_count: 0,
                    edge_count: 0,
                },
                diagnostics: self.diagnostics,
            },
            parse_succeeded: self.parse_succeeded,
            has_recoverable_errors,
            reused_incremental_tree: false,
            line_count: self.line_count,
            root: self.root,
        };
        ParseStoreFileRecord {
            path: inspection.artifact.path.clone(),
            cache_id: self.cache_id,
            reused_from_cache: true,
            inspection,
        }
    }
}

fn parser_config_digest(options: &ParseManagerOptions) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"phase4-parse-manager-v1\n");
    hasher.update(format!("max_file_bytes:{}\n", options.rules.max_file_bytes));
    hasher.update(format!(
        "diagnostics_max_per_file:{}\n",
        options.diagnostics_max_per_file
    ));
    hasher.update(format!(
        "exclude_vendor_paths:{}\n",
        options.rules.exclude_vendor_paths
    ));
    hasher.update(format!(
        "exclude_generated_paths:{}\n",
        options.rules.exclude_generated_paths
    ));
    hasher.update(format!(
        "exclude_binary_like_contents:{}\n",
        options.rules.exclude_binary_like_contents
    ));
    let mut ignore_patterns = options.rules.ignore_patterns.clone();
    ignore_patterns.sort();
    for pattern in ignore_patterns {
        hasher.update(format!("ignore:{pattern}\n"));
    }
    let mut enabled_languages = options
        .rules
        .enabled_languages
        .iter()
        .map(|language| format!("{language:?}"))
        .collect::<Vec<_>>();
    enabled_languages.sort();
    for language in enabled_languages {
        hasher.update(format!("language:{language}\n"));
    }
    format!("{:x}", hasher.finalize())
}

fn cache_id_for(
    parser_config_digest: &str,
    file: &crate::snapshot_catalog::ResolvedParseFile,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "{}\n{}\n{}\n{}\n",
        parser_config_digest,
        file.path,
        file.language.as_str(),
        file.content_sha256
    ));
    format!("{:x}", hasher.finalize())
}

fn build_id_for(snapshot_id: &str, parser_config_digest: &str) -> String {
    let short = &parser_config_digest[..12.min(parser_config_digest.len())];
    format!("parse-{snapshot_id}-{short}")
}

fn epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, BufferOverlay, ComposedSnapshot, OverlayEntryKind,
        SnapshotFile, WorkingTreeEntry, WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::ParseInputSourceKind;
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use super::{ParseManager, ParseManagerOptions};
    use crate::snapshot_catalog::ParseEligibilityRules;

    fn snapshot_with_files(
        snapshot_id: &str,
        base_files: Vec<(&str, &str)>,
        working_entries: Vec<WorkingTreeEntry>,
        buffers: Vec<BufferOverlay>,
    ) -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: snapshot_id.to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "abc123".to_string(),
                digest: "base".to_string(),
                file_count: base_files.len(),
                files: base_files
                    .into_iter()
                    .map(|(path, contents)| SnapshotFile {
                        path: path.to_string(),
                        content_sha256: format!("sha-{path}"),
                        content_bytes: contents.len(),
                        contents: contents.to_string(),
                    })
                    .collect(),
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: working_entries,
            },
            buffers,
        }
    }

    #[test]
    fn builds_parse_artifacts_deterministically() {
        let tempdir = tempdir().unwrap();
        let snapshot = snapshot_with_files(
            "snap-deterministic",
            vec![
                ("src/zeta.ts", "export const zeta = 1;"),
                ("src/alpha.ts", "export const alpha = 1;"),
                ("README.md", "ignored"),
            ],
            Vec::new(),
            Vec::new(),
        );
        let mut manager = ParseManager::new(ParseManagerOptions {
            store_root: tempdir.path().join("parse"),
            rules: ParseEligibilityRules::default(),
            diagnostics_max_per_file: 32,
        });

        let build = manager.build_snapshot(&snapshot, false).unwrap();

        assert_eq!(
            build
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            vec!["src/alpha.ts".to_string(), "src/zeta.ts".to_string()]
        );
        assert_eq!(build.stats.parsed_file_count, 2);
        assert_eq!(build.stats.reused_file_count, 0);
        assert_eq!(build.stats.skipped_file_count, 1);
        assert!(
            std::path::Path::new(&build.artifact_root)
                .join("build.json")
                .exists()
        );
    }

    #[test]
    fn buffer_overlays_change_parse_inputs() {
        let tempdir = tempdir().unwrap();
        let snapshot = snapshot_with_files(
            "snap-buffer",
            vec![("src/app.ts", "export const value = 1;")],
            vec![WorkingTreeEntry {
                path: "src/app.ts".to_string(),
                kind: OverlayEntryKind::Upsert,
                content_sha256: Some("sha-work".to_string()),
                content_bytes: Some(28),
                contents: Some("export const value = work();".to_string()),
            }],
            vec![BufferOverlay {
                buffer_id: "buffer-1".to_string(),
                path: "src/app.ts".to_string(),
                version: 4,
                content_sha256: "sha-buffer".to_string(),
                content_bytes: 44,
                contents: "export const value = <div>{label</div>;".to_string(),
            }],
        );
        let mut manager = ParseManager::new(ParseManagerOptions {
            store_root: tempdir.path().join("parse"),
            rules: ParseEligibilityRules::default(),
            diagnostics_max_per_file: 32,
        });

        let record = manager
            .inspect_file(&snapshot, "src/app.ts")
            .unwrap()
            .unwrap();

        assert_eq!(
            record.inspection.artifact.source_kind,
            ParseInputSourceKind::BufferOverlay
        );
        assert!(!record.inspection.artifact.diagnostics.is_empty());
    }

    #[test]
    fn warm_reuse_uses_persistent_cache_for_unchanged_files() {
        let tempdir = tempdir().unwrap();
        let store_root = tempdir.path().join("parse");
        let snapshot_a = snapshot_with_files(
            "snap-a",
            vec![("src/app.ts", "export const value = 1;")],
            Vec::new(),
            Vec::new(),
        );
        let snapshot_b = snapshot_with_files(
            "snap-b",
            vec![("src/app.ts", "export const value = 1;")],
            Vec::new(),
            Vec::new(),
        );

        let mut first = ParseManager::new(ParseManagerOptions {
            store_root: store_root.clone(),
            rules: ParseEligibilityRules::default(),
            diagnostics_max_per_file: 32,
        });
        let first_build = first.build_snapshot(&snapshot_a, false).unwrap();

        let mut second = ParseManager::new(ParseManagerOptions {
            store_root,
            rules: ParseEligibilityRules::default(),
            diagnostics_max_per_file: 32,
        });
        let second_build = second.build_snapshot(&snapshot_b, false).unwrap();

        assert_eq!(first_build.stats.parsed_file_count, 1);
        assert_eq!(second_build.stats.parsed_file_count, 0);
        assert_eq!(second_build.stats.reused_file_count, 1);
        assert!(second_build.files[0].reused_from_cache);
    }

    #[test]
    fn corrupted_cache_entries_are_discarded_and_rebuilt() {
        let tempdir = tempdir().unwrap();
        let store_root = tempdir.path().join("parse");
        let snapshot = snapshot_with_files(
            "snap-cache-corrupt",
            vec![("src/app.ts", "export const value = 1;")],
            Vec::new(),
            Vec::new(),
        );

        let mut first = ParseManager::new(ParseManagerOptions {
            store_root: store_root.clone(),
            rules: ParseEligibilityRules::default(),
            diagnostics_max_per_file: 32,
        });
        let build = first.build_snapshot(&snapshot, false).unwrap();
        let cache_path = store_root
            .join("_cache")
            .join(format!("{}.json", build.files[0].cache_id));
        fs::write(&cache_path, "{not json").unwrap();

        let mut second = ParseManager::new(ParseManagerOptions {
            store_root,
            rules: ParseEligibilityRules::default(),
            diagnostics_max_per_file: 32,
        });
        let rebuilt = second.build_snapshot(&snapshot, false).unwrap();

        assert_eq!(rebuilt.stats.parsed_file_count, 1);
        assert_eq!(rebuilt.stats.reused_file_count, 0);
    }

    #[test]
    fn corrupted_build_status_is_discarded_and_recomputed() {
        let tempdir = tempdir().unwrap();
        let store_root = tempdir.path().join("parse");
        let snapshot = snapshot_with_files(
            "snap-build-corrupt",
            vec![("src/app.ts", "export const value = 1;")],
            Vec::new(),
            Vec::new(),
        );

        let mut first = ParseManager::new(ParseManagerOptions {
            store_root: store_root.clone(),
            rules: ParseEligibilityRules::default(),
            diagnostics_max_per_file: 32,
        });
        let build = first.build_snapshot(&snapshot, false).unwrap();
        let build_path = std::path::Path::new(&build.artifact_root).join("build.json");
        fs::write(&build_path, "{not json").unwrap();

        let mut second = ParseManager::new(ParseManagerOptions {
            store_root,
            rules: ParseEligibilityRules::default(),
            diagnostics_max_per_file: 32,
        });
        let rebuilt = second.build_snapshot(&snapshot, false).unwrap();

        assert!(!rebuilt.loaded_from_existing_build);
        assert_eq!(rebuilt.stats.planned_file_count, 1);
    }
}
