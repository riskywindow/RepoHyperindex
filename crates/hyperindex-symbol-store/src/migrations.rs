pub const SYMBOL_STORE_SCHEMA_VERSION: i32 = 3;

pub const SYMBOL_STORE_MIGRATIONS: &str = r#"
CREATE TABLE IF NOT EXISTS scaffold_snapshots (
    repo_id TEXT NOT NULL,
    snapshot_id TEXT PRIMARY KEY,
    planned_file_count INTEGER NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS indexed_files (
    snapshot_id TEXT NOT NULL,
    path TEXT NOT NULL,
    artifact_json TEXT NOT NULL,
    facts_json TEXT NOT NULL,
    PRIMARY KEY (snapshot_id, path)
);

CREATE TABLE IF NOT EXISTS symbol_facts (
    snapshot_id TEXT NOT NULL,
    path TEXT NOT NULL,
    symbol_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    container_symbol_id TEXT,
    visibility TEXT NOT NULL,
    signature_digest TEXT NOT NULL,
    file_path TEXT NOT NULL,
    record_json TEXT NOT NULL,
    PRIMARY KEY (snapshot_id, symbol_id)
);

CREATE TABLE IF NOT EXISTS symbol_occurrences (
    snapshot_id TEXT NOT NULL,
    occurrence_id TEXT NOT NULL,
    symbol_id TEXT NOT NULL,
    path TEXT NOT NULL,
    role TEXT NOT NULL,
    record_json TEXT NOT NULL,
    PRIMARY KEY (snapshot_id, occurrence_id)
);

CREATE TABLE IF NOT EXISTS graph_edges (
    snapshot_id TEXT NOT NULL,
    edge_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    record_json TEXT NOT NULL,
    PRIMARY KEY (snapshot_id, edge_id)
);

CREATE TABLE IF NOT EXISTS import_facts (
    snapshot_id TEXT NOT NULL,
    path TEXT NOT NULL,
    symbol_id TEXT NOT NULL,
    local_name TEXT NOT NULL,
    module_specifier TEXT NOT NULL,
    record_json TEXT NOT NULL,
    PRIMARY KEY (snapshot_id, path, symbol_id, local_name, module_specifier)
);

CREATE TABLE IF NOT EXISTS export_facts (
    snapshot_id TEXT NOT NULL,
    path TEXT NOT NULL,
    symbol_id TEXT NOT NULL,
    exported_name TEXT NOT NULL,
    module_specifier TEXT NOT NULL,
    record_json TEXT NOT NULL,
    PRIMARY KEY (snapshot_id, path, symbol_id, exported_name, module_specifier)
);

CREATE TABLE IF NOT EXISTS indexed_snapshot_state (
    snapshot_id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    parser_config_digest TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    indexed_file_count INTEGER NOT NULL,
    refresh_mode TEXT NOT NULL,
    indexed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
"#;
