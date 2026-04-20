use std::path::{Path, PathBuf};

pub fn normalize_repo_path(repo_root: &Path, candidate: &Path) -> PathBuf {
    if candidate.is_absolute() {
        candidate
            .strip_prefix(repo_root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| candidate.to_path_buf())
    } else {
        candidate.to_path_buf()
    }
}
