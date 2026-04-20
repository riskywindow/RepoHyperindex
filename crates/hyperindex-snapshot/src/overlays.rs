use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use hyperindex_core::{HyperindexError, HyperindexResult, normalize_repo_relative_path};
use hyperindex_git_state::GitRepoState;
use hyperindex_protocol::buffers::BufferContents;
use hyperindex_protocol::snapshot::{
    BufferOverlay, OverlayEntryKind, WorkingTreeEntry, WorkingTreeOverlay,
};

use crate::base::{digest_snapshot_components, sha256_hex};

pub fn build_working_tree_overlay(
    repo_root: &Path,
    git_state: &GitRepoState,
) -> HyperindexResult<WorkingTreeOverlay> {
    let mut entries = Vec::new();
    let mut upsert_paths = BTreeSet::new();
    let mut delete_paths = BTreeSet::new();

    for path in git_state
        .working_tree
        .dirty_tracked_files
        .iter()
        .chain(git_state.working_tree.untracked_files.iter())
    {
        if upsert_paths.insert(path.clone()) {
            entries.push(build_upsert_entry(repo_root, path)?);
        }
    }

    for path in &git_state.working_tree.deleted_files {
        if delete_paths.insert(path.clone()) {
            entries.push(WorkingTreeEntry {
                path: path.clone(),
                kind: OverlayEntryKind::Delete,
                content_sha256: None,
                content_bytes: None,
                contents: None,
            });
        }
    }

    for rename in &git_state.working_tree.renamed_files {
        if delete_paths.insert(rename.from.clone()) {
            entries.push(WorkingTreeEntry {
                path: rename.from.clone(),
                kind: OverlayEntryKind::Delete,
                content_sha256: None,
                content_bytes: None,
                contents: None,
            });
        }
        if upsert_paths.insert(rename.to.clone()) {
            entries.push(build_upsert_entry(repo_root, &rename.to)?);
        }
    }

    entries.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| kind_rank(&left.kind).cmp(&kind_rank(&right.kind)))
    });
    let digest = digest_snapshot_components(entries.iter().map(|entry| match entry.kind {
        OverlayEntryKind::Upsert => format!(
            "upsert:{}:{}",
            entry.path,
            entry.content_sha256.as_deref().unwrap_or("-")
        ),
        OverlayEntryKind::Delete => format!("delete:{}", entry.path),
    }));
    Ok(WorkingTreeOverlay { digest, entries })
}

pub fn build_buffer_overlays(buffers: &[BufferContents]) -> HyperindexResult<Vec<BufferOverlay>> {
    let mut seen_paths = BTreeSet::new();
    let mut overlays = Vec::new();
    let mut sorted = buffers.to_vec();
    sorted.sort_by(|left, right| {
        left.state
            .path
            .cmp(&right.state.path)
            .then_with(|| left.state.buffer_id.cmp(&right.state.buffer_id))
    });
    for buffer in sorted {
        let normalized_path = normalize_repo_relative_path(&buffer.state.path, "buffer overlay")?;
        if !seen_paths.insert(normalized_path.clone()) {
            return Err(HyperindexError::Message(format!(
                "duplicate buffer path in snapshot inputs: {normalized_path}; clear one overlay or use distinct repo-relative paths"
            )));
        }
        overlays.push(BufferOverlay {
            buffer_id: buffer.state.buffer_id,
            path: normalized_path,
            version: buffer.state.version,
            content_sha256: buffer.state.content_sha256,
            content_bytes: buffer.state.content_bytes,
            contents: buffer.contents,
        });
    }
    Ok(overlays)
}

pub fn materialize_without_buffers(
    base_files: &BTreeMap<String, String>,
    working_tree: &WorkingTreeOverlay,
) -> BTreeMap<String, String> {
    let mut files = base_files.clone();
    apply_working_tree(&mut files, working_tree);
    files
}

pub fn materialize_with_buffers(
    base_files: &BTreeMap<String, String>,
    working_tree: &WorkingTreeOverlay,
    buffers: &[BufferOverlay],
) -> BTreeMap<String, String> {
    let mut files = materialize_without_buffers(base_files, working_tree);
    for buffer in buffers {
        files.insert(buffer.path.clone(), buffer.contents.clone());
    }
    files
}

pub fn base_file_map(
    files: &[hyperindex_protocol::snapshot::SnapshotFile],
) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|file| (file.path.clone(), file.contents.clone()))
        .collect()
}

fn apply_working_tree(files: &mut BTreeMap<String, String>, working_tree: &WorkingTreeOverlay) {
    for entry in &working_tree.entries {
        match entry.kind {
            OverlayEntryKind::Upsert => {
                if let Some(contents) = &entry.contents {
                    files.insert(entry.path.clone(), contents.clone());
                }
            }
            OverlayEntryKind::Delete => {
                files.remove(&entry.path);
            }
        }
    }
}

fn build_upsert_entry(repo_root: &Path, path: &str) -> HyperindexResult<WorkingTreeEntry> {
    let absolute = repo_root.join(path);
    let contents = fs::read_to_string(&absolute).map_err(|error| {
        HyperindexError::Message(format!("failed to read {}: {error}", absolute.display()))
    })?;
    Ok(WorkingTreeEntry {
        path: path.to_string(),
        kind: OverlayEntryKind::Upsert,
        content_sha256: Some(sha256_hex(contents.as_bytes())),
        content_bytes: Some(contents.len()),
        contents: Some(contents),
    })
}

fn kind_rank(kind: &OverlayEntryKind) -> u8 {
    match kind {
        OverlayEntryKind::Upsert => 0,
        OverlayEntryKind::Delete => 1,
    }
}
