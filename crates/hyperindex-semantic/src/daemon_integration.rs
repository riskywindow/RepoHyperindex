use hyperindex_protocol::semantic::{
    SemanticAnalysisState, SemanticBuildRecord, SemanticCapabilities, SemanticDiagnostic,
    SemanticDiagnosticSeverity, SemanticStatusResponse,
};

pub fn scaffold_capabilities(query_ready: bool) -> SemanticCapabilities {
    SemanticCapabilities {
        status: true,
        build: true,
        query: query_ready,
        inspect_chunk: true,
        local_rebuild: true,
        local_stats: true,
    }
}

pub fn info_diagnostic(code: &str, message: impl Into<String>) -> SemanticDiagnostic {
    SemanticDiagnostic {
        severity: SemanticDiagnosticSeverity::Info,
        code: code.to_string(),
        message: message.into(),
    }
}

pub fn warning_diagnostic(code: &str, message: impl Into<String>) -> SemanticDiagnostic {
    SemanticDiagnostic {
        severity: SemanticDiagnosticSeverity::Warning,
        code: code.to_string(),
        message: message.into(),
    }
}

pub fn scaffold_status_response(
    repo_id: &str,
    snapshot_id: &str,
    state: SemanticAnalysisState,
    builds: Vec<SemanticBuildRecord>,
    diagnostics: Vec<SemanticDiagnostic>,
) -> SemanticStatusResponse {
    let query_ready = matches!(state, SemanticAnalysisState::Ready);
    SemanticStatusResponse {
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        state,
        capabilities: scaffold_capabilities(query_ready),
        builds,
        diagnostics,
    }
}
