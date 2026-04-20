pub mod facts;
pub mod symbol_graph;
pub mod symbol_query;

use hyperindex_parser::ParseCore;
use hyperindex_protocol::snapshot::ComposedSnapshot;
use thiserror::Error;
use tracing::info;

pub use facts::{
    ExportFactRecord, ExtractedFileFacts, FactWorkspace, FactsBatch, ImportFactRecord,
    SymbolFactRecord, SymbolVisibility,
};
pub use symbol_graph::{SymbolGraph, SymbolGraphBuilder};
pub use symbol_query::{
    RankedSymbolSearchHit, SearchScoreFactor, SymbolExplainRecord, SymbolLookupHit,
    SymbolOccurrence, SymbolQueryEngine, SymbolSearchExplanation, SymbolShowRecord,
};

#[derive(Debug, Clone)]
pub struct SymbolScaffoldIndex {
    pub parse_plan: hyperindex_parser::ParseBatchPlan,
    pub facts: FactsBatch,
    pub graph: SymbolGraph,
}

#[derive(Debug, Error)]
pub enum SymbolError {
    #[error(transparent)]
    Parser(#[from] hyperindex_parser::ParserError),
}

pub type SymbolResult<T> = Result<T, SymbolError>;

#[derive(Debug, Default, Clone)]
pub struct SymbolWorkspace {
    parser: ParseCore,
    facts: FactWorkspace,
    graph: SymbolGraphBuilder,
    query: SymbolQueryEngine,
}

impl SymbolWorkspace {
    pub fn prepare_snapshot(
        &mut self,
        snapshot: &ComposedSnapshot,
    ) -> SymbolResult<SymbolScaffoldIndex> {
        let parse_plan = self.parser.parse_snapshot(snapshot)?;
        let facts = self.facts.extract(
            &snapshot.repo_id,
            &snapshot.snapshot_id,
            &parse_plan.artifacts,
        );
        let graph = self.graph.build_with_snapshot(&facts, snapshot);
        info!(
            snapshot_id = %snapshot.snapshot_id,
            indexed_files = graph.indexed_files,
            symbol_count = graph.symbol_count,
            "prepared phase4 symbol facts snapshot"
        );
        Ok(SymbolScaffoldIndex {
            parse_plan,
            facts,
            graph,
        })
    }

    pub fn query_engine(&self) -> SymbolQueryEngine {
        self.query.clone()
    }
}
