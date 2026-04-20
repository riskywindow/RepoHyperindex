use std::collections::BTreeSet;

use hyperindex_protocol::semantic::{
    SemanticChunkKind, SemanticChunkRecord, SemanticQueryText, SemanticRerankMode,
    SemanticRerankSignal, SemanticRetrievalExplanation, SemanticRetrievalHit,
};
use hyperindex_protocol::symbols::SymbolKind;

#[derive(Debug, Default, Clone)]
pub struct SemanticReranker;

impl SemanticReranker {
    pub fn build_hit(
        &self,
        query: &SemanticQueryText,
        chunk: &SemanticChunkRecord,
        semantic_score: u32,
        mode: SemanticRerankMode,
    ) -> SemanticRetrievalHit {
        let query_terms = ordered_terms(&query.text);
        let text_hits = overlap_terms(&query_terms, &chunk.serialized_text);
        let path_hits = overlap_terms(&query_terms, &chunk.metadata.path);
        let package_hits = overlap_terms(
            &query_terms,
            &[
                chunk.metadata.package_name.as_deref().unwrap_or_default(),
                chunk.metadata.package_root.as_deref().unwrap_or_default(),
            ]
            .join(" "),
        );
        let symbol_hits = overlap_terms(
            &query_terms,
            chunk
                .metadata
                .symbol_display_name
                .as_deref()
                .unwrap_or_default(),
        );

        let mut signals = vec![SemanticRerankSignal {
            label: "semantic_score".to_string(),
            points: 0,
            detail: format!("base semantic score {semantic_score}"),
        }];

        let mut bonus = 0i32;
        bonus += push_signal(
            &mut signals,
            "lexical_overlap",
            lexical_points(&text_hits),
            &text_hits,
            "serialized chunk text matched query terms",
        );
        bonus += push_signal(
            &mut signals,
            "path_hits",
            path_points(&path_hits),
            &path_hits,
            "file path matched query terms",
        );
        bonus += push_signal(
            &mut signals,
            "symbol_name",
            symbol_points(&symbol_hits),
            &symbol_hits,
            "symbol display name matched query terms",
        );
        bonus += push_signal(
            &mut signals,
            "package_hits",
            package_points(&package_hits),
            &package_hits,
            "package metadata matched query terms",
        );

        if let Some(kind_signal) =
            symbol_kind_signal(&query_terms, chunk.metadata.symbol_kind.as_ref())
        {
            bonus += kind_signal.points;
            signals.push(kind_signal);
        }
        if let Some(export_signal) = export_signal(
            chunk.metadata.symbol_is_exported,
            chunk.metadata.symbol_is_default_export,
        ) {
            bonus += export_signal.points;
            signals.push(export_signal);
        }
        if let Some(chunk_kind_signal) = chunk_kind_signal(&query_terms, &chunk.metadata.chunk_kind)
        {
            bonus += chunk_kind_signal.points;
            signals.push(chunk_kind_signal);
        }

        let applied_bonus = if matches!(mode, SemanticRerankMode::Hybrid) {
            bonus.max(0) as u32
        } else {
            0
        };
        let final_score = semantic_score.saturating_add(applied_bonus);
        let reason = reason_for(
            chunk,
            semantic_score,
            &signals,
            matches!(mode, SemanticRerankMode::Hybrid),
        );

        SemanticRetrievalHit {
            rank: 0,
            score: if matches!(mode, SemanticRerankMode::Hybrid) {
                final_score
            } else {
                semantic_score
            },
            semantic_score,
            rerank_score: if matches!(mode, SemanticRerankMode::Hybrid) {
                final_score
            } else {
                semantic_score
            },
            chunk: chunk.metadata.clone(),
            reason,
            snippet: String::new(),
            explanation: Some(SemanticRetrievalExplanation {
                query_terms,
                text_term_hits: text_hits,
                path_term_hits: path_hits,
                symbol_term_hits: symbol_hits,
                package_term_hits: package_hits,
                signals,
            }),
        }
    }
}

fn lexical_points(hits: &[String]) -> i32 {
    (hits.len() as i32) * 2_500
}

fn path_points(hits: &[String]) -> i32 {
    (hits.len() as i32) * 3_500
}

fn symbol_points(hits: &[String]) -> i32 {
    (hits.len() as i32) * 4_500
}

fn package_points(hits: &[String]) -> i32 {
    (hits.len() as i32) * 1_750
}

