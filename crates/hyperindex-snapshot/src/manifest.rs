use std::collections::{BTreeMap, BTreeSet};

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::snapshot::{
    BaseSnapshot, BufferOverlay, ComposedSnapshot, SnapshotDiffResponse, WorkingTreeOverlay,
};
use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

use crate::base::digest_snapshot_components;
use crate::overlays::{base_file_map, materialize_with_buffers, materialize_without_buffers};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedFrom {
    BufferOverlay(String),
    WorkingTreeOverlay,
    BaseSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFile {
    pub path: String,
    pub resolved_from: ResolvedFrom,
    pub contents: String,
}

#[derive(Debug, Default)]
pub struct SnapshotAssembler;

impl SnapshotAssembler {
    pub fn compose(
        &self,
        repo_id: &str,
        repo_root: &str,
        base: BaseSnapshot,
        working_tree: WorkingTreeOverlay,
        buffers: Vec<BufferOverlay>,
    ) -> HyperindexResult<ComposedSnapshot> {
        validate_snapshot_inputs(&base, &working_tree, &buffers)?;
        let snapshot_id =
            deterministic_snapshot_id(repo_id, repo_root, &base, &working_tree, &buffers);
        Ok(ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id,
            repo_id: repo_id.to_string(),
            repo_root: repo_root.to_string(),
            base,
            working_tree,
            buffers,
        })
    }

    pub fn resolve_file(&self, snapshot: &ComposedSnapshot, path: &str) -> Option<ResolvedFile> {
        for buffer in &snapshot.buffers {
            if buffer.path == path {
                return Some(ResolvedFile {
                    path: path.to_string(),
                    resolved_from: ResolvedFrom::BufferOverlay(buffer.buffer_id.clone()),
                    contents: buffer.contents.clone(),
                });
            }
        }

        for entry in &snapshot.working_tree.entries {
            if entry.path == path {
                return entry.contents.as_ref().map(|contents| ResolvedFile {
                    path: path.to_string(),
                    resolved_from: ResolvedFrom::WorkingTreeOverlay,
                    contents: contents.clone(),
                });
            }
        }

        snapshot
            .base
            .files
            .iter()
            .find(|file| file.path == path)
            .map(|file| ResolvedFile {
                path: path.to_string(),
                resolved_from: ResolvedFrom::BaseSnapshot,
                contents: file.contents.clone(),
            })
    }

    pub fn diff(&self, left: &ComposedSnapshot, right: &ComposedSnapshot) -> SnapshotDiffResponse {
        let left_base = base_file_map(&left.base.files);
        let right_base = base_file_map(&right.base.files);
        let left_without_buffers = materialize_without_buffers(&left_base, &left.working_tree);
        let right_without_buffers = materialize_without_buffers(&right_base, &right.working_tree);
        let left_full = materialize_with_buffers(&left_base, &left.working_tree, &left.buffers);
        let right_full = materialize_with_buffers(&right_base, &right.working_tree, &right.buffers);

        let paths = all_paths(&left_full, &right_full);
        let mut changed_paths = Vec::new();
        let mut added_paths = Vec::new();
        let mut deleted_paths = Vec::new();
        let mut buffer_only_changed_paths = Vec::new();

        for path in paths {
            let left_contents = left_full.get(&path);
            let right_contents = right_full.get(&path);
            if left_contents == right_contents {
                continue;
            }

            if left_contents.is_none() {
                added_paths.push(path.clone());
            } else if right_contents.is_none() {
                deleted_paths.push(path.clone());
            }
            changed_paths.push(path.clone());

            if left_without_buffers.get(&path) == right_without_buffers.get(&path) {
                buffer_only_changed_paths.push(path);
            }
        }

        SnapshotDiffResponse {
            left_snapshot_id: left.snapshot_id.clone(),
            right_snapshot_id: right.snapshot_id.clone(),
            changed_paths,
            added_paths,
            deleted_paths,
            buffer_only_changed_paths,
        }
    }
}

fn validate_snapshot_inputs(
    base: &BaseSnapshot,
    working_tree: &WorkingTreeOverlay,
    buffers: &[BufferOverlay],
) -> HyperindexResult<()> {
    ensure_unique_paths(base.files.iter().map(|file| file.path.as_str()), "base")?;
    ensure_unique_paths(
        working_tree.entries.iter().map(|entry| entry.path.as_str()),
        "working_tree",
    )?;
    ensure_unique_paths(buffers.iter().map(|buffer| buffer.path.as_str()), "buffers")?;
    Ok(())
}

fn ensure_unique_paths<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    area: &str,
) -> HyperindexResult<()> {
    let mut seen = BTreeSet::new();
    for path in paths {
        if !seen.insert(path.to_string()) {
            return Err(HyperindexError::Message(format!(
                "duplicate path in {area}: {path}"
            )));
        }
    }
    Ok(())
}

