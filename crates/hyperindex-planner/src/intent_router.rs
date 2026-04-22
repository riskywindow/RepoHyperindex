use hyperindex_protocol::planner::{
    PlannerIntentDecision, PlannerIntentKind, PlannerIntentSource, PlannerQueryParams,
};

use crate::common::normalize_query;

#[derive(Debug, Default, Clone)]
pub struct IntentRouter;

impl IntentRouter {
    pub fn classify(&self, params: &PlannerQueryParams) -> PlannerIntentDecision {
        if let Some(intent_hint) = params.intent_hint.clone() {
            return PlannerIntentDecision {
                selected_intent: intent_hint,
                source: PlannerIntentSource::ExplicitHint,
                reasons: vec!["intent selected from explicit planner hint".to_string()],
            };
        }

        let normalized = normalize_query(&params.query.text);
        let lowered = normalized.to_ascii_lowercase();
        let token_count = normalized.split_whitespace().count();

        let (selected_intent, reasons) = if lowered.contains("impact")
            || lowered.contains("what breaks")
            || lowered.contains("blast radius")
            || lowered.contains("who calls")
        {
            (
                PlannerIntentKind::Impact,
                vec![
                    "query includes impact-oriented language".to_string(),
                    "planner keeps impact as a first-class intent".to_string(),
                ],
            )
        } else if (lowered.starts_with("where ")
            || lowered.starts_with("how ")
            || lowered.starts_with("why "))
            && token_count >= 4
        {
            (
                PlannerIntentKind::Hybrid,
                vec![
                    "query looks like natural-language code navigation".to_string(),
                    "planner keeps multiple route families available in the scaffold".to_string(),
                ],
            )
        } else if lowered.contains('/')
            || lowered.contains("::")
            || lowered.ends_with(".ts")
            || lowered.ends_with(".tsx")
            || lowered.ends_with(".js")
            || lowered.ends_with(".jsx")
        {
            (
                PlannerIntentKind::Lookup,
                vec!["query looks path-like or identifier-oriented".to_string()],
            )
        } else if token_count >= 4 {
            (
                PlannerIntentKind::Semantic,
                vec![
                    "query is multi-token natural language".to_string(),
                    "semantic intent is the least-lossy scaffold fit".to_string(),
                ],
            )
        } else {
            (
                PlannerIntentKind::Lookup,
                vec!["query stays on the deterministic lookup fallback".to_string()],
            )
        };

        PlannerIntentDecision {
            selected_intent,
            source: PlannerIntentSource::Heuristic,
            reasons,
        }
    }
}