fn push_signal(
    signals: &mut Vec<SemanticRerankSignal>,
    label: &str,
    points: i32,
    hits: &[String],
    detail_prefix: &str,
) -> i32 {
    if hits.is_empty() || points <= 0 {
        return 0;
    }
    signals.push(SemanticRerankSignal {
        label: label.to_string(),
        points,
        detail: format!("{detail_prefix}: {}", hits.join(", ")),
    });
    points
}

fn symbol_kind_signal(
    query_terms: &[String],
    kind: Option<&SymbolKind>,
) -> Option<SemanticRerankSignal> {
    let kind = kind?;
    let kind_terms = symbol_kind_terms(kind);
    let query_set = query_terms.iter().cloned().collect::<BTreeSet<_>>();
    let matched = kind_terms
        .iter()
        .filter(|term| query_set.contains(**term))
        .cloned()
        .collect::<Vec<_>>();
    if matched.is_empty() {
        return None;
    }
    Some(SemanticRerankSignal {
        label: "symbol_kind".to_string(),
        points: 2_000 + matched.len() as i32 * 500,
        detail: format!(
            "query referenced symbol kind {:?} via {}",
            kind,
            matched.join(", ")
        ),
    })
}

fn export_signal(
    symbol_is_exported: Option<bool>,
    symbol_is_default_export: Option<bool>,
) -> Option<SemanticRerankSignal> {
    if symbol_is_default_export == Some(true) {
        return Some(SemanticRerankSignal {
            label: "export_visibility".to_string(),
            points: 1_500,
            detail: "default-exported symbol".to_string(),
        });
    }
    if symbol_is_exported == Some(true) {
        return Some(SemanticRerankSignal {
            label: "export_visibility".to_string(),
            points: 900,
            detail: "exported/public symbol".to_string(),
        });
    }
    None
}

fn chunk_kind_signal(
    query_terms: &[String],
    chunk_kind: &SemanticChunkKind,
) -> Option<SemanticRerankSignal> {
    let matched = match chunk_kind {
        SemanticChunkKind::RouteFile if query_terms.iter().any(|term| term == "route") => {
            Some(("route_prior", 3_000, "route-style chunk"))
        }
        SemanticChunkKind::ConfigFile
            if query_terms
                .iter()
                .any(|term| matches!(term.as_str(), "config" | "policy")) =>
        {
            Some(("config_prior", 2_500, "config-style chunk"))
        }
        SemanticChunkKind::TestFile if query_terms.iter().any(|term| term == "test") => {
            Some(("test_prior", 2_500, "test-style chunk"))
        }
        _ => None,
    }?;

    Some(SemanticRerankSignal {
        label: matched.0.to_string(),
        points: matched.1,
        detail: matched.2.to_string(),
    })
}

fn reason_for(
    chunk: &SemanticChunkRecord,
    semantic_score: u32,
    signals: &[SemanticRerankSignal],
    hybrid: bool,
) -> String {
    let target = chunk
        .metadata
        .symbol_display_name
        .clone()
        .unwrap_or_else(|| chunk.metadata.path.clone());
    let signal_labels = signals
        .iter()
        .filter(|signal| signal.points > 0)
        .map(|signal| signal.label.as_str())
        .take(3)
        .collect::<Vec<_>>();

    if hybrid && !signal_labels.is_empty() {
        format!(
            "semantic match on {target} in {} with {} (base {semantic_score})",
            chunk.metadata.path,
            signal_labels.join(", ")
        )
    } else if chunk.metadata.symbol_display_name.is_some() {
        format!(
            "semantic vector match on symbol {target} in {} (score {semantic_score})",
            chunk.metadata.path
        )
    } else {
        format!(
            "semantic vector match in {} (score {semantic_score})",
            chunk.metadata.path
        )
    }
}

fn overlap_terms(query_terms: &[String], text: &str) -> Vec<String> {
    let target = ordered_terms(text).into_iter().collect::<BTreeSet<_>>();
    query_terms
        .iter()
        .filter(|term| target.contains(*term))
        .cloned()
        .collect()
}

fn ordered_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = BTreeSet::new();
    for token in raw_terms(text) {
        if let Some(normalized) = normalize_term(&token) {
            if seen.insert(normalized.clone()) {
                terms.push(normalized);
            }
        }
    }
    terms
}

