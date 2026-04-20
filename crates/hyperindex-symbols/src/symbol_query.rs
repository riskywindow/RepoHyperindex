use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::Path;

use hyperindex_protocol::symbols::{
    GraphEdge, GraphEdgeKind, GraphNodeRef, OccurrenceRole, ResolvedSymbol, SymbolLocationSelector,
    SymbolMatchKind, SymbolOccurrence as ProtocolSymbolOccurrence, SymbolRecord, SymbolSearchHit,
    SymbolSearchMode, SymbolSearchQuery,
};
use tracing::debug;

use crate::facts::SymbolVisibility;
use crate::symbol_graph::{SymbolGraph, node_key};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolLookupHit {
    pub symbol_id: String,
    pub display_name: String,
    pub kind: String,
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolOccurrence {
    pub occurrence_id: String,
    pub symbol_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchScoreFactor {
    pub label: String,
    pub points: i32,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolSearchExplanation {
    pub match_kind: SymbolMatchKind,
    pub exact_case: bool,
    pub kind_preference_rank: Option<usize>,
    pub visibility: String,
    pub filename_match: bool,
    pub container_depth: usize,
    pub factors: Vec<SearchScoreFactor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedSymbolSearchHit {
    pub hit: SymbolSearchHit,
    pub explanation: SymbolSearchExplanation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolExplainRecord {
    pub symbol_id: String,
    pub canonical_symbol_id: String,
    pub container_chain: Vec<String>,
    pub child_count: usize,
    pub related_edge_count: usize,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolShowRecord {
    pub symbol: SymbolRecord,
    pub canonical_symbol: SymbolRecord,
    pub definitions: Vec<ProtocolSymbolOccurrence>,
    pub references: Vec<ProtocolSymbolOccurrence>,
    pub children: Vec<SymbolRecord>,
    pub related_edges: Vec<GraphEdge>,
    pub explanation: SymbolExplainRecord,
}

#[derive(Debug, Default, Clone)]
pub struct SymbolQueryEngine;

impl SymbolQueryEngine {
    pub fn search(
        &self,
        graph: &SymbolGraph,
        query: &SymbolSearchQuery,
        limit: usize,
    ) -> Vec<RankedSymbolSearchHit> {
        debug!(
            indexed_files = graph.indexed_files,
            query_text = query.text,
            "executing ranked symbol search over resolved graph"
        );
        let mut hits = self
            .candidate_symbols(graph, query)
            .into_iter()
            .filter_map(|symbol| self.rank_symbol(graph, query, symbol))
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| compare_ranked_hits(left, right, graph));
        hits.truncate(limit);
        hits
    }

    pub fn lookup(&self, graph: &SymbolGraph, query: &str, limit: usize) -> Vec<SymbolLookupHit> {
        self.search(
            graph,
            &SymbolSearchQuery {
                text: query.to_string(),
                mode: SymbolSearchMode::Exact,
                kinds: Vec::new(),
                path_prefix: None,
            },
            limit,
        )
        .into_iter()
        .map(|result| SymbolLookupHit {
            symbol_id: result.hit.symbol.symbol_id.0.clone(),
            display_name: result.hit.symbol.display_name.clone(),
            kind: format!("{:?}", result.hit.symbol.kind),
            path: result.hit.symbol.path.clone(),
            reason: result.hit.reason,
        })
        .collect()
    }

    pub fn show(&self, graph: &SymbolGraph, symbol_id: &str) -> Option<SymbolShowRecord> {
        let symbol = graph.symbols.get(symbol_id)?.clone();
        let canonical_symbol = graph
            .symbols
            .get(&self.canonical_symbol_id(graph, symbol_id))?
            .clone();
        let definitions = self.definition_occurrences(graph, symbol_id);
        let references = self.reference_occurrences(graph, symbol_id, None);
        let children = self.children(graph, symbol_id);
        let related_edges = self.related_edges(graph, symbol_id);
        let explanation = self.explain(graph, symbol_id);

        Some(SymbolShowRecord {
            symbol,
            canonical_symbol,
            definitions,
            references,
            children,
            related_edges,
            explanation,
        })
    }

    pub fn resolve(
        &self,
        graph: &SymbolGraph,
        selector: &SymbolLocationSelector,
    ) -> Option<ResolvedSymbol> {
        match selector {
            SymbolLocationSelector::LineColumn { path, line, column } => {
                self.resolve_line_column(graph, path, *line, *column)
            }
            SymbolLocationSelector::ByteOffset { path, offset } => {
                self.resolve_span(graph, path, *offset, *offset)
            }
        }
    }

    pub fn resolve_span(
        &self,
        graph: &SymbolGraph,
        path: &str,
        start: u32,
        end: u32,
    ) -> Option<ResolvedSymbol> {
        if let Some(occurrence) = self.best_occurrence_for_byte_range(graph, path, start, end) {
            let symbol = graph.symbols.get(&occurrence.symbol_id.0)?.clone();
            return Some(ResolvedSymbol {
                symbol,
                occurrence: Some(occurrence),
            });
        }

        let symbol = self.best_symbol_for_byte_range(graph, path, start, end)?;
        Some(ResolvedSymbol {
            symbol,
            occurrence: None,
        })
    }

    pub fn definitions(&self, graph: &SymbolGraph, symbol_id: &str) -> Vec<SymbolOccurrence> {
        self.definition_occurrences(graph, symbol_id)
            .iter()
            .map(to_occurrence)
            .collect()
    }

    pub fn references(&self, graph: &SymbolGraph, symbol_id: &str) -> Vec<SymbolOccurrence> {
        self.reference_occurrences(graph, symbol_id, None)
            .iter()
            .map(to_occurrence)
            .collect()
    }

    pub fn children(&self, graph: &SymbolGraph, symbol_id: &str) -> Vec<SymbolRecord> {
        let target = hyperindex_protocol::symbols::SymbolId(symbol_id.to_string());
        let mut children = graph
            .symbol_facts
            .values()
            .filter(|record| record.container.as_ref() == Some(&target))
            .map(|record| record.symbol.clone())
            .collect::<Vec<_>>();
        children.sort_by(compare_symbols);
        children
    }

    pub fn explain(&self, graph: &SymbolGraph, symbol_id: &str) -> SymbolExplainRecord {
        let Some(symbol) = graph.symbols.get(symbol_id) else {
            return SymbolExplainRecord {
                symbol_id: symbol_id.to_string(),
                canonical_symbol_id: symbol_id.to_string(),
                container_chain: Vec::new(),
                child_count: 0,
                related_edge_count: 0,
                notes: vec!["symbol id not found in resolved graph".to_string()],
            };
        };
        let canonical_symbol_id = self.canonical_symbol_id(graph, symbol_id);
        let canonical_symbol = graph.symbols.get(&canonical_symbol_id);
        let container_chain = container_chain(graph, symbol_id);
        let child_count = self.children(graph, symbol_id).len();
        let related_edges = self.related_edges(graph, symbol_id);
        let definition_count = self.definition_occurrences(graph, symbol_id).len();
        let reference_count = self.reference_occurrences(graph, symbol_id, None).len();

        let mut notes = vec![
            format!("{} declared in {}", symbol.display_name, symbol.path),
            format!("kind: {:?}", symbol.kind),
            format!("definitions: {definition_count}"),
            format!("references: {reference_count}"),
        ];
        if canonical_symbol_id != symbol_id {
            if let Some(canonical) = canonical_symbol {
                notes.push(format!(
                    "canonical target: {} ({})",
                    canonical.display_name, canonical.path
                ));
            }
        }
        if !container_chain.is_empty() {
            notes.push(format!("container chain: {}", container_chain.join(" -> ")));
        }

        SymbolExplainRecord {
            symbol_id: symbol_id.to_string(),
            canonical_symbol_id,
            container_chain,
            child_count,
            related_edge_count: related_edges.len(),
            notes,
        }
    }

    pub fn search_hits(
        &self,
        graph: &SymbolGraph,
        query: &SymbolSearchQuery,
        limit: usize,
    ) -> Vec<SymbolSearchHit> {
        self.search(graph, query, limit)
            .into_iter()
            .map(|result| result.hit)
            .collect()
    }

    pub fn definition_occurrences(
        &self,
        graph: &SymbolGraph,
        symbol_id: &str,
    ) -> Vec<ProtocolSymbolOccurrence> {
        let canonical_symbol_id = self.canonical_symbol_id(graph, symbol_id);
        let mut definitions = graph
            .occurrences_by_symbol
            .get(&canonical_symbol_id)
            .into_iter()
            .flat_map(|occurrences| occurrences.iter())
            .filter(|occurrence| {
                matches!(
                    occurrence.role,
                    OccurrenceRole::Definition | OccurrenceRole::Declaration
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        if definitions.is_empty() && canonical_symbol_id == symbol_id {
            definitions = graph
                .occurrences_by_symbol
                .get(symbol_id)
                .into_iter()
                .flat_map(|occurrences| occurrences.iter())
                .filter(|occurrence| matches!(occurrence.role, OccurrenceRole::Import))
                .cloned()
                .collect();
        }
        definitions.sort_by(compare_protocol_occurrences);
        definitions
    }

    pub fn reference_occurrences(
        &self,
        graph: &SymbolGraph,
        symbol_id: &str,
        limit: Option<usize>,
    ) -> Vec<ProtocolSymbolOccurrence> {
        let canonical_symbol_id = self.canonical_symbol_id(graph, symbol_id);
        let mut seen_occurrences = BTreeSet::new();
        let mut related_symbol_ids = BTreeSet::from([canonical_symbol_id.clone()]);
        let mut stack = vec![canonical_symbol_id.clone()];

        while let Some(current_symbol_id) = stack.pop() {
            let symbol_key = node_key(&GraphNodeRef::Symbol {
                symbol_id: hyperindex_protocol::symbols::SymbolId(current_symbol_id.clone()),
            });
            for edge in graph
                .incoming_edges
                .get(&symbol_key)
                .into_iter()
                .flat_map(|edges| edges.iter())
            {
                if !matches!(edge.kind, GraphEdgeKind::Imports | GraphEdgeKind::Exports) {
                    continue;
                }
                let GraphNodeRef::Symbol {
                    symbol_id: upstream_symbol,
                } = &edge.from
                else {
                    continue;
                };
                if related_symbol_ids.insert(upstream_symbol.0.clone()) {
                    stack.push(upstream_symbol.0.clone());
                }
            }
        }

        let mut occurrences = related_symbol_ids
            .into_iter()
            .flat_map(|current_symbol_id| {
                let roles = if current_symbol_id == canonical_symbol_id {
                    vec![OccurrenceRole::Reference, OccurrenceRole::Export]
                } else {
                    vec![
                        OccurrenceRole::Import,
                        OccurrenceRole::Reference,
                        OccurrenceRole::Export,
                    ]
                };
                graph
                    .occurrences_by_symbol
                    .get(&current_symbol_id)
                    .into_iter()
                    .flat_map(|values| values.iter())
                    .filter(move |occurrence| roles.contains(&occurrence.role))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .filter(|occurrence| seen_occurrences.insert(occurrence.occurrence_id.0.clone()))
            .collect::<Vec<_>>();

        occurrences.sort_by(compare_protocol_occurrences);
        if let Some(limit) = limit {
            occurrences.truncate(limit);
        }
        occurrences
    }

    fn rank_symbol(
        &self,
        graph: &SymbolGraph,
        query: &SymbolSearchQuery,
        symbol: &SymbolRecord,
    ) -> Option<RankedSymbolSearchHit> {
        if let Some(path_prefix) = &query.path_prefix {
            if !symbol.path.starts_with(path_prefix) {
                return None;
            }
        }
        if !query.kinds.is_empty() && !query.kinds.iter().any(|kind| *kind == symbol.kind) {
            return None;
        }

        let name_match = classify_match(&symbol.display_name, &query.text, &query.mode)?;
        let container_depth = container_depth(graph, &symbol.symbol_id.0);
        let visibility = graph
            .symbol_facts
            .get(&symbol.symbol_id.0)
            .map(|record| record.visibility.clone())
            .unwrap_or(SymbolVisibility::Local);
        let kind_preference_rank = query.kinds.iter().position(|kind| *kind == symbol.kind);
        let filename_match = file_stem_matches_query(&symbol.path, &query.text);

        let mut factors = Vec::new();
        let match_points = match name_match.match_kind {
            SymbolMatchKind::Exact => 1000,
            SymbolMatchKind::Prefix => 700,
            SymbolMatchKind::Substring => 400,
        };
        factors.push(SearchScoreFactor {
            label: "name_match".to_string(),
            points: match_points,
            detail: format!("{:?} name match", name_match.match_kind).to_lowercase(),
        });
        if name_match.exact_case {
            factors.push(SearchScoreFactor {
                label: "exact_case".to_string(),
                points: 40,
                detail: "query casing matches declaration name".to_string(),
            });
        }
        if let Some(rank) = kind_preference_rank {
            factors.push(SearchScoreFactor {
                label: "kind_preference".to_string(),
                points: 100 - (rank as i32 * 50),
                detail: format!("matched requested kind at preference index {rank}"),
            });
        }
        let visibility_points = match visibility {
            SymbolVisibility::DefaultExport => 25,
            SymbolVisibility::Exported => 20,
            SymbolVisibility::Local => 0,
        };
        if visibility_points > 0 {
            factors.push(SearchScoreFactor {
                label: "visibility".to_string(),
                points: visibility_points,
                detail: format!("{} declaration", visibility_name(&visibility)),
            });
        }
        if filename_match {
            factors.push(SearchScoreFactor {
                label: "filename".to_string(),
                points: 10,
                detail: "filename stem matches the query text".to_string(),
            });
        }
        factors.push(SearchScoreFactor {
            label: "container_depth".to_string(),
            points: 10 - container_depth.min(10) as i32,
            detail: format!("container depth {container_depth}"),
        });

        let score = factors
            .iter()
            .map(|factor| factor.points.max(0) as u32)
            .sum();
        let reason = factors
            .iter()
            .filter(|factor| factor.points > 0)
            .map(|factor| factor.detail.clone())
            .collect::<Vec<_>>()
            .join("; ");

        Some(RankedSymbolSearchHit {
            hit: SymbolSearchHit {
                symbol: symbol.clone(),
                match_kind: name_match.match_kind.clone(),
                score,
                reason,
            },
            explanation: SymbolSearchExplanation {
                match_kind: name_match.match_kind,
                exact_case: name_match.exact_case,
                kind_preference_rank,
                visibility: visibility_name(&visibility).to_string(),
                filename_match,
                container_depth,
                factors,
            },
        })
    }

    fn candidate_symbols<'a>(
        &self,
        graph: &'a SymbolGraph,
        query: &SymbolSearchQuery,
    ) -> Vec<&'a SymbolRecord> {
        if query.text.is_empty() {
            return Vec::new();
        }
        match query.mode {
            SymbolSearchMode::Exact => graph
                .symbol_ids_by_lower_name
                .get(&query.text.to_ascii_lowercase())
                .into_iter()
                .flat_map(|symbol_ids| symbol_ids.iter())
                .filter_map(|symbol_id| graph.symbols.get(symbol_id))
                .collect(),
            _ => graph.symbols.values().collect(),
        }
    }

    fn canonical_symbol_id(&self, graph: &SymbolGraph, symbol_id: &str) -> String {
        let mut current = symbol_id.to_string();
        let mut seen = BTreeSet::new();
        while seen.insert(current.clone()) {
            let symbol_key = node_key(&GraphNodeRef::Symbol {
                symbol_id: hyperindex_protocol::symbols::SymbolId(current.clone()),
            });
            let mut next_symbols = graph
                .outgoing_edges
                .get(&symbol_key)
                .into_iter()
                .flat_map(|edges| edges.iter())
                .filter(|edge| matches!(edge.kind, GraphEdgeKind::Imports | GraphEdgeKind::Exports))
                .filter_map(|edge| match &edge.to {
                    GraphNodeRef::Symbol { symbol_id } => Some(symbol_id.0.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            next_symbols.sort();
            next_symbols.dedup();
            if next_symbols.len() != 1 {
                break;
            }
            current = next_symbols.remove(0);
        }
        current
    }

    fn related_edges(&self, graph: &SymbolGraph, symbol_id: &str) -> Vec<GraphEdge> {
        let key = node_key(&GraphNodeRef::Symbol {
            symbol_id: hyperindex_protocol::symbols::SymbolId(symbol_id.to_string()),
        });
        let mut edges = graph
            .outgoing_edges
            .get(&key)
            .into_iter()
            .flat_map(|values| values.iter())
            .chain(
                graph
                    .incoming_edges
                    .get(&key)
                    .into_iter()
                    .flat_map(|values| values.iter()),
            )
            .cloned()
            .collect::<Vec<_>>();
        edges.sort_by(|left, right| left.edge_id.cmp(&right.edge_id));
        edges.dedup_by(|left, right| left.edge_id == right.edge_id);
        edges
    }

    fn resolve_line_column(
        &self,
        graph: &SymbolGraph,
        path: &str,
        line: u32,
        column: u32,
    ) -> Option<ResolvedSymbol> {
        if let Some(occurrence) = self.best_occurrence_for_line_column(graph, path, line, column) {
            let symbol = graph.symbols.get(&occurrence.symbol_id.0)?.clone();
            return Some(ResolvedSymbol {
                symbol,
                occurrence: Some(occurrence),
            });
        }

        let symbol = graph
            .symbol_ids_by_file
            .get(path)
            .into_iter()
            .flat_map(|symbol_ids| symbol_ids.iter())
            .filter_map(|symbol_id| graph.symbols.get(symbol_id))
            .filter(|symbol| span_contains_line_column(&symbol.span, line, column))
            .cloned()
            .min_by(|left, right| compare_symbols_for_resolution(left, right))?;
        Some(ResolvedSymbol {
            symbol,
            occurrence: None,
        })
    }

    fn best_occurrence_for_line_column(
        &self,
        graph: &SymbolGraph,
        path: &str,
        line: u32,
        column: u32,
    ) -> Option<ProtocolSymbolOccurrence> {
        graph
            .occurrences_by_file
            .get(path)
            .into_iter()
            .flat_map(|occurrences| occurrences.iter())
            .filter(|occurrence| span_contains_line_column(&occurrence.span, line, column))
            .cloned()
            .min_by(compare_occurrences_for_resolution)
    }

    fn best_occurrence_for_byte_range(
        &self,
        graph: &SymbolGraph,
        path: &str,
        start: u32,
        end: u32,
    ) -> Option<ProtocolSymbolOccurrence> {
        graph
            .occurrences_by_file
            .get(path)
            .into_iter()
            .flat_map(|occurrences| occurrences.iter())
            .filter(|occurrence| {
                occurrence.span.bytes.start <= start && end <= occurrence.span.bytes.end
            })
            .cloned()
            .min_by(compare_occurrences_for_resolution)
    }

    fn best_symbol_for_byte_range(
        &self,
        graph: &SymbolGraph,
        path: &str,
        start: u32,
        end: u32,
    ) -> Option<SymbolRecord> {
        graph
            .symbol_ids_by_file
            .get(path)
            .into_iter()
            .flat_map(|symbol_ids| symbol_ids.iter())
            .filter_map(|symbol_id| graph.symbols.get(symbol_id))
            .filter(|symbol| symbol.span.bytes.start <= start && end <= symbol.span.bytes.end)
            .cloned()
            .min_by(compare_symbols_for_resolution)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NameMatch {
    match_kind: SymbolMatchKind,
    exact_case: bool,
}

fn classify_match(name: &str, query: &str, mode: &SymbolSearchMode) -> Option<NameMatch> {
    let lower_name = name.to_ascii_lowercase();
    let lower_query = query.to_ascii_lowercase();
    if query.is_empty() {
        return None;
    }
    let exact_case = name == query;
    match mode {
        SymbolSearchMode::Exact => lower_name.eq(&lower_query).then_some(NameMatch {
            match_kind: SymbolMatchKind::Exact,
            exact_case,
        }),
        SymbolSearchMode::Prefix => {
            if lower_name == lower_query {
                Some(NameMatch {
                    match_kind: SymbolMatchKind::Exact,
                    exact_case,
                })
            } else if lower_name.starts_with(&lower_query) {
                Some(NameMatch {
                    match_kind: SymbolMatchKind::Prefix,
                    exact_case: name.starts_with(query),
                })
            } else {
                None
            }
        }
        SymbolSearchMode::Substring => {
            if lower_name == lower_query {
                Some(NameMatch {
                    match_kind: SymbolMatchKind::Exact,
                    exact_case,
                })
            } else if lower_name.starts_with(&lower_query) {
                Some(NameMatch {
                    match_kind: SymbolMatchKind::Prefix,
                    exact_case: name.starts_with(query),
                })
            } else if lower_name.contains(&lower_query) {
                Some(NameMatch {
                    match_kind: SymbolMatchKind::Substring,
                    exact_case: name.contains(query),
                })
            } else {
                None
            }
        }
    }
}

fn compare_ranked_hits(
    left: &RankedSymbolSearchHit,
    right: &RankedSymbolSearchHit,
    graph: &SymbolGraph,
) -> Ordering {
    right
        .hit
        .score
        .cmp(&left.hit.score)
        .then_with(|| {
            left.explanation
                .kind_preference_rank
                .unwrap_or(usize::MAX)
                .cmp(&right.explanation.kind_preference_rank.unwrap_or(usize::MAX))
        })
        .then_with(|| left.hit.symbol.path.cmp(&right.hit.symbol.path))
        .then_with(|| {
            left.hit
                .symbol
                .display_name
                .cmp(&right.hit.symbol.display_name)
        })
        .then_with(|| compare_symbols(&left.hit.symbol, &right.hit.symbol))
        .then_with(|| {
            container_depth(graph, &left.hit.symbol.symbol_id.0)
                .cmp(&container_depth(graph, &right.hit.symbol.symbol_id.0))
        })
}

fn compare_symbols(left: &SymbolRecord, right: &SymbolRecord) -> Ordering {
    left.display_name
        .cmp(&right.display_name)
        .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
        .then_with(|| left.path.cmp(&right.path))
        .then_with(|| left.span.bytes.start.cmp(&right.span.bytes.start))
        .then_with(|| left.symbol_id.0.cmp(&right.symbol_id.0))
}

fn compare_symbols_for_resolution(left: &SymbolRecord, right: &SymbolRecord) -> Ordering {
    span_width(&left.span)
        .cmp(&span_width(&right.span))
        .then_with(|| right.span.bytes.start.cmp(&left.span.bytes.start))
        .then_with(|| compare_symbols(left, right))
}

fn compare_protocol_occurrences(
    left: &ProtocolSymbolOccurrence,
    right: &ProtocolSymbolOccurrence,
) -> Ordering {
    left.path
        .cmp(&right.path)
        .then_with(|| left.span.bytes.start.cmp(&right.span.bytes.start))
        .then_with(|| occurrence_role_rank(&left.role).cmp(&occurrence_role_rank(&right.role)))
        .then_with(|| left.occurrence_id.0.cmp(&right.occurrence_id.0))
}

fn compare_occurrences_for_resolution(
    left: &ProtocolSymbolOccurrence,
    right: &ProtocolSymbolOccurrence,
) -> Ordering {
    span_width(&left.span)
        .cmp(&span_width(&right.span))
        .then_with(|| occurrence_role_rank(&left.role).cmp(&occurrence_role_rank(&right.role)))
        .then_with(|| right.span.bytes.start.cmp(&left.span.bytes.start))
        .then_with(|| left.occurrence_id.0.cmp(&right.occurrence_id.0))
}

fn occurrence_role_rank(role: &OccurrenceRole) -> usize {
    match role {
        OccurrenceRole::Definition => 0,
        OccurrenceRole::Import => 1,
        OccurrenceRole::Reference => 2,
        OccurrenceRole::Export => 3,
        OccurrenceRole::Declaration => 4,
    }
}

fn span_width(span: &hyperindex_protocol::symbols::SourceSpan) -> u32 {
    span.bytes.end.saturating_sub(span.bytes.start)
}

fn span_contains_line_column(
    span: &hyperindex_protocol::symbols::SourceSpan,
    line: u32,
    column: u32,
) -> bool {
    let after_start =
        (line > span.start.line) || (line == span.start.line && column >= span.start.column);
    let before_end = (line < span.end.line) || (line == span.end.line && column <= span.end.column);
    after_start && before_end
}

fn container_depth(graph: &SymbolGraph, symbol_id: &str) -> usize {
    let mut depth = 0usize;
    let mut current = graph
        .symbol_facts
        .get(symbol_id)
        .and_then(|record| record.container.clone());
    while let Some(container) = current {
        depth += 1;
        current = graph
            .symbol_facts
            .get(&container.0)
            .and_then(|record| record.container.clone());
    }
    depth
}

fn container_chain(graph: &SymbolGraph, symbol_id: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = graph
        .symbol_facts
        .get(symbol_id)
        .and_then(|record| record.container.clone());
    while let Some(container) = current {
        let Some(symbol) = graph.symbols.get(&container.0) else {
            break;
        };
        chain.push(symbol.display_name.clone());
        current = graph
            .symbol_facts
            .get(&container.0)
            .and_then(|record| record.container.clone());
    }
    chain.reverse();
    chain
}

fn file_stem_matches_query(path: &str, query: &str) -> bool {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(|stem| stem.eq_ignore_ascii_case(query))
        .unwrap_or(false)
}

fn visibility_name(visibility: &SymbolVisibility) -> &'static str {
    match visibility {
        SymbolVisibility::Local => "local",
        SymbolVisibility::Exported => "exported",
        SymbolVisibility::DefaultExport => "default_export",
    }
}

fn to_occurrence(occurrence: &ProtocolSymbolOccurrence) -> SymbolOccurrence {
    SymbolOccurrence {
        occurrence_id: occurrence.occurrence_id.0.clone(),
        symbol_id: occurrence.symbol_id.0.clone(),
        path: occurrence.path.clone(),
        line: occurrence.span.start.line as usize,
        column: occurrence.span.start.column as usize,
        role: format!("{:?}", occurrence.role),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, SnapshotFile, WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::{
        SymbolKind, SymbolLocationSelector, SymbolSearchMode, SymbolSearchQuery,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use crate::SymbolWorkspace;

    use super::SymbolQueryEngine;

    fn snapshot(files: Vec<(&str, &str)>) -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            snapshot_id: "snap-1".to_string(),
            repo_id: "repo-1".to_string(),
            repo_root: "/tmp/repo".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "abc123".to_string(),
                digest: "base".to_string(),
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
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    fn prepare(snapshot: &ComposedSnapshot) -> (crate::SymbolScaffoldIndex, SymbolQueryEngine) {
        let mut workspace = SymbolWorkspace::default();
        let index = workspace.prepare_snapshot(snapshot).unwrap();
        (index, workspace.query_engine())
    }

    fn line_column_of(contents: &str, needle: &str, occurrence: usize) -> (u32, u32) {
        let mut start = 0usize;
        let mut found = None;
        for _ in 0..=occurrence {
            let offset = contents[start..].find(needle).unwrap();
            found = Some(start + offset);
            start += offset + needle.len();
        }
        let offset = found.unwrap();
        let prefix = &contents[..offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
        let column = prefix
            .rsplit_once('\n')
            .map(|(_, tail)| tail.chars().count() as u32 + 1)
            .unwrap_or(prefix.chars().count() as u32 + 1);
        (line, column)
    }

    #[test]
    fn search_ranking_is_deterministic_and_tie_breaks_stably() {
        let snapshot = snapshot(vec![
            (
                "src/invalidateSession.ts",
                "export function invalidateSession() { return true; }\n",
            ),
            (
                "src/a.ts",
                "export function invalidateSession() { return true; }\n",
            ),
            (
                "src/b.ts",
                "export function invalidateSession() { return true; }\n",
            ),
            (
                "src/local.ts",
                "function invalidateSession() { return true; }\n",
            ),
            (
                "src/nested.ts",
                "export function outer() { function invalidateSession() { return true; } return invalidateSession(); }\n",
            ),
        ]);
        let (index, query) = prepare(&snapshot);

        let hits = query.search(
            &index.graph,
            &SymbolSearchQuery {
                text: "invalidateSession".to_string(),
                mode: SymbolSearchMode::Exact,
                kinds: Vec::new(),
                path_prefix: None,
            },
            10,
        );

        let ordered_paths = hits
            .iter()
            .map(|hit| hit.hit.symbol.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            ordered_paths,
            vec![
                "src/invalidateSession.ts".to_string(),
                "src/a.ts".to_string(),
                "src/b.ts".to_string(),
                "src/local.ts".to_string(),
                "src/nested.ts".to_string(),
            ]
        );
        assert!(hits[0].hit.reason.contains("exact"));
        assert_eq!(hits[1].hit.score, hits[2].hit.score);
    }

    #[test]
    fn search_supports_kind_filtered_queries_and_explanations() {
        let snapshot = snapshot(vec![(
            "src/run.ts",
            r#"
            export function run() {
              return true;
            }

            export class Runner {
              run() {
                return false;
              }
            }
            "#,
        )]);
        let (index, query) = prepare(&snapshot);

        let hits = query.search(
            &index.graph,
            &SymbolSearchQuery {
                text: "run".to_string(),
                mode: SymbolSearchMode::Exact,
                kinds: vec![SymbolKind::Method, SymbolKind::Function],
                path_prefix: None,
            },
            10,
        );

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].hit.symbol.kind, SymbolKind::Method);
        assert_eq!(hits[0].explanation.kind_preference_rank, Some(0));
        assert_eq!(hits[1].hit.symbol.kind, SymbolKind::Function);
        assert_eq!(hits[1].explanation.kind_preference_rank, Some(1));
    }

    #[test]
    fn show_symbol_lists_immediate_children() {
        let snapshot = snapshot(vec![(
            "src/service.ts",
            r#"
            export class SessionService {
              constructor() {}
              invalidateSession() {
                return true;
              }
            }
            "#,
        )]);
        let (index, query) = prepare(&snapshot);
        let class_symbol_id = query.search(
            &index.graph,
            &SymbolSearchQuery {
                text: "SessionService".to_string(),
                mode: SymbolSearchMode::Exact,
                kinds: vec![SymbolKind::Class],
                path_prefix: None,
            },
            1,
        )[0]
        .hit
        .symbol
        .symbol_id
        .0
        .clone();

        let show = query.show(&index.graph, &class_symbol_id).unwrap();
        let child_names = show
            .children
            .iter()
            .map(|symbol| symbol.display_name.clone())
            .collect::<Vec<_>>();

        assert_eq!(
            child_names,
            vec!["constructor".to_string(), "invalidateSession".to_string()]
        );
        assert_eq!(show.explanation.child_count, 2);
    }

    #[test]
    fn resolve_cursor_lookup_and_definitions_follow_imports() {
        let consumer = r#"
        import { createSession } from "./api";

        export function run() {
          return createSession();
        }
        "#;
        let snapshot = snapshot(vec![
            (
                "src/api.ts",
                r#"
                export function createSession() {
                  return 1;
                }
                "#,
            ),
            ("src/consumer.ts", consumer),
        ]);
        let (index, query) = prepare(&snapshot);
        let (line, column) = line_column_of(consumer, "createSession()", 0);

        let resolved = query
            .resolve(
                &index.graph,
                &SymbolLocationSelector::LineColumn {
                    path: "src/consumer.ts".to_string(),
                    line,
                    column,
                },
            )
            .unwrap();
        assert_eq!(resolved.symbol.display_name, "createSession");
        assert_eq!(resolved.symbol.path, "src/consumer.ts");

        let definitions = query.definition_occurrences(&index.graph, &resolved.symbol.symbol_id.0);
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].path, "src/api.ts");
    }

    #[test]
    fn reference_lookup_is_stable_for_cross_file_symbol_linkage() {
        let snapshot = snapshot(vec![
            (
                "src/api.ts",
                r#"
                export function createSession() {
                  return 1;
                }

                export function invalidateSession() {
                  return createSession();
                }
                "#,
            ),
            (
                "src/consumer.ts",
                r#"
                import { createSession } from "./api";
                export function run() {
                  return createSession();
                }
                "#,
            ),
        ]);
        let (index, query) = prepare(&snapshot);
        let source_symbol_id = query.search(
            &index.graph,
            &SymbolSearchQuery {
                text: "createSession".to_string(),
                mode: SymbolSearchMode::Exact,
                kinds: vec![SymbolKind::Function],
                path_prefix: Some("src/api".to_string()),
            },
            1,
        )[0]
        .hit
        .symbol
        .symbol_id
        .0
        .clone();

        let references = query.reference_occurrences(&index.graph, &source_symbol_id, None);
        let rendered = references
            .iter()
            .map(|occurrence| (occurrence.path.clone(), format!("{:?}", occurrence.role)))
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec![
                ("src/api.ts".to_string(), "Export".to_string()),
                ("src/api.ts".to_string(), "Reference".to_string()),
                ("src/consumer.ts".to_string(), "Import".to_string()),
                ("src/consumer.ts".to_string(), "Reference".to_string()),
            ]
        );
    }

    #[derive(Debug, Deserialize)]
    struct Phase1SymbolPack {
        queries: Vec<Phase1SymbolQuery>,
    }

    #[derive(Debug, Deserialize)]
    struct Phase1SymbolQuery {
        symbol: String,
        #[serde(rename = "type")]
        query_type: String,
    }

    #[test]
    fn representative_phase1_symbol_pack_queries_rank_as_exact_name_hits() {
        let pack = serde_json::from_str::<Phase1SymbolPack>(include_str!(
            "../../../bench/configs/query-packs/synthetic-saas-medium-symbol-pack.json"
        ))
        .unwrap();
        let wanted = pack
            .queries
            .into_iter()
            .filter(|query| query.query_type == "symbol")
            .map(|query| query.symbol)
            .filter(|symbol| {
                matches!(
                    symbol.as_str(),
                    "sessionStore" | "recordSessionEvent" | "invalidateSession"
                )
            })
            .collect::<Vec<_>>();

        let snapshot = snapshot(vec![
            (
                "packages/session/src/store/session-store.ts",
                r#"
                export const sessionStore = {};
                export function recordSessionEvent() {
                  return sessionStore;
                }
                "#,
            ),
            (
                "packages/auth/src/session/service.ts",
                r#"
                import { sessionStore, recordSessionEvent } from "@hyperindex/session";
                export function invalidateSession() {
                  recordSessionEvent();
                  return sessionStore;
                }
                "#,
            ),
            (
                "packages/session/package.json",
                r#"{ "name": "@hyperindex/session" }"#,
            ),
            ("package.json", r#"{ "name": "repo-root" }"#),
        ]);
        let (index, query) = prepare(&snapshot);

        for symbol in wanted {
            let hit = query.search(
                &index.graph,
                &SymbolSearchQuery {
                    text: symbol.clone(),
                    mode: SymbolSearchMode::Exact,
                    kinds: Vec::new(),
                    path_prefix: None,
                },
                1,
            );
            assert_eq!(hit.len(), 1);
            assert_eq!(hit[0].hit.symbol.display_name, symbol);
            assert_eq!(
                hit[0].hit.match_kind,
                hyperindex_protocol::symbols::SymbolMatchKind::Exact
            );
        }
    }
}
