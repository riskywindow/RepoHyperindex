#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticStoreMigration {
    pub schema_version: u32,
    pub description: &'static str,
}

pub const SEMANTIC_STORE_SCHEMA_VERSION: u32 = 4;

pub fn planned_migrations() -> Vec<SemanticStoreMigration> {
    vec![
        SemanticStoreMigration {
            schema_version: 1,
            description: "phase6 semantic scaffold tables",
        },
        SemanticStoreMigration {
            schema_version: 2,
            description: "phase6 semantic chunk persistence",
        },
        SemanticStoreMigration {
            schema_version: 3,
            description: "phase6 embedding cache identity expansion",
        },
        SemanticStoreMigration {
            schema_version: 4,
            description: "phase6 persisted flat vector index",
        },
    ]
}