fn raw_terms(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut previous_was_lower = false;

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && previous_was_lower && !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            current.push(ch.to_ascii_lowercase());
            previous_was_lower = ch.is_ascii_lowercase();
        } else {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            previous_was_lower = false;
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn normalize_term(token: &str) -> Option<String> {
    if token.len() < 2 {
        return None;
    }
    if STOPWORDS.contains(&token) {
        return None;
    }

    let normalized = if token.ends_with("ies") && token.len() > 4 {
        format!("{}y", &token[..token.len() - 3])
    } else if token.ends_with('s') && token.len() > 3 {
        token[..token.len() - 1].to_string()
    } else {
        token.to_string()
    };

    if STOPWORDS.contains(&normalized.as_str()) {
        None
    } else {
        Some(normalized)
    }
}

fn symbol_kind_terms(kind: &SymbolKind) -> &'static [&'static str] {
    match kind {
        SymbolKind::File => &["file"],
        SymbolKind::Module => &["module"],
        SymbolKind::Namespace => &["namespace"],
        SymbolKind::Class => &["class"],
        SymbolKind::Interface => &["interface"],
        SymbolKind::TypeAlias => &["type", "alias"],
        SymbolKind::Enum => &["enum"],
        SymbolKind::EnumMember => &["enum", "member"],
        SymbolKind::Function => &["function"],
        SymbolKind::Method => &["method"],
        SymbolKind::Constructor => &["constructor"],
        SymbolKind::Property => &["property"],
        SymbolKind::Field => &["field"],
        SymbolKind::Variable => &["variable"],
        SymbolKind::Constant => &["constant"],
        SymbolKind::Parameter => &["parameter"],
        SymbolKind::ImportBinding => &["import", "binding"],
    }
}

const STOPWORDS: [&str; 21] = [
    "a", "all", "an", "by", "do", "for", "from", "how", "in", "is", "of", "on", "one", "place",
    "the", "through", "to", "we", "what", "where", "which",
];

#[cfg(test)]
mod tests {
    use hyperindex_protocol::semantic::{
        SemanticChunkId, SemanticChunkKind, SemanticChunkMetadata, SemanticChunkRecord,
        SemanticChunkSourceKind, SemanticChunkTextMetadata, SemanticQueryText,
    };
    use hyperindex_protocol::symbols::SymbolKind;

    use super::{SemanticReranker, ordered_terms};

    #[test]
    fn ordered_terms_split_camel_case_and_pluralize() {
        assert_eq!(
            ordered_terms("Where do we invalidate sessions in handlePasswordReset?"),
            vec![
                "invalidate".to_string(),
                "session".to_string(),
                "handle".to_string(),
                "password".to_string(),
                "reset".to_string(),
            ]
        );
    }

    #[test]
    fn hybrid_hit_adds_explanations_and_scores() {
        let hit = SemanticReranker::default().build_hit(
            &SemanticQueryText {
                text: "Where is the logout route?".to_string(),
            },
            &SemanticChunkRecord {
                metadata: SemanticChunkMetadata {
                    chunk_id: SemanticChunkId("chunk-route".to_string()),
                    chunk_kind: SemanticChunkKind::RouteFile,
                    source_kind: SemanticChunkSourceKind::File,
                    path: "packages/api/src/routes/logout.ts".to_string(),
                    language: None,
                    extension: Some("ts".to_string()),
                    package_name: Some("@hyperindex/api".to_string()),
                    package_root: Some("packages/api".to_string()),
                    workspace_root: Some(".".to_string()),
                    symbol_id: None,
                    symbol_display_name: Some("logoutRoute".to_string()),
                    symbol_kind: Some(SymbolKind::Function),
                    symbol_is_exported: Some(true),
                    symbol_is_default_export: Some(false),
                    span: None,
                    content_sha256: "sha-route".to_string(),
                    text: SemanticChunkTextMetadata {
                        serializer_id: "phase6".to_string(),
                        format_version: 1,
                        text_digest: "text-route".to_string(),
                        text_bytes: 10,
                        token_count_estimate: 2,
                    },
                },
                serialized_text: "export function logoutRoute() {}".to_string(),
                embedding_cache: None,
            },
            500_000,
            hyperindex_protocol::semantic::SemanticRerankMode::Hybrid,
        );

        assert!(hit.score > hit.semantic_score);
        assert_eq!(hit.rerank_score, hit.score);
        assert!(
            hit.explanation
                .as_ref()
                .unwrap()
                .signals
                .iter()
                .any(|signal| signal.label == "route_prior")
        );
    }
}
