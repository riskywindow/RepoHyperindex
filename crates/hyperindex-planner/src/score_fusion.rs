use hyperindex_protocol::planner::{
    PlannerAnchor, PlannerEvidenceItem, PlannerRouteKind, PlannerRouteTrace,
};
use hyperindex_protocol::symbols::{SourceSpan, SymbolId};

use crate::route_adapters::NormalizedPlannerCandidate;
use crate::route_policy::PlannerRoutePolicyKind;

#[derive(Debug, Default, Clone)]
pub struct ScoreFusion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FusedCandidate {
    pub dedup_key: DedupKey,
    pub fused_score: u32,
    pub contributing_routes: Vec<PlannerRouteKind>,
    pub primary: NormalizedPlannerCandidate,
    pub merged_evidence: Vec<PlannerEvidenceItem>,
    pub anchor: PlannerAnchor,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub(crate) enum DedupKey {
    Symbol(SymbolId),
    PathSpan {
        path: String,
        byte_start: u32,
        byte_end: u32,
    },
    Path(String),
    Unique(String),
}

impl ScoreFusion {
    pub(crate) fn fuse(
        &self,
        candidates: &[NormalizedPlannerCandidate],
        _traces: &[PlannerRouteTrace],
        _route_policy: &PlannerRoutePolicyKind,
    ) -> Vec<FusedCandidate> {
        if candidates.is_empty() {
            return Vec::new();
        }

        let normalized = candidates
            .iter()
            .map(|c| (c.clone(), normalize_score(c)))
            .collect::<Vec<_>>();

        let mut dedup_groups: Vec<(DedupKey, Vec<(NormalizedPlannerCandidate, u32)>)> = Vec::new();

        for (candidate, norm_score) in normalized {
            let key = dedup_key_for(&candidate);
            let mut merged_into = None;

            for (index, (existing_key, existing_members)) in dedup_groups.iter().enumerate() {
                if should_merge(existing_key, existing_members, &key, &candidate) {
                    merged_into = Some(index);
                    break;
                }
            }

            if let Some(index) = merged_into {
                dedup_groups[index].1.push((candidate, norm_score));
            } else {
                dedup_groups.push((key, vec![(candidate, norm_score)]));
            }
        }

        let mut fused: Vec<FusedCandidate> = dedup_groups
            .into_iter()
            .map(|(key, members)| build_fused(key, members))
            .collect();

        fused.sort_by(deterministic_cmp);
        fused
    }
}

fn normalize_score(candidate: &NormalizedPlannerCandidate) -> u32 {
    let raw = match candidate.engine_score {
        Some(score) => score,
        None => return 0,
    };

    let max_raw = max_raw_for(&candidate.engine_type);
    std::cmp::min(raw.saturating_mul(100) / max_raw.max(1), 100)
}

fn max_raw_for(engine_type: &PlannerRouteKind) -> u32 {
    match engine_type {
        PlannerRouteKind::Exact => 1100,
        PlannerRouteKind::Symbol => 1100,
        PlannerRouteKind::Semantic => 1_000_000,
        PlannerRouteKind::Impact => 1000,
    }
}

fn dedup_key_for(candidate: &NormalizedPlannerCandidate) -> DedupKey {
    if let Some(symbol_id) = &candidate.primary_symbol_id {
        return DedupKey::Symbol(symbol_id.clone());
    }
    if let Some(path) = &candidate.primary_path {
        if let Some(span) = &candidate.primary_span {
            return DedupKey::PathSpan {
                path: path.clone(),
                byte_start: span.bytes.start,
                byte_end: span.bytes.end,
            };
        }
        return DedupKey::Path(path.clone());
    }
    DedupKey::Unique(candidate.candidate_id.clone())
}

fn should_merge(
    existing_key: &DedupKey,
    existing_members: &[(NormalizedPlannerCandidate, u32)],
    candidate_key: &DedupKey,
    candidate: &NormalizedPlannerCandidate,
) -> bool {
    // 1. Same symbol_id → merge
    if let (DedupKey::Symbol(a), DedupKey::Symbol(b)) = (existing_key, candidate_key) {
        if a == b {
            return true;
        }
    }
    // Also check if candidate has a symbol that matches the group's symbol key
    if let Some(sym_id) = &candidate.primary_symbol_id {
        if matches!(existing_key, DedupKey::Symbol(k) if k == sym_id) {
            return true;
        }
    }

    // 2. Same path + overlapping span byte ranges → merge
    if let (Some(path), Some(span)) = (&candidate.primary_path, &candidate.primary_span) {
        for (existing, _) in existing_members {
            if let (Some(e_path), Some(e_span)) = (&existing.primary_path, &existing.primary_span)
                && path == e_path
                && spans_overlap(span, e_span)
            {
                return true;
            }
        }
    }

    // 3. Same path (file-level, no symbol/span) → merge
    if let (DedupKey::Path(a), DedupKey::Path(b)) = (existing_key, candidate_key) {
        if a == b {
            return true;
        }
    }

    // 4. Same unique id
    if existing_key == candidate_key {
        return true;
    }

    false
}

fn spans_overlap(a: &SourceSpan, b: &SourceSpan) -> bool {
    a.bytes.start < b.bytes.end && b.bytes.start < a.bytes.end
}

fn build_fused(
    key: DedupKey,
    mut members: Vec<(NormalizedPlannerCandidate, u32)>,
) -> FusedCandidate {
    // Sort members by normalized score descending, then by route priority
    members.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| route_priority(&b.0.engine_type).cmp(&route_priority(&a.0.engine_type)))
    });

    let (primary, best_score) = members[0].clone();

    let mut contributing_routes = Vec::new();
    let mut merged_evidence = Vec::new();
    for (member, _) in &members {
        if !contributing_routes.contains(&member.route_kind) {
            contributing_routes.push(member.route_kind.clone());
        }
        merged_evidence.extend(member.evidence.iter().cloned());
    }

    let num_routes = contributing_routes.len() as u32;
    let agreement_bonus = std::cmp::min((num_routes.saturating_sub(1)) * 2, 6);
    let fused_score = best_score + agreement_bonus;

    FusedCandidate {
        dedup_key: key,
        fused_score,
        contributing_routes,
        anchor: primary.anchor.clone(),
        label: primary.label.clone(),
        primary,
        merged_evidence,
    }
}

