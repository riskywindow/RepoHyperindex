use std::collections::BTreeMap;

use crate::common::{ImpactComponentStatus, implemented_status};
use crate::impact_enrichment::ImpactEnrichmentPlan;
use crate::impact_model::{ImpactModelSeed, ResolvedImpactTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRankingCandidate {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Default, Clone)]
pub struct TestRankingPolicy;

impl TestRankingPolicy {
    pub fn rank_candidates(
        &self,
        seed: &ImpactModelSeed,
        enrichment: &ImpactEnrichmentPlan,
    ) -> Vec<TestRankingCandidate> {
        let mut candidates = BTreeMap::<String, TestRankingCandidate>::new();

        let mut push_candidates = |paths: Vec<TestRankingCandidate>| {
            for candidate in paths {
                candidates
                    .entry(candidate.path.clone())
                    .or_insert(candidate);
            }
        };

        match &seed.resolved_target {
            ResolvedImpactTarget::Symbol(symbol) => {
                push_candidates(
                    enrichment
                        .tests_by_symbol
                        .get(&symbol.canonical_symbol_id)
                        .into_iter()
                        .flatten()
                        .map(|association| TestRankingCandidate {
                            path: association.test_path.clone(),
                            reason: association.detail.clone(),
                        })
                        .collect(),
                );
                push_candidates(
                    enrichment
                        .tests_by_file
                        .get(&symbol.path)
                        .into_iter()
                        .flatten()
                        .map(|association| TestRankingCandidate {
                            path: association.test_path.clone(),
                            reason: association.detail.clone(),
                        })
                        .collect(),
                );
            }
            ResolvedImpactTarget::File(file) => {
                push_candidates(
                    enrichment
                        .tests_by_file
                        .get(&file.path)
                        .into_iter()
                        .flatten()
                        .map(|association| TestRankingCandidate {
                            path: association.test_path.clone(),
                            reason: association.detail.clone(),
                        })
                        .collect(),
                );
            }
        }

        candidates.into_values().collect()
    }

    pub fn status(&self) -> ImpactComponentStatus {
        implemented_status(
            "test_ranking",
            "test affinity ranking seeds are derived deterministically from explicit symbol/file associations",
        )
    }
}
