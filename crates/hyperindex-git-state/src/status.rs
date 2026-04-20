use hyperindex_protocol::repo::{RepoRenamedPath, WorkingTreeSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedWorkingTreeStatus {
    pub dirty_tracked_files: Vec<String>,
    pub untracked_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub renamed_files: Vec<RepoRenamedPath>,
    pub ignored_files: Vec<String>,
}

impl ParsedWorkingTreeStatus {
    pub fn into_summary(self, head_commit: Option<&str>) -> WorkingTreeSummary {
        WorkingTreeSummary {
            digest: working_tree_digest(head_commit, &self),
            dirty_tracked_files: self.dirty_tracked_files,
            untracked_files: self.untracked_files,
            deleted_files: self.deleted_files,
            renamed_files: self.renamed_files,
            ignored_files: self.ignored_files,
        }
    }

    pub fn dirty_path_count(&self) -> usize {
        self.dirty_tracked_files.len()
            + self.untracked_files.len()
            + self.deleted_files.len()
            + self.renamed_files.len()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty_path_count() > 0
    }
}

pub fn parse_porcelain_v1(raw: &str) -> ParsedWorkingTreeStatus {
    let mut status = ParsedWorkingTreeStatus {
        dirty_tracked_files: Vec::new(),
        untracked_files: Vec::new(),
        deleted_files: Vec::new(),
        renamed_files: Vec::new(),
        ignored_files: Vec::new(),
    };

    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        if let Some(path) = line.strip_prefix("?? ") {
            status.untracked_files.push(normalize_git_path(path));
            continue;
        }
        if let Some(path) = line.strip_prefix("!! ") {
            status.ignored_files.push(normalize_git_path(path));
            continue;
        }
        if line.len() < 4 {
            continue;
        }

        let code = &line[..2];
        let payload = line.get(3..).unwrap_or_default().trim_start();
        let x = code.as_bytes()[0] as char;
        let y = code.as_bytes()[1] as char;

        if (x == 'R' || y == 'R') && payload.contains(" -> ") {
            let mut parts = payload.splitn(2, " -> ");
            let from = normalize_git_path(parts.next().unwrap_or_default());
            let to = normalize_git_path(parts.next().unwrap_or_default());
            status.renamed_files.push(RepoRenamedPath { from, to });
            continue;
        }

        if x == 'D' || y == 'D' {
            status.deleted_files.push(normalize_git_path(payload));
            continue;
        }

        if !code.trim().is_empty() {
            status.dirty_tracked_files.push(normalize_git_path(payload));
        }
    }

    status.dirty_tracked_files.sort();
    status.dirty_tracked_files.dedup();
    status.untracked_files.sort();
    status.untracked_files.dedup();
    status.deleted_files.sort();
    status.deleted_files.dedup();
    status.renamed_files.sort();
    status.renamed_files.dedup();
    status.ignored_files.sort();
    status.ignored_files.dedup();

    status
}

fn normalize_git_path(path: &str) -> String {
    path.trim_matches('"').replace('\\', "/")
}

fn working_tree_digest(head_commit: Option<&str>, status: &ParsedWorkingTreeStatus) -> String {
    let mut lines = Vec::new();
    lines.push(format!("head:{}", head_commit.unwrap_or("-")));
    lines.extend(
        status
            .dirty_tracked_files
            .iter()
            .map(|path| format!("dirty:{path}")),
    );
    lines.extend(
        status
            .untracked_files
            .iter()
            .map(|path| format!("untracked:{path}")),
    );
    lines.extend(
        status
            .deleted_files
            .iter()
            .map(|path| format!("deleted:{path}")),
    );
    lines.extend(
        status
            .renamed_files
            .iter()
            .map(|rename| format!("renamed:{}=>{}", rename.from, rename.to)),
    );
    lines.extend(
        status
            .ignored_files
            .iter()
            .map(|path| format!("ignored:{path}")),
    );
    let joined = lines.join("\n");
    format!("gitwt-{:016x}", stable_hash(joined.as_bytes()))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::parse_porcelain_v1;

    #[test]
    fn parse_porcelain_v1_groups_git_changes() {
        let status = parse_porcelain_v1(
            " M src/app.ts\n?? scratch.txt\n D old.txt\nR  src/old.ts -> src/new.ts\n!! dist/out.js\n",
        );

        assert_eq!(status.dirty_tracked_files, vec!["src/app.ts"]);
        assert_eq!(status.untracked_files, vec!["scratch.txt"]);
        assert_eq!(status.deleted_files, vec!["old.txt"]);
        assert_eq!(status.renamed_files.len(), 1);
        assert_eq!(status.renamed_files[0].from, "src/old.ts");
        assert_eq!(status.renamed_files[0].to, "src/new.ts");
        assert_eq!(status.ignored_files, vec!["dist/out.js"]);
        assert!(status.is_dirty());
    }

    #[test]
    fn digest_is_deterministic_for_equivalent_status() {
        let a = parse_porcelain_v1("?? b.txt\n M a.txt\n");
        let b = parse_porcelain_v1(" M a.txt\n?? b.txt\n");
        assert_eq!(
            a.clone().into_summary(Some("abc123")).digest,
            b.into_summary(Some("abc123")).digest
        );
    }
}