fn route_priority(route_kind: &PlannerRouteKind) -> u32 {
    match route_kind {
        PlannerRouteKind::Exact => 4,
        PlannerRouteKind::Symbol => 3,
        PlannerRouteKind::Semantic => 2,
        PlannerRouteKind::Impact => 1,
    }
}

fn best_route_priority(candidate: &FusedCandidate) -> u32 {
    candidate
        .contributing_routes
        .iter()
        .map(route_priority)
        .max()
        .unwrap_or(0)
}

fn deterministic_cmp(a: &FusedCandidate, b: &FusedCandidate) -> std::cmp::Ordering {
    // 1. Higher fused_score first
    b.fused_score
        .cmp(&a.fused_score)
        // 2. Higher route priority first
        .then_with(|| best_route_priority(b).cmp(&best_route_priority(a)))
        // 3. Lower engine rank first
        .then_with(|| {
            let a_rank = a.primary.rank.unwrap_or(u32::MAX);
            let b_rank = b.primary.rank.unwrap_or(u32::MAX);
            a_rank.cmp(&b_rank)
        })
        // 4. Lexicographic candidate_id
        .then_with(|| a.primary.candidate_id.cmp(&b.primary.candidate_id))
}

#[cfg(test)]
mod tests {
    use hyperindex_protocol::planner::{
        PlannerAnchor, PlannerEvidenceItem, PlannerEvidenceKind, PlannerRouteKind,
    };
    use hyperindex_protocol::symbols::{ByteRange, LinePosition, SourceSpan, SymbolId};

    use crate::route_adapters::NormalizedPlannerCandidate;
    use crate::route_policy::PlannerRoutePolicyKind;

    use super::*;

