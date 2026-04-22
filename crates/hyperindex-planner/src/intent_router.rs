use hyperindex_protocol::planner::{
    PlannerMode, PlannerModeDecision, PlannerModeSelectionSource, PlannerQueryParams,
};

use crate::common::normalize_query;

#[derive(Debug, Default, Clone)]
pub struct IntentRouter;

impl IntentRouter {
    pub fn classify(
        &self,
        params: &PlannerQueryParams,
        default_mode: PlannerMode,
    ) -> PlannerModeDecision {
        if let Some(mode_override) = params.mode_override.clone() {
            if !matches!(mode_override, PlannerMode::Auto) {
                return PlannerModeDecision {
                    requested_mode: Some(mode_override.clone()),
                    selected_mode: mode_override,
                    source: PlannerModeSelectionSource::ExplicitOverride,
                    reasons: vec!["planner mode selected from explicit override".to_string()],
                };
            }
        }

        let normalized = normalize_query(&params.query.text);
        let lowered = normalized.to_ascii_lowercase();
        let token_count = normalized.split_whitespace().count();

        let (selected_mode, reasons) = if lowered.contains("impact")
            || lowered.contains("what breaks")
            || lowered.contains("blast radius")
            || lowered.contains("who calls")
            || lowered.contains("invalidate")
        {
            (
                PlannerMode::Impact,
                vec![
                    "query includes impact-oriented language".to_string(),
                    "impact remains the product wedge for blast-radius style questions".to_string(),
                ],
            )
        } else if lowered.contains('/')
            || lowered.contains("::")
            || lowered.ends_with(".ts")
            || lowered.ends_with(".tsx")
            || lowered.ends_with(".js")
            || lowered.ends_with(".jsx")
            || token_count <= 2
        {
            (
                PlannerMode::Symbol,
                vec!["query looks path-like or identifier-oriented".to_string()],
            )
        } else if token_count >= 4 {
            (
                PlannerMode::Semantic,
                vec![
                    "query is multi-token natural language".to_string(),
                    "semantic mode is the least-lossy deterministic fit".to_string(),
                ],
            )
        } else {
            (
                default_mode,
                vec!["query fell back to the configured planner default mode".to_string()],
            )
        };

        PlannerModeDecision {
            requested_mode: params.mode_override.clone(),
            selected_mode,
            source: PlannerModeSelectionSource::Heuristic,
            reasons,
        }
    }
}
