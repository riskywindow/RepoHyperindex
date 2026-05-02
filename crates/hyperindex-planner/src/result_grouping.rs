use std::collections::BTreeMap;

use hyperindex_protocol::planner::{
    PlannerExplanationPayload, PlannerResultGroup, PlannerRouteKind, PlannerTrustPayload,
    PlannerTrustTier,
};

use crate::score_fusion::FusedCandidate;

#[derive(Debug, Default, Clone)]
pub struct ResultGrouping;

impl ResultGrouping {
    pub(crate) fn group(
        &self,
        fused_candidates: Vec<FusedCandidate>,
        limit: u32,
    ) -> Vec<PlannerResultGroup> {
        if fused_candidates.is_empty() {
            return Vec::new();
        }

        let mut group_map: BTreeMap<String, Vec<FusedCandidate>> = BTreeMap::new();

        for candidate in fused_candidates {
            let group_key = grouping_key(&candidate);
            group_map.entry(group_key).or_default().push(candidate);
        }

        let mut groups: Vec<PlannerResultGroup> = group_map
            .into_iter()
            .map(|(group_id, members)| build_group(group_id, members))
            .collect();

        groups.sort_by(|a, b| {
            let score_a = a.score.unwrap_or(0);
            let score_b = b.score.unwrap_or(0);
            score_b
                .cmp(&score_a)
                .then_with(|| a.group_id.cmp(&b.group_id))
        });

        groups.truncate(limit as usize);
        groups
    }
}

fn grouping_key(candidate: &FusedCandidate) -> String {
    if let Some(symbol_id) = &candidate.primary.primary_symbol_id {
        return format!("group:symbol:{}", symbol_id.0);
    }
    if let Some(path) = &candidate.primary.primary_path {
        return format!("group:file:{path}");
    }
    format!("group:standalone:{}", candidate.primary.candidate_id)
}

fn build_group(group_id: String, mut members: Vec<FusedCandidate>) -> PlannerResultGroup {
    members.sort_by(|a, b| {
        b.fused_score
            .cmp(&a.fused_score)
            .then_with(|| a.primary.candidate_id.cmp(&b.primary.candidate_id))
    });

    let best = &members[0];
    let score = members.iter().map(|m| m.fused_score).max().unwrap_or(0);

    let mut routes = Vec::new();
    let mut evidence = Vec::new();
    for member in &members {
        for route in &member.contributing_routes {
            if !routes.contains(route) {
                routes.push(route.clone());
            }
        }
        evidence.extend(member.merged_evidence.iter().cloned());
    }

    let member_count = members.len();
    let explanation = PlannerExplanationPayload {
        template_id: "planner.group.fused".to_string(),
        summary: if member_count == 1 {
            format!("1 candidate from {} route(s)", routes.len())
        } else {
            format!("{member_count} candidates from {} route(s)", routes.len())
        },
        details: Vec::new(),
    };

    PlannerResultGroup {
        group_id,
        label: best.label.clone(),
        anchor: Some(best.anchor.clone()),
        routes,
        trust: default_trust(evidence.len() as u32, routes_len_for_trust(&members)),
        explanation,
        evidence,
        score: Some(score),
    }
}

fn routes_len_for_trust(members: &[FusedCandidate]) -> u32 {
    let mut all_routes: Vec<&PlannerRouteKind> = Vec::new();
    for member in members {
        for route in &member.contributing_routes {
            if !all_routes.contains(&route) {
                all_routes.push(route);
            }
        }
    }
    all_routes.len() as u32
}