fn deterministic_snapshot_id(
    repo_id: &str,
    repo_root: &str,
    base: &BaseSnapshot,
    working_tree: &WorkingTreeOverlay,
    buffers: &[BufferOverlay],
) -> String {
    digest_snapshot_components(
        std::iter::once(format!("repo:{repo_id}"))
            .chain(std::iter::once(format!("root:{repo_root}")))
            .chain(std::iter::once(format!("base:{}", base.digest)))
            .chain(std::iter::once(format!(
                "working_tree:{}",
                working_tree.digest
            )))
            .chain(buffers.iter().map(|buffer| {
                format!(
                    "buffer:{}:{}:{}:{}",
                    buffer.buffer_id, buffer.path, buffer.version, buffer.content_sha256
                )
            })),
    )
}

fn all_paths(left: &BTreeMap<String, String>, right: &BTreeMap<String, String>) -> Vec<String> {
    let mut paths = left.keys().cloned().collect::<BTreeSet<_>>();
    paths.extend(right.keys().cloned());
    paths.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, BufferOverlay, OverlayEntryKind, SnapshotFile,
        WorkingTreeEntry, WorkingTreeOverlay,
    };

    use super::{ResolvedFrom, SnapshotAssembler};

    #[test]
    fn precedence_rules_are_buffer_then_working_tree_then_base() {
        let assembler = SnapshotAssembler;
        let snapshot = assembler
            .compose(
                "repo-1",
                "/tmp/repo",
                base_snapshot(vec![("src/app.ts", "base")]),
                WorkingTreeOverlay {
                    digest: "work".to_string(),
                    entries: vec![WorkingTreeEntry {
                        path: "src/app.ts".to_string(),
                        kind: OverlayEntryKind::Upsert,
                        content_sha256: Some("sha-work".to_string()),
                        content_bytes: Some(4),
                        contents: Some("work".to_string()),
                    }],
                },
                vec![BufferOverlay {
                    buffer_id: "buffer-1".to_string(),
                    path: "src/app.ts".to_string(),
                    version: 2,
                    content_sha256: "sha-buffer".to_string(),
                    content_bytes: 6,
                    contents: "buffer".to_string(),
                }],
            )
            .unwrap();

        let resolved = assembler.resolve_file(&snapshot, "src/app.ts").unwrap();
        assert_eq!(resolved.contents, "buffer");
        assert_eq!(
            resolved.resolved_from,
            ResolvedFrom::BufferOverlay("buffer-1".to_string())
        );
    }

    #[test]
    fn snapshot_identity_is_deterministic() {
        let assembler = SnapshotAssembler;
        let left = assembler
            .compose(
                "repo-1",
                "/tmp/repo",
                base_snapshot(vec![("a.ts", "one")]),
                WorkingTreeOverlay {
                    digest: "work".to_string(),
                    entries: Vec::new(),
                },
                Vec::new(),
            )
            .unwrap();
        let right = assembler
            .compose(
                "repo-1",
                "/tmp/repo",
                base_snapshot(vec![("a.ts", "one")]),
                WorkingTreeOverlay {
                    digest: "work".to_string(),
                    entries: Vec::new(),
                },
                Vec::new(),
            )
            .unwrap();
        assert_eq!(left.snapshot_id, right.snapshot_id);
    }

    #[test]
    fn diff_reports_added_deleted_changed_and_buffer_only_paths() {
        let assembler = SnapshotAssembler;
        let left = assembler
            .compose(
                "repo-1",
                "/tmp/repo",
                base_snapshot(vec![("same.ts", "one"), ("deleted.ts", "gone")]),
                WorkingTreeOverlay {
                    digest: "work-left".to_string(),
                    entries: Vec::new(),
                },
                vec![BufferOverlay {
                    buffer_id: "buffer-1".to_string(),
                    path: "same.ts".to_string(),
                    version: 1,
                    content_sha256: "sha".to_string(),
                    content_bytes: 6,
                    contents: "buffer".to_string(),
                }],
            )
            .unwrap();
        let right = assembler
            .compose(
                "repo-1",
                "/tmp/repo",
                base_snapshot(vec![("same.ts", "one"), ("added.ts", "new")]),
                WorkingTreeOverlay {
                    digest: "work-right".to_string(),
                    entries: vec![WorkingTreeEntry {
                        path: "same.ts".to_string(),
                        kind: OverlayEntryKind::Upsert,
                        content_sha256: Some("sha".to_string()),
                        content_bytes: Some(3),
                        contents: Some("one".to_string()),
                    }],
                },
                Vec::new(),
            )
            .unwrap();

        let diff = assembler.diff(&left, &right);
        assert_eq!(diff.added_paths, vec!["added.ts"]);
        assert_eq!(diff.deleted_paths, vec!["deleted.ts"]);
        assert_eq!(diff.buffer_only_changed_paths, vec!["same.ts"]);
        assert_eq!(
            diff.changed_paths,
            vec!["added.ts", "deleted.ts", "same.ts"]
        );
    }

    fn base_snapshot(files: Vec<(&str, &str)>) -> BaseSnapshot {
        BaseSnapshot {
            kind: BaseSnapshotKind::GitCommit,
            commit: "abc123".to_string(),
            digest: "base-digest".to_string(),
            file_count: files.len(),
            files: files
                .into_iter()
                .map(|(path, contents)| SnapshotFile {
                    path: path.to_string(),
                    content_sha256: format!("sha-{path}"),
                    content_bytes: contents.len(),
                    contents: contents.to_string(),
                })
                .collect(),
        }
    }
}
