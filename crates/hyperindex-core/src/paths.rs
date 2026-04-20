use std::path::{Component, Path};

use crate::{HyperindexError, HyperindexResult};

pub fn normalize_repo_relative_path(path: &str, subject: &str) -> HyperindexResult<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(HyperindexError::Message(format!(
            "{subject} path is empty; use a repo-relative path such as src/app.ts"
        )));
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return Err(HyperindexError::Message(format!(
            "{subject} path must be repo-relative: {trimmed}; use a path like src/app.ts"
        )));
    }

    let mut normalized = Vec::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => normalized.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(HyperindexError::Message(format!(
                    "{subject} path cannot escape the repo root: {trimmed}; clear or replace the overlay with a repo-relative path"
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(HyperindexError::Message(format!(
                    "{subject} path must be repo-relative: {trimmed}; use a path like src/app.ts"
                )));
            }
        }
    }

    if normalized.is_empty() {
        return Err(HyperindexError::Message(format!(
            "{subject} path is empty; use a repo-relative path such as src/app.ts"
        )));
    }

    Ok(normalized.join("/"))
}

#[cfg(test)]
mod tests {
    use super::normalize_repo_relative_path;

    #[test]
    fn normalizes_repo_relative_paths() {
        let normalized = normalize_repo_relative_path("./src/../src/app.ts", "buffer").unwrap_err();
        assert!(
            normalized
                .to_string()
                .contains("cannot escape the repo root")
        );

        let normalized = normalize_repo_relative_path("src//app.ts", "buffer").unwrap();
        assert_eq!(normalized, "src/app.ts");
    }
}
