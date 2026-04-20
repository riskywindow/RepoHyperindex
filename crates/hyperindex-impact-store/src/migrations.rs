#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactStoreMigration {
    pub schema_version: u32,
    pub description: &'static str,
}

pub const IMPACT_STORE_SCHEMA_VERSION: u32 = 2;

pub fn planned_migrations() -> Vec<ImpactStoreMigration> {
    vec![
        ImpactStoreMigration {
            schema_version: 1,
            description: "bootstrap phase5 impact store scaffold metadata",
        },
        ImpactStoreMigration {
            schema_version: IMPACT_STORE_SCHEMA_VERSION,
            description: "persist snapshot-scoped impact builds as typed json records",
        },
    ]
}
