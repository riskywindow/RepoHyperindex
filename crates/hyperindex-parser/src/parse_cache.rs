use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::parse_core::ParseArtifact;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParseCacheKey {
    pub path: String,
    pub content_sha256: String,
}

impl ParseCacheKey {
    pub fn from_contents(path: &str, contents: &str) -> Self {
        let digest = Sha256::digest(contents.as_bytes());
        Self {
            path: path.to_string(),
            content_sha256: format!("{digest:x}"),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ParseCache {
    entries: BTreeMap<ParseCacheKey, ParseArtifact>,
    latest_by_path: BTreeMap<String, ParseCacheKey>,
}

impl ParseCache {
    pub fn upsert(&mut self, artifact: ParseArtifact) {
        let key = artifact.cache_key().clone();
        self.latest_by_path.insert(key.path.clone(), key.clone());
        self.entries.insert(key, artifact);
    }

    pub fn get(&self, key: &ParseCacheKey) -> Option<&ParseArtifact> {
        self.entries.get(key)
    }

    pub fn latest_for_path(&self, path: &str) -> Option<&ParseArtifact> {
        let key = self.latest_by_path.get(path)?;
        self.entries.get(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::symbols::{
        FileFactsSummary, FileParseArtifactMetadata, ParseArtifactStage, ParseInputSourceKind,
    };

    use crate::language_pack_ts_js::TsJsLanguage;
    use crate::line_index::LineIndex;
    use crate::parse_core::{AstNodeHandle, ParseArtifact, ParseCandidate, ParsedSyntaxTree};

    use super::{ParseCache, ParseCacheKey};

    #[test]
    fn cache_roundtrip_uses_content_identity_and_latest_path_key() {
        let key = ParseCacheKey::from_contents("src/app.ts", "export const value = 1;");
        let line_index = LineIndex::new("export const value = 1;");
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        parser.set_language(&language).unwrap();
        let tree = parser.parse("export const value = 1;", None).unwrap();
        let artifact = ParseArtifact::new_for_tests(
            ParseCandidate {
                path: "src/app.ts".to_string(),
                language: TsJsLanguage::TypeScript,
                source_kind: ParseInputSourceKind::BaseSnapshot,
                content_sha256: key.content_sha256.clone(),
                content_bytes: 23,
            },
            key.clone(),
            FileParseArtifactMetadata {
                artifact_id: "artifact:src/app.ts".to_string(),
                path: "src/app.ts".to_string(),
                language: hyperindex_protocol::symbols::LanguageId::Typescript,
                source_kind: ParseInputSourceKind::BaseSnapshot,
                stage: ParseArtifactStage::Parsed,
                content_sha256: key.content_sha256.clone(),
                content_bytes: 23,
                parser_pack_id: "ts_js_core".to_string(),
                facts: FileFactsSummary {
                    symbol_count: 0,
                    occurrence_count: 0,
                    edge_count: 0,
                },
                diagnostics: Vec::new(),
            },
            true,
            false,
            "export const value = 1;".to_string(),
            line_index.clone(),
            ParsedSyntaxTree::new(tree),
            AstNodeHandle::from_byte_range(
                "program",
                &line_index,
                0,
                23,
                false,
                false,
                false,
                1,
                1,
            ),
        );

        let mut cache = ParseCache::default();
        cache.upsert(artifact);

        let entry = cache.get(&key).unwrap();
        assert!(entry.parse_succeeded());
        assert_eq!(
            cache.latest_for_path("src/app.ts").unwrap().cache_key(),
            &key
        );
        assert_eq!(cache.len(), 1);
    }
}
