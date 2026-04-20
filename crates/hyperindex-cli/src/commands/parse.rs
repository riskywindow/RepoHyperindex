use std::path::Path;

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::api::{RequestBody, SuccessPayload};
use hyperindex_protocol::symbols::{
    ParseBuildParams, ParseBuildRecord, ParseBuildResponse, ParseInspectFileParams,
    ParseInspectFileResponse, ParseStatusParams, ParseStatusResponse,
};

use crate::client::DaemonClient;

pub fn build(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    force: bool,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::ParseBuild(ParseBuildParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        force,
    }))? {
        SuccessPayload::ParseBuild(response) => render_build(&response, json_output),
        other => Err(unexpected_response("parse_build", other)),
    }
}

pub fn status(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::ParseStatus(ParseStatusParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        build_id: None,
    }))? {
        SuccessPayload::ParseStatus(response) => render_status(&response, json_output),
        other => Err(unexpected_response("parse_status", other)),
    }
}

pub fn inspect_file(
    config_path: Option<&Path>,
    repo_id: &str,
    snapshot_id: &str,
    path: &str,
    include_facts: bool,
    json_output: bool,
) -> HyperindexResult<String> {
    let client = DaemonClient::load(config_path)?;
    match client.send(RequestBody::ParseInspectFile(ParseInspectFileParams {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        path: path.to_string(),
        include_facts,
    }))? {
        SuccessPayload::ParseInspectFile(response) => render_inspect_file(&response, json_output),
        other => Err(unexpected_response("parse_inspect_file", other)),
    }
}

fn render_build(response: &ParseBuildResponse, json_output: bool) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    Ok(render_build_record("parse build", &response.build))
}

fn render_status(response: &ParseStatusResponse, json_output: bool) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }
    match response.builds.first() {
        Some(build) => Ok(render_build_record("parse status", build)),
        None => Ok(format!(
            "parse status {}\nno persisted parse build found",
            response.snapshot_id
        )),
    }
}

fn render_inspect_file(
    response: &ParseInspectFileResponse,
    json_output: bool,
) -> HyperindexResult<String> {
    if json_output {
        return render_json(response);
    }

    let mut rendered = format!(
        "parse inspect {}\nlanguage: {}\nsource: {:?}\nstage: {:?}\nparser_pack: {}\nbytes: {}\ndiagnostics: {}\nsymbols: {}\noccurrences: {}\nedges: {}",
        response.artifact.path,
        language_name(&response.artifact.language),
        response.artifact.source_kind,
        response.artifact.stage,
        response.artifact.parser_pack_id,
        response.artifact.content_bytes,
        response.artifact.diagnostics.len(),
        response.artifact.facts.symbol_count,
        response.artifact.facts.occurrence_count,
        response.artifact.facts.edge_count,
    );

    if let Some(facts) = &response.facts {
        rendered.push_str(&format!(
            "\nfacts: {} symbols / {} occurrences / {} edges",
            facts.symbols.len(),
            facts.occurrences.len(),
            facts.edges.len()
        ));
    }

    for diagnostic in &response.artifact.diagnostics {
        rendered.push_str(&format!(
            "\n- {}:{} {}",
            diagnostic
                .span
                .as_ref()
                .map(|span| span.start.line.to_string())
                .unwrap_or_else(|| "-".to_string()),
            diagnostic
                .span
                .as_ref()
                .map(|span| span.start.column.to_string())
                .unwrap_or_else(|| "-".to_string()),
            diagnostic.message
        ));
    }

    Ok(rendered)
}

fn render_build_record(title: &str, build: &ParseBuildRecord) -> String {
    format!(
        "{title} {}\nstate: {:?}\nfiles_planned: {}\nfiles_parsed: {}\nfiles_reused_from_cache: {}\nfiles_skipped: {}\ndiagnostics: {}\nloaded_from_existing_build: {}",
        build.build_id.0,
        build.state,
        build.counts.planned_file_count,
        build.counts.parsed_file_count,
        build.counts.reused_file_count,
        build.counts.skipped_file_count,
        build.counts.diagnostic_count,
        build.loaded_from_existing_build,
    )
}

fn render_json<T: serde::Serialize>(response: &T) -> HyperindexResult<String> {
    serde_json::to_string_pretty(response)
        .map_err(|error| HyperindexError::Message(format!("failed to render json: {error}")))
}

fn unexpected_response(method: &str, other: SuccessPayload) -> HyperindexError {
    HyperindexError::Message(format!("unexpected {method} response: {other:?}"))
}

fn language_name(language: &hyperindex_protocol::symbols::LanguageId) -> &'static str {
    match language {
        hyperindex_protocol::symbols::LanguageId::Typescript => "typescript",
        hyperindex_protocol::symbols::LanguageId::Tsx => "tsx",
        hyperindex_protocol::symbols::LanguageId::Javascript => "javascript",
        hyperindex_protocol::symbols::LanguageId::Jsx => "jsx",
        hyperindex_protocol::symbols::LanguageId::Mts => "mts",
        hyperindex_protocol::symbols::LanguageId::Cts => "cts",
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::symbols::{
        FileFactsSummary, FileParseArtifactMetadata, LanguageId, ParseArtifactStage,
        ParseBuildCounts, ParseBuildId, ParseBuildRecord, ParseBuildState, ParseInputSourceKind,
        ParseStatusResponse,
    };

    use super::{render_build_record, render_status};

    #[test]
    fn render_status_uses_reused_file_count_and_missing_state() {
        let build = ParseBuildRecord {
            build_id: ParseBuildId("parse-1".to_string()),
            state: ParseBuildState::Succeeded,
            requested_at: "epoch-ms:1".to_string(),
            started_at: Some("epoch-ms:1".to_string()),
            finished_at: Some("epoch-ms:2".to_string()),
            counts: ParseBuildCounts {
                planned_file_count: 3,
                parsed_file_count: 1,
                reused_file_count: 2,
                skipped_file_count: 0,
                diagnostic_count: 0,
            },
            manifest: None,
            loaded_from_existing_build: true,
        };

        let rendered = render_build_record("parse build", &build);
        assert!(rendered.contains("files_reused_from_cache: 2"));
        assert!(rendered.contains("loaded_from_existing_build: true"));

        let empty = render_status(
            &ParseStatusResponse {
                repo_id: "repo-1".to_string(),
                snapshot_id: "snap-1".to_string(),
                builds: Vec::new(),
            },
            false,
        )
        .unwrap();
        assert!(empty.contains("no persisted parse build found"));
    }

    #[test]
    fn inspect_json_shape_roundtrips_new_fields() {
        let response = hyperindex_protocol::symbols::ParseInspectFileResponse {
            repo_id: "repo-1".to_string(),
            snapshot_id: "snap-1".to_string(),
            artifact: FileParseArtifactMetadata {
                artifact_id: "artifact:src/app.ts".to_string(),
                path: "src/app.ts".to_string(),
                language: LanguageId::Typescript,
                source_kind: ParseInputSourceKind::BaseSnapshot,
                stage: ParseArtifactStage::Parsed,
                content_sha256: "sha".to_string(),
                content_bytes: 42,
                parser_pack_id: "ts_js_core".to_string(),
                facts: FileFactsSummary {
                    symbol_count: 1,
                    occurrence_count: 2,
                    edge_count: 3,
                },
                diagnostics: Vec::new(),
            },
            facts: None,
        };

        let encoded = serde_json::to_string_pretty(&response).unwrap();
        assert!(encoded.contains("\"symbol_count\": 1"));
    }
}