fn default_trust(evidence_count: u32, route_agreement_count: u32) -> PlannerTrustPayload {
    PlannerTrustPayload {
        tier: PlannerTrustTier::NeedsReview,
        deterministic: false,
        evidence_count,
        route_agreement_count,
        template_id: "planner.trust.structural_default".to_string(),
        reasons: vec!["trust decoration is deferred; structural default applied".to_string()],
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::planner::{
        PlannerAnchor, PlannerEvidenceItem, PlannerEvidenceKind, PlannerRouteKind,
    };
    use hyperindex_protocol::symbols::SymbolId;

    use crate::route_adapters::NormalizedPlannerCandidate;
    use crate::score_fusion::{DedupKey, FusedCandidate};

    use super::*;

    fn fused_candidate(
        id: &str,
        score: u32,
        routes: Vec<PlannerRouteKind>,
        symbol_id: Option<&str>,
        path: Option<&str>,
    ) -> FusedCandidate {
        let anchor = if let Some(sym) = symbol_id {
            PlannerAnchor::Symbol {
                symbol_id: SymbolId(sym.to_string()),
                path: path.unwrap_or("src/main.rs").to_string(),
                span: None,
            }
        } else if let Some(p) = path {
            PlannerAnchor::File {
                path: p.to_string(),
            }
        } else {
            PlannerAnchor::File {
                path: "unknown".to_string(),
            }
        };

        let evidence: Vec<PlannerEvidenceItem> = routes
            .iter()
            .map(|route| PlannerEvidenceItem {
                evidence_kind: match route {
                    PlannerRouteKind::Semantic => PlannerEvidenceKind::SemanticHit,
                    PlannerRouteKind::Impact => PlannerEvidenceKind::ImpactHit,
                    _ => PlannerEvidenceKind::SymbolHit,
                },
                route_kind: route.clone(),
                label: format!("evidence from {id}"),
                path: path.map(|s| s.to_string()),
                span: None,
                symbol_id: symbol_id.map(|s| SymbolId(s.to_string())),
                impact_entity: None,
                snippet: None,
                score: Some(score),
                notes: Vec::new(),
            })
            .collect();

        let dedup_key = if let Some(sym) = symbol_id {
            DedupKey::Symbol(SymbolId(sym.to_string()))
        } else if let Some(p) = path {
            DedupKey::Path(p.to_string())
        } else {
            DedupKey::Unique(id.to_string())
        };

        FusedCandidate {
            dedup_key,
            fused_score: score,
            contributing_routes: routes,
            primary: NormalizedPlannerCandidate {
                candidate_id: id.to_string(),
                route_kind: PlannerRouteKind::Symbol,
                engine_type: PlannerRouteKind::Symbol,
                label: id.to_string(),
                anchor: anchor.clone(),
                rank: Some(1),
                engine_score: Some(score),
                normalized_score: None,
                primary_path: path.map(|s| s.to_string()),
                primary_symbol_id: symbol_id.map(|s| SymbolId(s.to_string())),
                primary_span: None,
                language: None,
                extension: None,
                symbol_kind: None,
                package_name: None,
                package_root: None,
                workspace_root: None,
                evidence: Vec::new(),
                engine_diagnostics: Vec::new(),
                notes: Vec::new(),
            },
            merged_evidence: evidence,
            anchor,
            label: id.to_string(),
        }
    }

    #[test]
    fn groups_by_symbol_id() {
        let candidates = vec![
            fused_candidate(
                "a",
                90,
                vec![PlannerRouteKind::Symbol],
                Some("sym.Foo"),
                Some("src/foo.rs"),
            ),
            fused_candidate(
                "b",
                80,
                vec![PlannerRouteKind::Semantic],
                Some("sym.Foo"),
                Some("src/foo.rs"),
            ),
        ];

        let groups = ResultGrouping.group(candidates, 10);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group_id, "group:symbol:sym.Foo");
    }

    #[test]
    fn groups_by_file_path() {
        let candidates = vec![
            fused_candidate(
                "a",
                90,
                vec![PlannerRouteKind::Symbol],
                None,
                Some("src/foo.rs"),
            ),
            fused_candidate(
                "b",
                80,
                vec![PlannerRouteKind::Semantic],
                None,
                Some("src/foo.rs"),
            ),
        ];

        let groups = ResultGrouping.group(candidates, 10);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group_id, "group:file:src/foo.rs");
    }

    #[test]
    fn standalone_ungroupable() {
        let candidates = vec![
            fused_candidate("a", 90, vec![PlannerRouteKind::Symbol], None, None),
            fused_candidate("b", 80, vec![PlannerRouteKind::Semantic], None, None),
        ];

        let groups = ResultGrouping.group(candidates, 10);
        assert_eq!(groups.len(), 2);
        assert!(groups[0].group_id.starts_with("group:standalone:"));
        assert!(groups[1].group_id.starts_with("group:standalone:"));
    }

    #[test]
    fn group_score_is_max_member_score() {
        let candidates = vec![
            fused_candidate(
                "a",
                90,
                vec![PlannerRouteKind::Symbol],
                Some("sym.Foo"),
                Some("src/foo.rs"),
            ),
            fused_candidate(
                "b",
                95,
                vec![PlannerRouteKind::Semantic],
                Some("sym.Foo"),
                Some("src/foo.rs"),
            ),
        ];

        let groups = ResultGrouping.group(candidates, 10);
        assert_eq!(groups[0].score, Some(95));
    }

    #[test]
    fn group_evidence_preserves_all_route_kinds() {
        let candidates = vec![
            fused_candidate(
                "a",
                90,
                vec![PlannerRouteKind::Symbol],
                Some("sym.Foo"),
                Some("src/foo.rs"),
            ),
            fused_candidate(
                "b",
                80,
                vec![PlannerRouteKind::Semantic],
                Some("sym.Foo"),
                Some("src/foo.rs"),
            ),
        ];

        let groups = ResultGrouping.group(candidates, 10);
        let route_kinds: Vec<PlannerRouteKind> = groups[0]
            .evidence
            .iter()
            .map(|e| e.route_kind.clone())
            .collect();
        assert!(route_kinds.contains(&PlannerRouteKind::Symbol));
        assert!(route_kinds.contains(&PlannerRouteKind::Semantic));
    }

    #[test]
    fn groups_ordered_by_score_descending() {
        let candidates = vec![
            fused_candidate(
                "low",
                50,
                vec![PlannerRouteKind::Symbol],
                Some("sym.Low"),
                Some("src/low.rs"),
            ),
            fused_candidate(
                "high",
                100,
                vec![PlannerRouteKind::Exact],
                Some("sym.High"),
                Some("src/high.rs"),
            ),
            fused_candidate(
                "mid",
                75,
                vec![PlannerRouteKind::Semantic],
                Some("sym.Mid"),
                Some("src/mid.rs"),
            ),
        ];

        let groups = ResultGrouping.group(candidates, 10);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].score, Some(100));
        assert_eq!(groups[1].score, Some(75));
        assert_eq!(groups[2].score, Some(50));
    }

    #[test]
    fn limit_truncates_groups() {
        let candidates = vec![
            fused_candidate(
                "a",
                100,
                vec![PlannerRouteKind::Symbol],
                Some("sym.A"),
                Some("a.rs"),
            ),
            fused_candidate(
                "b",
                90,
                vec![PlannerRouteKind::Symbol],
                Some("sym.B"),
                Some("b.rs"),
            ),
            fused_candidate(
                "c",
                80,
                vec![PlannerRouteKind::Symbol],
                Some("sym.C"),
                Some("c.rs"),
            ),
            fused_candidate(
                "d",
                70,
                vec![PlannerRouteKind::Symbol],
                Some("sym.D"),
                Some("d.rs"),
            ),
        ];

        let groups = ResultGrouping.group(candidates, 2);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].score, Some(100));
        assert_eq!(groups[1].score, Some(90));
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let groups = ResultGrouping.group(Vec::new(), 10);
        assert!(groups.is_empty());
    }

    #[test]
    fn grouping_is_deterministic() {
        let candidates = vec![
            fused_candidate(
                "a",
                100,
                vec![PlannerRouteKind::Symbol],
                Some("sym.A"),
                Some("a.rs"),
            ),
            fused_candidate(
                "b",
                90,
                vec![PlannerRouteKind::Semantic],
                Some("sym.B"),
                Some("b.rs"),
            ),
            fused_candidate(
                "c",
                80,
                vec![PlannerRouteKind::Exact],
                Some("sym.C"),
                Some("c.rs"),
            ),
        ];

        let first = ResultGrouping.group(candidates.clone(), 10);
        for _ in 0..10 {
            let result = ResultGrouping.group(candidates.clone(), 10);
            assert_eq!(first, result);
        }
    }
}
