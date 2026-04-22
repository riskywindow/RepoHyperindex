use hyperindex_protocol::planner::PlannerResultGroup;

#[derive(Debug, Default, Clone)]
pub struct TrustPayloadFactory;

impl TrustPayloadFactory {
    pub fn decorate_placeholder(&self, groups: Vec<PlannerResultGroup>) -> Vec<PlannerResultGroup> {
        groups
    }
}
