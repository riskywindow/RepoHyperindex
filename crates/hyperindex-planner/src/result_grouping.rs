use hyperindex_protocol::planner::PlannerResultGroup;

#[derive(Debug, Default, Clone)]
pub struct ResultGrouping;

impl ResultGrouping {
    pub fn group_placeholder(&self, groups: Vec<PlannerResultGroup>) -> Vec<PlannerResultGroup> {
        groups
    }
}
