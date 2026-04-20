use std::path::{Path, PathBuf};
use std::process::Command;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::snapshot::{BaseSnapshot, BaseSnapshotKind, SnapshotFile};

pub fn build_base_snapshot(repo_root: &Path, commit: &str) -> HyperindexResult<BaseSnapshot> {
    let repo_root = canonicalize(repo_root)?;
    let files = git_stdout_lines(&repo_root, &["ls-tree", "-r", "--name-only", commit])?
        .into_iter()
        .map(|path| build_base_file(&repo_root, commit, &path))
        .collect::<HyperindexResult<Vec<_>>>()?;
    let digest = digest_snapshot_components(
        std::iter::once(commit.to_string()).chain(
            files
                .iter()
                .map(|file| format!("{}:{}", file.path, file.content_sha256)),
        ),
    );
    Ok(BaseSnapshot {
        kind: BaseSnapshotKind::GitCommit,
        commit: commit.to_string(),
        digest,
        file_count: files.len(),
        files,
    })
}

fn build_base_file(repo_root: &Path, commit: &str, path: &str) -> HyperindexResult<SnapshotFile> {
    let spec = format!("{commit}:{path}");
    let contents = git_stdout(repo_root, &["show", &spec])?;
    Ok(SnapshotFile {
        path: path.to_string(),
        content_sha256: sha256_hex(contents.as_bytes()),
        content_bytes: contents.len(),
        contents,
    })
}

fn canonicalize(path: &Path) -> HyperindexResult<PathBuf> {
    std::fs::canonicalize(path).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to canonicalize {}: {error}",
            path.display()
        ))
    })
}

fn git_stdout_lines(repo_root: &Path, args: &[&str]) -> HyperindexResult<Vec<String>> {
    Ok(git_stdout(repo_root, args)?
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn git_stdout(repo_root: &Path, args: &[&str]) -> HyperindexResult<String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.quotepath=false")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .map_err(|error| HyperindexError::Message(format!("git invocation failed: {error}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(HyperindexError::Message(format!(
            "git {} failed: {}",
            args.join(" "),
            if stderr.is_empty() {
                "unknown error".to_string()
            } else {
                stderr
            }
        )));
    }

    String::from_utf8(output.stdout)
        .map(|raw| raw.trim_end().to_string())
        .map_err(|error| {
            HyperindexError::Message(format!("git output was not utf-8 text: {error}"))
        })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn digest_snapshot_components<I>(components: I) -> String
where
    I: IntoIterator<Item = String>,
{
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    for component in components {
        hasher.update(component.as_bytes());
        hasher.update(b"\n");
    }
    format!("snap-{}", hex::encode(hasher.finalize()))
}
