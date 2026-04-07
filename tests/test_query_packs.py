"""Validation tests for Phase 1 query packs and expectation sets."""

from pathlib import Path

import pytest
from hyperbench.query_packs import (
    SYNTHETIC_TARGET_COUNTS,
    load_query_artifacts,
    validate_query_artifacts,
)
from hyperbench.schemas import (
    ExpectedHit,
    GoldenExpectation,
    GoldenReason,
    GoldenSet,
    QueryPack,
    QueryType,
    SemanticQuery,
)

REPO_ROOT = Path(__file__).resolve().parents[1]
CONFIG_DIR = REPO_ROOT / "bench" / "configs"


def test_checked_in_query_artifacts_validate_and_meet_phase1_counts() -> None:
    query_packs, golden_sets = load_query_artifacts(
        CONFIG_DIR / "query-packs",
        CONFIG_DIR / "goldens",
    )

    summary = validate_query_artifacts(
        query_packs,
        golden_sets,
        synthetic_corpus_ids={"synthetic-saas-medium"},
        required_synthetic_counts=SYNTHETIC_TARGET_COUNTS,
    )

    assert summary.query_counts[QueryType.EXACT] >= 100
    assert summary.query_counts[QueryType.SYMBOL] >= 50
    assert summary.query_counts[QueryType.SEMANTIC] >= 30
    assert summary.query_counts[QueryType.IMPACT] >= 30


def test_query_artifact_validation_fails_when_expectation_is_missing() -> None:
    broken_pack = QueryPack(
        query_pack_id="broken-pack",
        corpus_id="synthetic-broken",
        queries=[
            SemanticQuery(
                query_id="broken-semantic-query",
                title="Broken semantic query",
                text="where is the broken expectation?",
                tags=["synthetic", "semantic"],
                limit=10,
            )
        ],
    )
    broken_golden = GoldenSet(
        golden_set_id="broken-goldens",
        corpus_id="synthetic-broken",
        query_pack_id="broken-pack",
        expectations=[
            GoldenExpectation(
                query_id="different-query",
                expected_hits=[
                    ExpectedHit(
                        path="packages/auth/src/session/service.ts",
                        reason=GoldenReason.SEMANTIC,
                        rank_max=3,
                    )
                ],
            )
        ],
    )

    with pytest.raises(ValueError) as exc_info:
        validate_query_artifacts(
            [broken_pack],
            [broken_golden],
            synthetic_corpus_ids={"synthetic-broken"},
            required_synthetic_counts={QueryType.SEMANTIC: 1},
        )

    assert "missing expectations for: broken-semantic-query" in str(exc_info.value)
