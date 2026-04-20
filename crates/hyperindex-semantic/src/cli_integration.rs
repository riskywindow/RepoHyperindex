use serde::Serialize;

use hyperindex_protocol::semantic::{SemanticQueryResponse, SemanticStatusResponse};

pub fn render_status_response(
    response: &SemanticStatusResponse,
    json_output: bool,
) -> Result<String, serde_json::Error> {
    if json_output {
        return serde_json::to_string_pretty(response);
    }
    Ok([
        format!("semantic status {}", response.snapshot_id),
        format!("repo_id: {}", response.repo_id),
        format!("state: {:?}", response.state).to_lowercase(),
        format!("query_ready: {}", response.capabilities.query),
        format!(
            "build_id: {}",
            response
                .builds
                .first()
                .map(|build| build.build_id.0.clone())
                .unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "diagnostics: {}",
            if response.diagnostics.is_empty() {
                "-".to_string()
            } else {
                response
                    .diagnostics
                    .iter()
                    .map(|diagnostic| format!("{}:{}", diagnostic.code, diagnostic.message))
                    .collect::<Vec<_>>()
                    .join(" | ")
            }
        ),
    ]
    .join("\n"))
}

pub fn render_search_response(
    response: &SemanticQueryResponse,
    json_output: bool,
) -> Result<String, serde_json::Error> {
    if json_output {
        return serde_json::to_string_pretty(response);
    }
    Ok([
        format!("semantic query {}", response.snapshot_id),
        format!("repo_id: {}", response.repo_id),
        format!("query: {}", response.query.text),
        format!("hit_count: {}", response.hits.len()),
        format!(
            "candidate_chunk_count: {}",
            response.stats.candidate_chunk_count
        ),
        format!(
            "filtered_chunk_count: {}",
            response.stats.filtered_chunk_count
        ),
        format!(
            "diagnostics: {}",
            if response.diagnostics.is_empty() {
                "-".to_string()
            } else {
                response
                    .diagnostics
                    .iter()
                    .map(|diagnostic| format!("{}:{}", diagnostic.code, diagnostic.message))
                    .collect::<Vec<_>>()
                    .join(" | ")
            }
        ),
    ]
    .join("\n"))
}

pub fn render_local_report<T: Serialize>(
    report: &T,
    title: &str,
    lines: &[String],
    json_output: bool,
) -> Result<String, serde_json::Error> {
    if json_output {
        return serde_json::to_string_pretty(report);
    }
    let mut rendered = vec![title.to_string()];
    rendered.extend(lines.iter().cloned());
    Ok(rendered.join("\n"))
}
