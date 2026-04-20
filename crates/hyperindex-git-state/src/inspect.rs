use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::repo::WorkingTreeSummary;

use crate::status::parse_porcelain_v1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepoState {
    pub repo_root: PathBuf,
    pub branch: Option<String>,
    pub head_commit: Option<String>,
    pub working_tree: WorkingTreeSummary,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default)]
pub struct GitInspector;

impl GitInspector {
    pub fn inspect_repo(&self, path: impl Into<PathBuf>) -> HyperindexResult<GitRepoState> {
        let path = path.into();
        let resolved_path = resolve_existing_path(&path)?;
        let probe_path = if resolved_path.is_file() {
            resolved_path
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    HyperindexError::Message(format!(
                        "failed to derive parent directory for {}",
                        resolved_path.display()
                    ))
                })?
        } else {
            resolved_path
        };

        match git_stdout(&probe_path, &["rev-parse", "--show-toplevel"]) {
            Ok(root) => {
                let repo_root = PathBuf::from(root);
                let branch =
                    git_stdout(&repo_root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok();
                let head_commit = git_stdout(&repo_root, &["rev-parse", "HEAD"]).ok();
                let raw_status = git_stdout(
                    &repo_root,
                    &[
                        "status",
                        "--porcelain=v1",
                        "--ignored=matching",
                        "--untracked-files=all",
                        "--find-renames",
                    ],
                )
                .unwrap_or_default();
                let working_tree =
                    parse_porcelain_v1(&raw_status).into_summary(head_commit.as_deref());
                Ok(GitRepoState {
                    repo_root,
                    branch,
                    head_commit,
                    working_tree,
                    warnings: Vec::new(),
                })
            }
            Err(_) => Ok(GitRepoState {
                repo_root: probe_path,
                branch: None,
                head_commit: None,
                working_tree: parse_porcelain_v1("").into_summary(None),
                warnings: vec!["git metadata unavailable for this path".to_string()],
            }),
        }
    }
}

fn resolve_existing_path(path: &Path) -> HyperindexResult<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| HyperindexError::Message(format!("current_dir failed: {error}")))?
            .join(path)
    };

    if !absolute.exists() {
        return Err(HyperindexError::Message(format!(
            "path does not exist: {}",
            absolute.display()
        )));
    }

    fs::canonicalize(&absolute).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to canonicalize {}: {error}",
            absolute.display()
        ))
    })
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

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use tempfile::tempdir;

    use super::GitInspector;

    #[test]
    fn inspect_repo_reports_clean_repo() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);
        fs::write(repo_root.join("tracked.txt"), "initial\n").unwrap();
        run_git(&repo_root, &["add", "."]);
        commit_all(&repo_root, "initial");

        let state = GitInspector.inspect_repo(&repo_root).unwrap();

        assert_eq!(state.repo_root, repo_root.canonicalize().unwrap());
        assert_eq!(state.branch.as_deref(), Some("trunk"));
        assert!(state.head_commit.is_some());
        assert!(state.working_tree.dirty_tracked_files.is_empty());
        assert!(state.working_tree.untracked_files.is_empty());
        assert!(state.working_tree.deleted_files.is_empty());
        assert!(state.working_tree.renamed_files.is_empty());
        assert!(state.working_tree.ignored_files.is_empty());
        assert!(!state.working_tree.digest.is_empty());
    }

    #[test]
    fn inspect_repo_reports_modified_untracked_deleted_and_ignored_files() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);
        fs::write(repo_root.join("tracked.txt"), "initial\n").unwrap();
        fs::write(repo_root.join("delete-me.txt"), "remove me\n").unwrap();
        fs::write(repo_root.join(".gitignore"), "ignored.log\n").unwrap();
        run_git(&repo_root, &["add", "."]);
        commit_all(&repo_root, "initial");

        fs::write(repo_root.join("tracked.txt"), "changed\n").unwrap();
        fs::write(repo_root.join("new.txt"), "hello\n").unwrap();
        fs::write(repo_root.join("ignored.log"), "ignore me\n").unwrap();
        fs::remove_file(repo_root.join("delete-me.txt")).unwrap();

        let state = GitInspector.inspect_repo(&repo_root).unwrap();

        assert_eq!(state.working_tree.dirty_tracked_files, vec!["tracked.txt"]);
        assert_eq!(state.working_tree.untracked_files, vec!["new.txt"]);
        assert_eq!(state.working_tree.deleted_files, vec!["delete-me.txt"]);
        assert_eq!(state.working_tree.ignored_files, vec!["ignored.log"]);
    }

    #[test]
    fn inspect_repo_reports_practical_branch_changes() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);
        fs::write(repo_root.join("tracked.txt"), "initial\n").unwrap();
        run_git(&repo_root, &["add", "."]);
        commit_all(&repo_root, "initial");
        run_git(&repo_root, &["checkout", "-b", "feature/branch-status"]);

        let state = GitInspector.inspect_repo(&repo_root).unwrap();

        assert_eq!(state.branch.as_deref(), Some("feature/branch-status"));
    }

    #[test]
    fn inspect_repo_reports_practical_renames() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("repo");
        init_repo(&repo_root);
        fs::write(repo_root.join("old.txt"), "initial\n").unwrap();
        run_git(&repo_root, &["add", "."]);
        commit_all(&repo_root, "initial");
        run_git(&repo_root, &["mv", "old.txt", "new.txt"]);

        let state = GitInspector.inspect_repo(&repo_root).unwrap();

        assert_eq!(state.working_tree.renamed_files.len(), 1);
        assert_eq!(state.working_tree.renamed_files[0].from, "old.txt");
        assert_eq!(state.working_tree.renamed_files[0].to, "new.txt");
    }

    #[test]
    fn inspect_repo_resolves_nested_git_paths() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("repo");
        fs::create_dir_all(repo_root.join("src")).unwrap();
        init_repo(&repo_root);
        fs::write(repo_root.join("src/lib.ts"), "export const answer = 42;\n").unwrap();
        run_git(&repo_root, &["add", "."]);
        commit_all(&repo_root, "initial");

        let state = GitInspector.inspect_repo(repo_root.join("src")).unwrap();

        assert_eq!(state.repo_root, repo_root.canonicalize().unwrap());
        assert_eq!(state.branch.as_deref(), Some("trunk"));
        assert!(state.head_commit.is_some());
        assert!(state.warnings.is_empty());
    }

    #[test]
    fn inspect_repo_returns_warning_for_non_git_dirs() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("plain");
        fs::create_dir_all(&repo_root).unwrap();

        let state = GitInspector.inspect_repo(&repo_root).unwrap();

        assert_eq!(state.repo_root, repo_root.canonicalize().unwrap());
        assert_eq!(state.branch, None);
        assert_eq!(state.head_commit, None);
        assert!(
            state.working_tree.dirty_tracked_files.is_empty()
                && state.working_tree.untracked_files.is_empty()
        );
        assert_eq!(
            state.warnings,
            vec!["git metadata unavailable for this path"]
        );
    }

    fn init_repo(repo_root: &Path) {
        fs::create_dir_all(repo_root).unwrap();
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["checkout", "-b", "trunk"]);
    }

    fn commit_all(repo_root: &Path, message: &str) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .arg("commit")
            .arg("-m")
            .arg(message)
            .env("GIT_AUTHOR_NAME", "Codex")
            .env("GIT_AUTHOR_EMAIL", "codex@example.com")
            .env("GIT_COMMITTER_NAME", "Codex")
            .env("GIT_COMMITTER_EMAIL", "codex@example.com")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_git(repo_root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
