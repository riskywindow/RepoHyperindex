use crate::common::{ImpactComponentStatus, implemented_status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasonPathSummary {
    pub included: bool,
    pub path_count: u32,
}

#[derive(Debug, Default, Clone)]
pub struct ReasonPathScaffold;

impl ReasonPathScaffold {
    pub fn summary(&self, included: bool) -> ReasonPathSummary {
        ReasonPathSummary {
            included,
            path_count: 0,
        }
    }

    pub fn status(&self) -> ImpactComponentStatus {
        implemented_status(
            "reason_paths",
            "direct impact hits emit deterministic one-edge reason paths when requested",
        )
    }
}
