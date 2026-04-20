#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreMatcher {
    patterns: Vec<String>,
}

impl IgnoreMatcher {
    pub fn new(patterns: Vec<String>) -> Self {
        Self {
            patterns: patterns.into_iter().map(normalize_pattern).collect(),
        }
    }

    pub fn is_ignored(&self, relative_path: &str) -> bool {
        let normalized_path = normalize_path(relative_path);
        let file_name = normalized_path
            .rsplit('/')
            .next()
            .unwrap_or(&normalized_path);
        is_safe_temp_name(file_name)
            || self
                .patterns
                .iter()
                .any(|pattern| wildcard_matches(pattern, &normalized_path))
    }

    pub fn should_skip_dir(&self, relative_path: &str) -> bool {
        let normalized_path = normalize_path(relative_path);
        self.is_ignored(&normalized_path)
            || self.patterns.iter().any(|pattern| {
                wildcard_matches(pattern, &normalized_path)
                    || wildcard_matches(pattern, &format!("{normalized_path}/"))
            })
    }
}

fn normalize_pattern(pattern: String) -> String {
    pattern.replace('\\', "/")
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn is_safe_temp_name(name: &str) -> bool {
    name == ".DS_Store"
        || name.starts_with(".#")
        || (name.starts_with('#') && name.ends_with('#'))
        || name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".swo")
        || name.ends_with(".swx")
        || name.ends_with(".tmp")
        || name.ends_with(".temp")
}

fn wildcard_matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut pattern_index, mut text_index) = (0usize, 0usize);
    let mut star_index = None;
    let mut match_index = 0usize;

    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            match_index = text_index;
        } else if let Some(index) = star_index {
            pattern_index = index + 1;
            match_index += 1;
            text_index = match_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::IgnoreMatcher;

    #[test]
    fn matcher_respects_patterns_and_temp_files() {
        let matcher = IgnoreMatcher::new(vec!["ignored/**".to_string(), "*.log".to_string()]);
        assert!(matcher.is_ignored("ignored/example.txt"));
        assert!(matcher.is_ignored("service.log"));
        assert!(matcher.is_ignored(".#scratch"));
        assert!(!matcher.is_ignored("src/app.ts"));
    }
}
