pub const IMPACT_PHASE: &str = "phase5";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactComponentStatus {
    pub name: &'static str,
    pub status: &'static str,
    pub notes: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactScaffoldReport {
    pub phase: &'static str,
    pub components: Vec<ImpactComponentStatus>,
}

impl ImpactScaffoldReport {
    pub fn new(components: Vec<ImpactComponentStatus>) -> Self {
        Self {
            phase: IMPACT_PHASE,
            components,
        }
    }
}

pub fn scaffold_status(name: &'static str, notes: &'static str) -> ImpactComponentStatus {
    ImpactComponentStatus {
        name,
        status: "scaffolded",
        notes,
    }
}

pub fn implemented_status(name: &'static str, notes: &'static str) -> ImpactComponentStatus {
    ImpactComponentStatus {
        name,
        status: "implemented",
        notes,
    }
}
