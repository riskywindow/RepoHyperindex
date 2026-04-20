use hyperindex_parser::ParseCore;
use hyperindex_protocol::snapshot::{
    BaseSnapshot, BaseSnapshotKind, BufferOverlay, ComposedSnapshot, OverlayEntryKind,
    SnapshotFile, WorkingTreeEntry, WorkingTreeOverlay,
};
use hyperindex_protocol::symbols::{LanguageId, ParseInputSourceKind};
use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

fn snapshot_with_file(path: &str, contents: &str) -> ComposedSnapshot {
    ComposedSnapshot {
        version: STORAGE_VERSION,
        protocol_version: PROTOCOL_VERSION.to_string(),
        snapshot_id: format!("snap-{path}"),
        repo_id: "repo-1".to_string(),
        repo_root: "/tmp/repo".to_string(),
        base: BaseSnapshot {
            kind: BaseSnapshotKind::GitCommit,
            commit: "abc123".to_string(),
            digest: "base".to_string(),
            file_count: 1,
            files: vec![SnapshotFile {
                path: path.to_string(),
                content_sha256: format!("sha-{path}"),
                content_bytes: contents.len(),
                contents: contents.to_string(),
            }],
        },
        working_tree: WorkingTreeOverlay {
            digest: "work".to_string(),
            entries: Vec::new(),
        },
        buffers: Vec::new(),
    }
}

#[test]
fn parses_all_supported_ts_js_extensions() {
    let cases = [
        (
            "src/module.ts",
            include_str!("fixtures/valid/module.ts"),
            LanguageId::Typescript,
        ),
        (
            "src/component.tsx",
            include_str!("fixtures/valid/component.tsx"),
            LanguageId::Tsx,
        ),
        (
            "src/plain.js",
            include_str!("fixtures/valid/plain.js"),
            LanguageId::Javascript,
        ),
        (
            "src/widget.jsx",
            include_str!("fixtures/valid/widget.jsx"),
            LanguageId::Jsx,
        ),
        (
            "src/server.mts",
            include_str!("fixtures/valid/server.mts"),
            LanguageId::Mts,
        ),
        (
            "src/config.cts",
            include_str!("fixtures/valid/config.cts"),
            LanguageId::Cts,
        ),
    ];

    for (path, contents, language) in cases {
        let snapshot = snapshot_with_file(path, contents);
        let mut parser = ParseCore::default();
        let artifact = parser
            .parse_file_from_snapshot(&snapshot, path)
            .unwrap()
            .unwrap();

        assert!(artifact.parse_succeeded(), "{path}");
        assert!(!artifact.has_recoverable_errors(), "{path}");
        assert_eq!(artifact.metadata().language, language, "{path}");
        assert_eq!(artifact.root().kind, "program", "{path}");
        assert!(artifact.root().child_count > 0, "{path}");
    }
}

#[test]
fn broken_editing_fixture_reports_recoverable_diagnostics() {
    let contents = include_str!("fixtures/broken/editing.tsx");
    let snapshot = snapshot_with_file("src/editing.tsx", contents);
    let mut parser = ParseCore::default();
    let artifact = parser
        .parse_file_from_snapshot(&snapshot, "src/editing.tsx")
        .unwrap()
        .unwrap();

    assert!(artifact.parse_succeeded());
    assert!(artifact.has_recoverable_errors());
    assert_eq!(artifact.metadata().language, LanguageId::Tsx);
    assert!(!artifact.metadata().diagnostics.is_empty());
    assert!(artifact.root().has_error);
}

#[test]
fn snapshot_parse_respects_buffer_and_working_tree_precedence() {
    let mut parser = ParseCore::default();
    let snapshot = ComposedSnapshot {
        version: STORAGE_VERSION,
        protocol_version: PROTOCOL_VERSION.to_string(),
        snapshot_id: "snap-precedence".to_string(),
        repo_id: "repo-1".to_string(),
        repo_root: "/tmp/repo".to_string(),
        base: BaseSnapshot {
            kind: BaseSnapshotKind::GitCommit,
            commit: "abc123".to_string(),
            digest: "base".to_string(),
            file_count: 1,
            files: vec![SnapshotFile {
                path: "src/module.ts".to_string(),
                content_sha256: "sha-base".to_string(),
                content_bytes: include_str!("fixtures/valid/module.ts").len(),
                contents: include_str!("fixtures/valid/module.ts").to_string(),
            }],
        },
        working_tree: WorkingTreeOverlay {
            digest: "work".to_string(),
            entries: vec![WorkingTreeEntry {
                path: "src/module.ts".to_string(),
                kind: OverlayEntryKind::Upsert,
                content_sha256: Some("sha-work".to_string()),
                content_bytes: Some(include_str!("fixtures/valid/plain.js").len()),
                contents: Some(include_str!("fixtures/valid/plain.js").to_string()),
            }],
        },
        buffers: vec![BufferOverlay {
            buffer_id: "buffer-1".to_string(),
            path: "src/module.ts".to_string(),
            version: 2,
            content_sha256: "sha-buffer".to_string(),
            content_bytes: include_str!("fixtures/broken/editing.tsx").len(),
            contents: include_str!("fixtures/broken/editing.tsx").to_string(),
        }],
    };

    let artifact = parser
        .parse_file_from_snapshot(&snapshot, "src/module.ts")
        .unwrap()
        .unwrap();

    assert_eq!(
        artifact.metadata().source_kind,
        ParseInputSourceKind::BufferOverlay
    );
    assert_eq!(
        artifact.contents(),
        include_str!("fixtures/broken/editing.tsx")
    );
    assert!(artifact.has_recoverable_errors());
}

#[test]
fn incremental_reparse_reuses_prior_tree_and_updates_line_mapping() {
    let mut parser = ParseCore::default();
    let snapshot = snapshot_with_file("src/module.ts", include_str!("fixtures/valid/module.ts"));
    let original = parser
        .parse_file_from_snapshot(&snapshot, "src/module.ts")
        .unwrap()
        .unwrap();
    let edited = format!(
        "{}\nexport const finalFlag = true;\n",
        include_str!("fixtures/valid/module.ts")
    );

    let reparsed = parser
        .reparse(&original, &edited, ParseInputSourceKind::BufferOverlay)
        .unwrap();
    let expected_byte = edited.find("export const finalFlag").unwrap();
    let final_line = edited.lines().count() as u32;

    assert!(reparsed.reused_incremental_tree());
    assert!(!reparsed.has_recoverable_errors());
    assert_eq!(
        reparsed.line_index().line_column_to_byte(final_line, 1),
        Some(expected_byte)
    );
}
