use hyperindex_protocol::planner::{
    PlannerExplainResponse, PlannerQueryParams, PlannerQueryResponse,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use thiserror::Error;

use crate::daemon_integration::PlannerRuntimeContext;
use crate::intent_router::IntentRouter;
use crate::planner_engine::PlannerEngine;
use crate::query_ir::QueryIrBuilder;
use crate::result_grouping::ResultGrouping;
use crate::route_registry::PlannerRouteRegistry;
use crate::score_fusion::ScoreFusion;
use crate::trust_payloads::TrustPayloadFactory;

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("planner query is invalid: {0}")]
    InvalidQuery(String),
    #[error(
        "planner snapshot mismatch: requested repo={requested_repo_id} snapshot={requested_snapshot_id}, loaded repo={loaded_repo_id} snapshot={loaded_snapshot_id}"
    )]
    SnapshotMismatch {
        requested_repo_id: String,
        requested_snapshot_id: String,
        loaded_repo_id: String,
        loaded_snapshot_id: String,
    },
}

pub type PlannerResult<T> = Result<T, PlannerError>;

#[derive(Debug, Default, Clone)]
pub struct PlannerWorkspace {
    intent_router: IntentRouter,
    ir_builder: QueryIrBuilder,
    route_registry: PlannerRouteRegistry,
    engine: PlannerEngine,
    fusion: ScoreFusion,
    grouping: ResultGrouping,
    trust_payloads: TrustPayloadFactory,
}

impl PlannerWorkspace {
    pub fn plan(
        &self,
        context: &PlannerRuntimeContext,
        params: &PlannerQueryParams,
        snapshot: &ComposedSnapshot,
    ) -> PlannerResult<PlannerQueryResponse> {
        if params.repo_id != snapshot.repo_id || params.snapshot_id != snapshot.snapshot_id {
            return Err(PlannerError::SnapshotMismatch {
                requested_repo_id: params.repo_id.clone(),
                requested_snapshot_id: params.snapshot_id.clone(),
                loaded_repo_id: snapshot.repo_id.clone(),
                loaded_snapshot_id: snapshot.snapshot_id.clone(),
            });
        }

        let mode = self
            .intent_router
            .classify(params, context.default_mode.clone());
        let ir = self.ir_builder.build(context, params, snapshot, &mode)?;
        let route_plan = self.route_registry.plan(context, &ir);
        self.engine.query_scaffold(
            self,
            context,
            snapshot,
            params,
            mode,
            ir,
            route_plan,
            &self.fusion,
            &self.grouping,
            &self.trust_payloads,
        )
    }

    pub fn explain(
        &self,
        context: &PlannerRuntimeContext,
        params: &PlannerQueryParams,
        snapshot: &ComposedSnapshot,
    ) -> PlannerResult<PlannerExplainResponse> {
        if params.repo_id != snapshot.repo_id || params.snapshot_id != snapshot.snapshot_id {
            return Err(PlannerError::SnapshotMismatch {
                requested_repo_id: params.repo_id.clone(),
                requested_snapshot_id: params.snapshot_id.clone(),
                loaded_repo_id: snapshot.repo_id.clone(),
                loaded_snapshot_id: snapshot.snapshot_id.clone(),
            });
        }

        let mode = self
            .intent_router
            .classify(params, context.default_mode.clone());
        let ir = self.ir_builder.build(context, params, snapshot, &mode)?;
        let route_plan = self.route_registry.plan(context, &ir);
        self.engine.explain_scaffold(
            self,
            context,
            snapshot,
            params,
            mode,
            ir,
            route_plan,
            &self.fusion,
            &self.grouping,
            &self.trust_payloads,
        )
    }
}
