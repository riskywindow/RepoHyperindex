use hyperindex_protocol::planner::{PlannerResultGroup, PlannerRouteTrace};

#[derive(Debug, Default, Clone)]
pub struct ScoreFusion;

impl ScoreFusion {
    pub fn fuse_placeholder(&self, _route_traces: &[PlannerRouteTrace]) -> Vec<PlannerResultGroup> {
        Vec::new()
    }
}