    fn span_at(start: u32, end: u32) -> SourceSpan {
        SourceSpan {
            start: LinePosition {
                line: 1,
                column: start,
            },
            end: LinePosition {
                line: 1,
                column: end,
            },
            bytes: ByteRange { start, end },
        }
    }

    fn candidate_with(
        id: &str,
        route: PlannerRouteKind,
        engine_score: Option<u32>,
        rank: Option<u32>,
        symbol_id: Option<&str>,
        path: Option<&str>,
        span: Option<SourceSpan>,
    ) -> NormalizedPlannerCandidate {
        let evidence_kind = match &route {
            PlannerRouteKind::Semantic => PlannerEvidenceKind::SemanticHit,
            PlannerRouteKind::Impact => PlannerEvidenceKind::ImpactHit,
            _ => PlannerEvidenceKind::SymbolHit,
        };
        NormalizedPlannerCandidate {
            candidate_id: id.to_string(),
            route_kind: route.clone(),
            engine_type: route.clone(),
            label: id.to_string(),
            anchor: if let Some(sym) = symbol_id {
                PlannerAnchor::Symbol {
                    symbol_id: SymbolId(sym.to_string()),
                    path: path.unwrap_or("src/main.rs").to_string(),
                    span: span.clone(),
                }
            } else if let Some(p) = path {
                PlannerAnchor::File {
                    path: p.to_string(),
                }
            } else {
                PlannerAnchor::File {
                    path: "unknown".to_string(),
                }
            },
            rank,
            engine_score,
            normalized_score: None,
            primary_path: path.map(|s| s.to_string()),
            primary_symbol_id: symbol_id.map(|s| SymbolId(s.to_string())),
            primary_span: span,
            language: None,
            extension: None,
            symbol_kind: None,
            package_name: None,
            package_root: None,
            workspace_root: None,
            evidence: vec![PlannerEvidenceItem {
                evidence_kind,
                route_kind: route,
                label: format!("evidence for {id}"),
                path: path.map(|s| s.to_string()),
                span: None,
                symbol_id: symbol_id.map(|s| SymbolId(s.to_string())),
                impact_entity: None,
                snippet: None,
                score: engine_score,
                notes: Vec::new(),
            }],
            engine_diagnostics: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn fuse(candidates: &[NormalizedPlannerCandidate]) -> Vec<FusedCandidate> {
        ScoreFusion.fuse(
            candidates,
            &[],
            &PlannerRoutePolicyKind::MultiRouteCandidates,
        )
    }

    #[test]
    fn normalization_is_stable_across_engine_types() {
        // Exact: 550 * 100 / 1100 = 50
        assert_eq!(
            normalize_score(&candidate_with(
                "a",
                PlannerRouteKind::Exact,
                Some(550),
                None,
                None,
                None,
                None
            )),
            50
        );
        // Symbol: 1100 * 100 / 1100 = 100
        assert_eq!(
            normalize_score(&candidate_with(
                "b",
                PlannerRouteKind::Symbol,
                Some(1100),
                None,
                None,
                None,
                None
            )),
            100
        );
        // Semantic: 500_000 * 100 / 1_000_000 = 50
        assert_eq!(
            normalize_score(&candidate_with(
                "c",
                PlannerRouteKind::Semantic,
                Some(500_000),
                None,
                None,
                None,
                None
            )),
            50
        );
        // Impact: 500 * 100 / 1000 = 50
        assert_eq!(
            normalize_score(&candidate_with(
                "d",
                PlannerRouteKind::Impact,
                Some(500),
                None,
                None,
                None,
                None
            )),
            50
        );
    }

    #[test]
    fn normalization_clamps_above_max() {
        // Exact max is 1100, score of 2000 should clamp to 100
        assert_eq!(
            normalize_score(&candidate_with(
                "a",
                PlannerRouteKind::Exact,
                Some(2000),
                None,
                None,
                None,
                None
            )),
            100
        );
        // Impact max is 1000, score of 5000 should clamp to 100
        assert_eq!(
            normalize_score(&candidate_with(
                "b",
                PlannerRouteKind::Impact,
                Some(5000),
                None,
                None,
                None,
                None
            )),
            100
        );
    }

    #[test]
    fn normalization_handles_none_score() {
        assert_eq!(
            normalize_score(&candidate_with(
                "a",
                PlannerRouteKind::Symbol,
                None,
                None,
                None,
                None,
                None
            )),
            0
        );
    }

    #[test]
    fn dedup_merges_same_symbol_across_routes() {
        let candidates = vec![
            candidate_with(
                "sym:a",
                PlannerRouteKind::Symbol,
                Some(1000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "sem:a",
                PlannerRouteKind::Semantic,
                Some(800_000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
        ];

        let fused = fuse(&candidates);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].contributing_routes.len(), 2);
        assert!(
            fused[0]
                .contributing_routes
                .contains(&PlannerRouteKind::Symbol)
        );
        assert!(
            fused[0]
                .contributing_routes
                .contains(&PlannerRouteKind::Semantic)
        );
    }

    #[test]
    fn dedup_merges_overlapping_spans() {
        let candidates = vec![
            candidate_with(
                "a",
                PlannerRouteKind::Symbol,
                Some(900),
                Some(1),
                None,
                Some("src/foo.rs"),
                Some(span_at(0, 50)),
            ),
            candidate_with(
                "b",
                PlannerRouteKind::Semantic,
                Some(700_000),
                Some(1),
                None,
                Some("src/foo.rs"),
                Some(span_at(30, 80)),
            ),
        ];

        let fused = fuse(&candidates);
        assert_eq!(fused.len(), 1);
    }

    #[test]
    fn dedup_keeps_separate_non_overlapping_spans() {
        let candidates = vec![
            candidate_with(
                "a",
                PlannerRouteKind::Symbol,
                Some(900),
                Some(1),
                None,
                Some("src/foo.rs"),
                Some(span_at(0, 30)),
            ),
            candidate_with(
                "b",
                PlannerRouteKind::Semantic,
                Some(700_000),
                Some(1),
                None,
                Some("src/foo.rs"),
                Some(span_at(50, 80)),
            ),
        ];

        let fused = fuse(&candidates);
        assert_eq!(fused.len(), 2);
    }

    #[test]
    fn dedup_keeps_separate_different_symbols() {
        let candidates = vec![
            candidate_with(
                "a",
                PlannerRouteKind::Symbol,
                Some(1000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "b",
                PlannerRouteKind::Symbol,
                Some(900),
                Some(2),
                Some("sym.Bar"),
                Some("src/bar.rs"),
                None,
            ),
        ];

        let fused = fuse(&candidates);
        assert_eq!(fused.len(), 2);
    }

    #[test]
    fn fusion_is_deterministic() {
        let candidates = vec![
            candidate_with(
                "a",
                PlannerRouteKind::Symbol,
                Some(1000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "b",
                PlannerRouteKind::Semantic,
                Some(800_000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "c",
                PlannerRouteKind::Symbol,
                Some(900),
                Some(2),
                Some("sym.Bar"),
                Some("src/bar.rs"),
                None,
            ),
        ];

        let first = fuse(&candidates);
        for _ in 0..10 {
            let result = fuse(&candidates);
            assert_eq!(first, result);
        }
    }

    #[test]
    fn tie_breaking_prefers_route_priority() {
        // Same normalized score, exact should come before symbol which comes before semantic
        let candidates = vec![
            candidate_with(
                "sem",
                PlannerRouteKind::Semantic,
                Some(1_000_000),
                Some(1),
                Some("sym.A"),
                Some("a.rs"),
                None,
            ),
            candidate_with(
                "sym",
                PlannerRouteKind::Symbol,
                Some(1100),
                Some(1),
                Some("sym.B"),
                Some("b.rs"),
                None,
            ),
            candidate_with(
                "exact",
                PlannerRouteKind::Exact,
                Some(1100),
                Some(1),
                Some("sym.C"),
                Some("c.rs"),
                None,
            ),
        ];

        let fused = fuse(&candidates);
        assert_eq!(fused.len(), 3);
        // All normalized to 100, so route priority breaks tie: Exact(4) > Symbol(3) > Semantic(2)
        assert!(
            fused[0]
                .contributing_routes
                .contains(&PlannerRouteKind::Exact)
        );
        assert!(
            fused[1]
                .contributing_routes
                .contains(&PlannerRouteKind::Symbol)
        );
        assert!(
            fused[2]
                .contributing_routes
                .contains(&PlannerRouteKind::Semantic)
        );
    }

    #[test]
    fn tie_breaking_uses_rank_then_candidate_id() {
        let candidates = vec![
            candidate_with(
                "b_candidate",
                PlannerRouteKind::Symbol,
                Some(1100),
                Some(3),
                Some("sym.B"),
                Some("b.rs"),
                None,
            ),
            candidate_with(
                "a_candidate",
                PlannerRouteKind::Symbol,
                Some(1100),
                Some(2),
                Some("sym.A"),
                Some("a.rs"),
                None,
            ),
            candidate_with(
                "c_candidate",
                PlannerRouteKind::Symbol,
                Some(1100),
                Some(2),
                Some("sym.C"),
                Some("c.rs"),
                None,
            ),
        ];

        let fused = fuse(&candidates);
        assert_eq!(fused.len(), 3);
        // All score=100, all Symbol priority. Rank 2 < Rank 3, so rank 2 first.
        // Among rank 2: a_candidate < c_candidate lexicographically
        assert_eq!(fused[0].primary.candidate_id, "a_candidate");
        assert_eq!(fused[1].primary.candidate_id, "c_candidate");
        assert_eq!(fused[2].primary.candidate_id, "b_candidate");
    }

    #[test]
    fn agreement_bonus_rewards_multi_route() {
        // Single route: no bonus
        let single = vec![candidate_with(
            "a",
            PlannerRouteKind::Symbol,
            Some(1100),
            Some(1),
            Some("sym.Foo"),
            Some("src/foo.rs"),
            None,
        )];
        let fused_single = fuse(&single);
        assert_eq!(fused_single[0].fused_score, 100); // 100 + 0

        // Two routes: +2 bonus
        let two = vec![
            candidate_with(
                "a",
                PlannerRouteKind::Symbol,
                Some(1100),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "b",
                PlannerRouteKind::Semantic,
                Some(1_000_000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
        ];
        let fused_two = fuse(&two);
        assert_eq!(fused_two[0].fused_score, 102); // 100 + 2

        // Three routes: +4 bonus
        let three = vec![
            candidate_with(
                "a",
                PlannerRouteKind::Symbol,
                Some(1100),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "b",
                PlannerRouteKind::Semantic,
                Some(1_000_000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "c",
                PlannerRouteKind::Exact,
                Some(1100),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
        ];
        let fused_three = fuse(&three);
        assert_eq!(fused_three[0].fused_score, 104); // 100 + 4

        // Four routes: capped at +6
        let four = vec![
            candidate_with(
                "a",
                PlannerRouteKind::Symbol,
                Some(1100),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "b",
                PlannerRouteKind::Semantic,
                Some(1_000_000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "c",
                PlannerRouteKind::Exact,
                Some(1100),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "d",
                PlannerRouteKind::Impact,
                Some(1000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
        ];
        let fused_four = fuse(&four);
        assert_eq!(fused_four[0].fused_score, 106); // 100 + 6
    }

    #[test]
    fn provenance_preserved_after_merge() {
        let candidates = vec![
            candidate_with(
                "a",
                PlannerRouteKind::Symbol,
                Some(1000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
            candidate_with(
                "b",
                PlannerRouteKind::Semantic,
                Some(800_000),
                Some(1),
                Some("sym.Foo"),
                Some("src/foo.rs"),
                None,
            ),
        ];

        let fused = fuse(&candidates);
        assert_eq!(fused.len(), 1);

        let evidence_routes: Vec<PlannerRouteKind> = fused[0]
            .merged_evidence
            .iter()
            .map(|e| e.route_kind.clone())
            .collect();
        assert!(evidence_routes.contains(&PlannerRouteKind::Symbol));
        assert!(evidence_routes.contains(&PlannerRouteKind::Semantic));
        assert_eq!(fused[0].merged_evidence.len(), 2);
    }
}
