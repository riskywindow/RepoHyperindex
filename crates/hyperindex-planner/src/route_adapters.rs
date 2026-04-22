use hyperindex_protocol::planner::{
    PlannerAmbiguity, PlannerAnchor, PlannerCandidate, PlannerDiagnostic, PlannerEvidenceItem,
    PlannerFilterCapabilities, PlannerQueryFilters, PlannerQueryIr, PlannerRouteBudget,
    PlannerRouteCapability, PlannerRouteKind,
};
use hyperindex_protocol::snapshot::ComposedSnapshot;
use hyperindex_protocol::symbols::{LanguageId, SourceSpan, SymbolId, SymbolKind};

use crate::daemon_integration::PlannerRuntimeContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannerRouteReadiness {
    Ready,
    Disabled,
    Unavailable,
    Unbuilt,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerRouteConstraints {
    pub requires_unique_target: bool,
    pub emits_engine_local_scores: bool,
    pub returns_file_provenance: bool,
    pub returns_symbol_provenance: bool,
    pub returns_span_provenance: bool,
    pub planner_applies_filters_post_retrieval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerRouteCapabilityReport {
    pub route_kind: PlannerRouteKind,
    pub enabled: bool,
    pub available: bool,
    pub readiness: PlannerRouteReadiness,
    pub reason: Option<String>,
    pub supported_filters: PlannerFilterCapabilities,
    pub constraints: PlannerRouteConstraints,
    pub diagnostics: Vec<PlannerDiagnostic>,
    pub notes: Vec<String>,
}

impl PlannerRouteCapabilityReport {
    pub fn to_public_capability(&self) -> PlannerRouteCapability {
        PlannerRouteCapability {
            route_kind: self.route_kind.clone(),
            enabled: self.enabled,
            available: self.available,
            reason: self.reason.clone(),
        }
    }

    pub fn unsupported_filters(&self, filters: &PlannerQueryFilters) -> Vec<&'static str> {
        let mut unsupported = Vec::new();
        if !filters.path_globs.is_empty() && !self.supported_filters.path_globs {
            unsupported.push("path_globs");
        }
        if !filters.package_names.is_empty() && !self.supported_filters.package_names {
            unsupported.push("package_names");
        }
        if !filters.package_roots.is_empty() && !self.supported_filters.package_roots {
            unsupported.push("package_roots");
        }
        if !filters.workspace_roots.is_empty() && !self.supported_filters.workspace_roots {
            unsupported.push("workspace_roots");
        }
        if !filters.languages.is_empty() && !self.supported_filters.languages {
            unsupported.push("languages");
        }
        if !filters.extensions.is_empty() && !self.supported_filters.extensions {
            unsupported.push("extensions");
        }
        if !filters.symbol_kinds.is_empty() && !self.supported_filters.symbol_kinds {
            unsupported.push("symbol_kinds");
        }
        unsupported
    }
}

#[derive(Debug, Clone)]
pub struct PlannerRouteRequest<'a> {
    pub runtime: &'a PlannerRuntimeContext,
    pub snapshot: &'a ComposedSnapshot,
    pub ir: &'a PlannerQueryIr,
    pub budget: PlannerRouteBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannerRouteExecutionState {
    Deferred,
    Executed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPlannerCandidate {
    pub candidate_id: String,
    pub route_kind: PlannerRouteKind,
    pub engine_type: PlannerRouteKind,
    pub label: String,
    pub anchor: PlannerAnchor,
    pub rank: Option<u32>,
    pub engine_score: Option<u32>,
    pub normalized_score: Option<u32>,
    pub primary_path: Option<String>,
    pub primary_symbol_id: Option<SymbolId>,
    pub primary_span: Option<SourceSpan>,
    pub language: Option<LanguageId>,
    pub extension: Option<String>,
    pub symbol_kind: Option<SymbolKind>,
    pub package_name: Option<String>,
    pub package_root: Option<String>,
    pub workspace_root: Option<String>,
    pub evidence: Vec<PlannerEvidenceItem>,
    pub engine_diagnostics: Vec<String>,
    pub notes: Vec<String>,
}

impl NormalizedPlannerCandidate {
    pub fn to_public_candidate(&self) -> PlannerCandidate {
        let mut notes = self.notes.clone();
        notes.extend(
            self.engine_diagnostics
                .iter()
                .map(|diagnostic| format!("engine diagnostic: {diagnostic}")),
        );

        PlannerCandidate {
            candidate_id: self.candidate_id.clone(),
            route_kind: self.route_kind.clone(),
            label: self.label.clone(),
            anchor: self.anchor.clone(),
            rank: self.rank,
            route_score: self.engine_score,
            normalized_score: self.normalized_score,
            evidence: self.evidence.clone(),
            notes,
        }
    }

    pub fn matches_filters(
        &self,
        filters: &PlannerQueryFilters,
        snapshot: &ComposedSnapshot,
    ) -> bool {
        if !filters.path_globs.is_empty() {
            let Some(path) = self.path() else {
                return false;
            };
            if !filters
                .path_globs
                .iter()
                .any(|pattern| wildcard_match(pattern, path))
            {
                return false;
            }
        }

        if !filters.package_names.is_empty() {
            let Some(package_name) = self.package_name() else {
                return false;
            };
            if !filters
                .package_names
                .iter()
                .any(|value| value == package_name)
            {
                return false;
            }
        }

        if !filters.package_roots.is_empty() {
            let Some(package_root) = self.package_root() else {
                return false;
            };
            if !filters
                .package_roots
                .iter()
                .any(|value| value == package_root)
            {
                return false;
            }
        }

        if !filters.workspace_roots.is_empty() {
            let workspace_root = self
                .workspace_root
                .as_deref()
                .unwrap_or(snapshot.repo_root.as_str());
            if !filters
                .workspace_roots
                .iter()
                .any(|value| value == workspace_root)
            {
                return false;
            }
        }

        if !filters.languages.is_empty() {
            let Some(language) = self.language.as_ref() else {
                return false;
            };
            if !filters.languages.iter().any(|value| value == language) {
                return false;
            }
        }

        if !filters.extensions.is_empty() {
            let Some(extension) = self.extension() else {
                return false;
            };
            if !filters.extensions.iter().any(|value| {
                value
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(extension)
            }) {
                return false;
            }
        }

        if !filters.symbol_kinds.is_empty() {
            let Some(symbol_kind) = self.symbol_kind.as_ref() else {
                return false;
            };
            if !filters
                .symbol_kinds
                .iter()
                .any(|value| value == symbol_kind)
            {
                return false;
            }
        }

        true
    }

    pub fn path(&self) -> Option<&str> {
        if let Some(path) = self.primary_path.as_deref() {
            return Some(path);
        }
        match &self.anchor {
            PlannerAnchor::Symbol { path, .. }
            | PlannerAnchor::Span { path, .. }
            | PlannerAnchor::File { path } => Some(path.as_str()),
            PlannerAnchor::Impact { entity } => match entity {
                hyperindex_protocol::impact::ImpactEntityRef::Symbol { path, .. }
                | hyperindex_protocol::impact::ImpactEntityRef::File { path }
                | hyperindex_protocol::impact::ImpactEntityRef::Test { path, .. } => {
                    Some(path.as_str())
                }
                hyperindex_protocol::impact::ImpactEntityRef::Package { .. } => None,
            },
            PlannerAnchor::Package { .. } | PlannerAnchor::Workspace { .. } => None,
        }
    }

    pub fn package_name(&self) -> Option<&str> {
        if let Some(package_name) = self.package_name.as_deref() {
            return Some(package_name);
        }
        match &self.anchor {
            PlannerAnchor::Package { package_name, .. } => Some(package_name.as_str()),
            PlannerAnchor::Impact { entity } => match entity {
                hyperindex_protocol::impact::ImpactEntityRef::Package { package_name, .. } => {
                    Some(package_name.as_str())
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub fn package_root(&self) -> Option<&str> {
        if let Some(package_root) = self.package_root.as_deref() {
            return Some(package_root);
        }
        match &self.anchor {
            PlannerAnchor::Package { package_root, .. } => Some(package_root.as_str()),
            PlannerAnchor::Impact { entity } => match entity {
                hyperindex_protocol::impact::ImpactEntityRef::Package { package_root, .. } => {
                    Some(package_root.as_str())
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn extension(&self) -> Option<&str> {
        self.extension.as_deref().or_else(|| {
            self.path()?
                .rsplit_once('.')
                .map(|(_, extension)| extension)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerRouteExecution {
    pub state: PlannerRouteExecutionState,
    pub candidates: Vec<NormalizedPlannerCandidate>,
    pub diagnostics: Vec<PlannerDiagnostic>,
    pub notes: Vec<String>,
    pub elapsed_ms: u64,
    pub ambiguity: Option<PlannerAmbiguity>,
}

pub trait PlannerRouteAdapter: std::fmt::Debug + Send + Sync {
    fn kind(&self) -> PlannerRouteKind;

    fn capability(
        &self,
        context: &PlannerRuntimeContext,
        snapshot: &ComposedSnapshot,
    ) -> PlannerRouteCapabilityReport;

    fn execute(&self, request: &PlannerRouteRequest<'_>) -> PlannerRouteExecution;
}

pub fn full_filter_capabilities() -> PlannerFilterCapabilities {
    PlannerFilterCapabilities {
        path_globs: true,
        package_names: true,
        package_roots: true,
        workspace_roots: true,
        languages: true,
        extensions: true,
        symbol_kinds: true,
    }
}

pub fn empty_filter_capabilities() -> PlannerFilterCapabilities {
    PlannerFilterCapabilities {
        path_globs: false,
        package_names: false,
        package_roots: false,
        workspace_roots: false,
        languages: false,
        extensions: false,
        symbol_kinds: false,
    }
}

fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    wildcard_match_bytes(pattern.as_bytes(), candidate.as_bytes())
}

fn wildcard_match_bytes(pattern: &[u8], candidate: &[u8]) -> bool {
    if pattern.is_empty() {
        return candidate.is_empty();
    }

    match pattern[0] {
        b'*' => {
            wildcard_match_bytes(&pattern[1..], candidate)
                || (!candidate.is_empty() && wildcard_match_bytes(pattern, &candidate[1..]))
        }
        b'?' => !candidate.is_empty() && wildcard_match_bytes(&pattern[1..], &candidate[1..]),
        byte => {
            !candidate.is_empty()
                && byte.eq_ignore_ascii_case(&candidate[0])
                && wildcard_match_bytes(&pattern[1..], &candidate[1..])
        }
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::planner::{
        PlannerAnchor, PlannerEvidenceItem, PlannerEvidenceKind, PlannerRouteKind,
    };
    use hyperindex_protocol::snapshot::{
        BaseSnapshot, BaseSnapshotKind, ComposedSnapshot, WorkingTreeOverlay,
    };
    use hyperindex_protocol::symbols::{
        ByteRange, LanguageId, LinePosition, SourceSpan, SymbolId, SymbolKind,
    };
    use hyperindex_protocol::{PROTOCOL_VERSION, STORAGE_VERSION};

    use super::NormalizedPlannerCandidate;

    fn snapshot() -> ComposedSnapshot {
        ComposedSnapshot {
            version: STORAGE_VERSION,
            protocol_version: PROTOCOL_VERSION.to_string(),
            repo_id: "repo-123".to_string(),
            repo_root: "/tmp/repo".to_string(),
            snapshot_id: "snap-123".to_string(),
            base: BaseSnapshot {
                kind: BaseSnapshotKind::GitCommit,
                commit: "deadbeef".to_string(),
                digest: "base".to_string(),
                file_count: 0,
                files: Vec::new(),
            },
            working_tree: WorkingTreeOverlay {
                digest: "work".to_string(),
                entries: Vec::new(),
            },
            buffers: Vec::new(),
        }
    }

    fn span() -> SourceSpan {
        SourceSpan {
            start: LinePosition { line: 1, column: 0 },
            end: LinePosition {
                line: 1,
                column: 12,
            },
            bytes: ByteRange { start: 0, end: 12 },
        }
    }

    fn candidate() -> NormalizedPlannerCandidate {
        NormalizedPlannerCandidate {
            candidate_id: "symbol:sym.invalidateSession".to_string(),
            route_kind: PlannerRouteKind::Symbol,
            engine_type: PlannerRouteKind::Symbol,
            label: "invalidateSession".to_string(),
            anchor: PlannerAnchor::Symbol {
                symbol_id: SymbolId("sym.invalidateSession".to_string()),
                path: "packages/auth/src/session/service.ts".to_string(),
                span: Some(span()),
            },
            rank: Some(1),
            engine_score: Some(98),
            normalized_score: None,
            primary_path: Some("packages/auth/src/session/service.ts".to_string()),
            primary_symbol_id: Some(SymbolId("sym.invalidateSession".to_string())),
            primary_span: Some(span()),
            language: Some(LanguageId::Typescript),
            extension: Some("ts".to_string()),
            symbol_kind: Some(SymbolKind::Function),
            package_name: Some("@hyperindex/auth".to_string()),
            package_root: Some("packages/auth".to_string()),
            workspace_root: Some("/tmp/repo".to_string()),
            evidence: vec![PlannerEvidenceItem {
                evidence_kind: PlannerEvidenceKind::SymbolHit,
                route_kind: PlannerRouteKind::Symbol,
                label: "exact symbol lookup".to_string(),
                path: Some("packages/auth/src/session/service.ts".to_string()),
                span: Some(span()),
                symbol_id: Some(SymbolId("sym.invalidateSession".to_string())),
                impact_entity: None,
                snippet: None,
                score: Some(98),
                notes: Vec::new(),
            }],
            engine_diagnostics: vec!["symbol build loaded from existing snapshot".to_string()],
            notes: vec!["match_kind=exact".to_string()],
        }
    }

    #[test]
    fn normalized_candidate_matches_supported_filters() {
        let candidate = candidate();
        let filters = hyperindex_protocol::planner::PlannerQueryFilters {
            path_globs: vec!["packages/**".to_string()],
            package_names: vec!["@hyperindex/auth".to_string()],
            package_roots: vec!["packages/auth".to_string()],
            workspace_roots: vec!["/tmp/repo".to_string()],
            languages: vec![LanguageId::Typescript],
            extensions: vec![".ts".to_string()],
            symbol_kinds: vec![SymbolKind::Function],
        };

        assert!(candidate.matches_filters(&filters, &snapshot()));
    }

    #[test]
    fn normalized_candidate_rejects_missing_filter_metadata() {
        let mut candidate = candidate();
        candidate.package_name = None;
        let filters = hyperindex_protocol::planner::PlannerQueryFilters {
            package_names: vec!["@hyperindex/auth".to_string()],
            ..hyperindex_protocol::planner::PlannerQueryFilters::default()
        };

        assert!(!candidate.matches_filters(&filters, &snapshot()));
    }
}
