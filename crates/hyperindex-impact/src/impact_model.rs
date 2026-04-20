use std::collections::BTreeSet;

use hyperindex_protocol::impact::{ImpactAnalyzeParams, ImpactChangeScenario, ImpactTargetRef};
use hyperindex_protocol::symbols::SymbolKind;
use hyperindex_symbols::SymbolGraph;

use crate::common::{ImpactComponentStatus, implemented_status};
use crate::impact_enrichment::ImpactEnrichmentPlan;
use crate::{ImpactError, ImpactResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImpactSymbolTarget {
    pub symbol_id: String,
    pub canonical_symbol_id: String,
    pub display_name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImpactFileTarget {
    pub path: String,
    pub owned_symbol_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedImpactTarget {
    Symbol(ResolvedImpactSymbolTarget),
    File(ResolvedImpactFileTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactModelSeed {
    pub requested_target: ImpactTargetRef,
    pub resolved_target: ResolvedImpactTarget,
    pub change_hint: ImpactChangeScenario,
    pub include_transitive: bool,
}

#[derive(Debug, Default, Clone)]
pub struct ImpactModel;

impl ImpactModel {
    pub fn seed_for(
        &self,
        graph: &SymbolGraph,
        enrichment: &ImpactEnrichmentPlan,
        params: &ImpactAnalyzeParams,
    ) -> ImpactResult<ImpactModelSeed> {
        Ok(ImpactModelSeed {
            requested_target: params.target.clone(),
            resolved_target: self.resolve_target(graph, enrichment, &params.target)?,
            change_hint: params.change_hint.clone(),
            include_transitive: params.include_transitive,
        })
    }

    pub fn status(&self) -> ImpactComponentStatus {
        implemented_status(
            "impact_model",
            "typed target-resolution and query seed surface is present for symbol and file targets",
        )
    }

    fn resolve_target(
        &self,
        graph: &SymbolGraph,
        enrichment: &ImpactEnrichmentPlan,
        target: &ImpactTargetRef,
    ) -> ImpactResult<ResolvedImpactTarget> {
        match target {
            ImpactTargetRef::Symbol {
                value,
                symbol_id,
                path,
            } => self
                .resolve_symbol_target(
                    graph,
                    enrichment,
                    value,
                    symbol_id.as_ref(),
                    path.as_deref(),
                )
                .map(ResolvedImpactTarget::Symbol),
            ImpactTargetRef::File { path } => self
                .resolve_file_target(graph, enrichment, path)
                .map(ResolvedImpactTarget::File),
        }
    }

    fn resolve_symbol_target(
        &self,
        graph: &SymbolGraph,
        enrichment: &ImpactEnrichmentPlan,
        value: &str,
        explicit_symbol_id: Option<&hyperindex_protocol::symbols::SymbolId>,
        explicit_path: Option<&str>,
    ) -> ImpactResult<ResolvedImpactSymbolTarget> {
        let mut candidates = BTreeSet::new();

        if let Some(symbol_id) = explicit_symbol_id {
            if graph.symbols.contains_key(&symbol_id.0) {
                candidates.insert(symbol_id.0.clone());
            }
        }

        if graph.symbols.contains_key(value) {
            candidates.insert(value.to_string());
        }

        let selector = parse_symbol_selector(value, explicit_path);
        if let Some((path, display_name)) = selector.as_ref() {
            for symbol_id in graph.symbol_ids_by_file.get(path).into_iter().flatten() {
                let Some(symbol) = graph.symbols.get(symbol_id) else {
                    continue;
                };
                if symbol.display_name == *display_name {
                    candidates.insert(symbol_id.clone());
                }
            }
        }

        if candidates.is_empty() {
            for (symbol_id, symbol) in &graph.symbols {
                if symbol.display_name == value
                    && symbol.kind != SymbolKind::Module
                    && explicit_path
                        .map(|path| path == symbol.path)
                        .unwrap_or(true)
                {
                    candidates.insert(symbol_id.clone());
                }
            }
        }

        let canonical_candidates = candidates
            .into_iter()
            .map(|symbol_id| {
                enrichment
                    .canonical_symbol_by_symbol
                    .get(&symbol_id)
                    .cloned()
                    .unwrap_or(symbol_id)
            })
            .collect::<BTreeSet<_>>();

        if canonical_candidates.len() != 1 {
            return Err(ImpactError::TargetNotFound(value.to_string()));
        }
        let canonical_symbol_id = canonical_candidates
            .into_iter()
            .next()
            .ok_or_else(|| ImpactError::TargetNotFound(value.to_string()))?;

        if graph.symbols.get(&canonical_symbol_id).is_none() {
            return Err(ImpactError::TargetNotFound(value.to_string()));
        }

        let symbol = graph
            .symbols
            .get(&canonical_symbol_id)
            .ok_or_else(|| ImpactError::TargetNotFound(value.to_string()))?;

        Ok(ResolvedImpactSymbolTarget {
            symbol_id: canonical_symbol_id.clone(),
            canonical_symbol_id,
            display_name: symbol.display_name.clone(),
            path: symbol.path.clone(),
        })
    }

    fn resolve_file_target(
        &self,
        graph: &SymbolGraph,
        enrichment: &ImpactEnrichmentPlan,
        path: &str,
    ) -> ImpactResult<ResolvedImpactFileTarget> {
        let exists = graph.files.iter().any(|file| file.path == path)
            || enrichment.file_to_symbols.contains_key(path)
            || enrichment.reverse_dependents_by_file.contains_key(path)
            || enrichment.tests_by_file.contains_key(path);
        if !exists {
            return Err(ImpactError::TargetNotFound(path.to_string()));
        }

        Ok(ResolvedImpactFileTarget {
            path: path.to_string(),
            owned_symbol_ids: enrichment
                .file_to_symbols
                .get(path)
                .cloned()
                .unwrap_or_default(),
        })
    }
}

fn parse_symbol_selector(value: &str, explicit_path: Option<&str>) -> Option<(String, String)> {
    if let Some((path, display_name)) = value.split_once('#') {
        return Some((path.to_string(), display_name.to_string()));
    }
    explicit_path.map(|path| (path.to_string(), value.to_string()))
}
