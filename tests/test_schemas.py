"""Schema validation tests for Phase 1 Hyperbench contracts."""

from pathlib import Path

import pytest
from hyperbench.schemas import (
    BenchmarkHardwareCatalog,
    CompareBudget,
    CompareOutput,
    CorpusManifest,
    GoldenSet,
    MetricsDocument,
    QueryPack,
    RealRepoCatalog,
    RepoTierCatalog,
    RunMetadata,
    SyntheticCorpusConfig,
)
from pydantic import ValidationError

REPO_ROOT = Path(__file__).resolve().parents[1]
CONFIG_DIR = REPO_ROOT / "bench" / "configs"


@pytest.mark.parametrize(
    ("model_type", "file_name"),
    [
        (RepoTierCatalog, "repo-tiers.yaml"),
        (BenchmarkHardwareCatalog, "hardware-targets.yaml"),
        (SyntheticCorpusConfig, "synthetic-corpus.yaml"),
        (CorpusManifest, "corpus-manifest.synthetic.yaml"),
        (RealRepoCatalog, "repos.yaml"),
        (CompareBudget, "budgets.yaml"),
        (QueryPack, "query-pack.yaml"),
        (GoldenSet, "golden-set.yaml"),
        (RunMetadata, "run-metadata.json"),
        (MetricsDocument, "metrics-document.json"),
        (CompareBudget, "compare-budget.yaml"),
        (CompareOutput, "compare-output.json"),
    ],
)
def test_example_configs_validate(
    model_type: type[object],
    file_name: str,
) -> None:
    config_path = CONFIG_DIR / file_name
    document = model_type.from_path(config_path)  # type: ignore[attr-defined]
    assert document is not None


def test_query_pack_roundtrips_between_yaml_and_json() -> None:
    query_pack = QueryPack.from_path(CONFIG_DIR / "query-pack.yaml")

    as_json = query_pack.to_json_text()
    from_json = QueryPack.from_json_text(as_json)
    assert from_json == query_pack

    as_yaml = query_pack.to_yaml_text()
    from_yaml = QueryPack.from_yaml_text(as_yaml)
    assert from_yaml == query_pack


@pytest.mark.parametrize(
    "config_path",
    sorted((CONFIG_DIR / "query-packs").glob("*")),
)
def test_query_pack_directory_examples_validate(config_path: Path) -> None:
    query_pack = QueryPack.from_path(config_path)
    assert query_pack is not None


@pytest.mark.parametrize(
    "config_path",
    sorted((CONFIG_DIR / "goldens").glob("*")),
)
def test_golden_directory_examples_validate(config_path: Path) -> None:
    golden_set = GoldenSet.from_path(config_path)
    assert golden_set is not None


def test_run_metadata_roundtrips_between_json_and_yaml() -> None:
    metadata = RunMetadata.from_path(CONFIG_DIR / "run-metadata.json")

    as_yaml = metadata.to_yaml_text()
    from_yaml = RunMetadata.from_yaml_text(as_yaml)
    assert from_yaml == metadata

    as_json = metadata.to_json_text()
    from_json = RunMetadata.from_json_text(as_json)
    assert from_json == metadata


def test_invalid_repo_tier_range_fails_with_helpful_error() -> None:
    invalid_yaml = """
schema_version: "1"
tiers:
  - tier: S
    loc_range:
      minimum: 10
      maximum: 5
    package_range:
      minimum: 1
      maximum: 3
"""
    with pytest.raises(ValidationError) as exc_info:
        RepoTierCatalog.from_yaml_text(invalid_yaml)

    assert "maximum must be greater than or equal to minimum" in str(exc_info.value)


def test_invalid_synthetic_config_requires_core_roles() -> None:
    invalid_yaml = """
schema_version: "1"
config_id: missing-session
corpus_id: synthetic-saas-medium
repo_tier: M
seed: 1
scenario: saas_monorepo
package_manager: pnpm
package_count: 5
required_package_roles:
  - auth
  - api
  - worker
  - tests
workspace_package_prefix: "@hyperindex"
"""
    with pytest.raises(ValidationError) as exc_info:
        SyntheticCorpusConfig.from_yaml_text(invalid_yaml)

    assert "required_package_roles must include the core scenario roles" in str(exc_info.value)
    assert "session" in str(exc_info.value)


def test_invalid_query_pack_rejects_unknown_query_type() -> None:
    invalid_yaml = """
schema_version: "1"
query_pack_id: bad-pack
corpus_id: synthetic-saas-medium
queries:
  - query_id: bad-query
    type: unknown
    title: Bad query
    text: hi
"""
    with pytest.raises(ValidationError) as exc_info:
        QueryPack.from_yaml_text(invalid_yaml)

    errors = exc_info.value.errors()
    assert errors[0]["loc"] == ("queries", 0)
    assert "Input tag 'unknown'" in errors[0]["msg"]


def test_invalid_metrics_summary_reports_out_of_range_mean() -> None:
    invalid_json = """
{
  "schema_version": "1",
  "run_id": "phase1-smoke-001",
  "summaries": [
    {
      "metric_name": "exact-p95",
      "metric_kind": "latency",
      "unit": "ms",
      "sample_count": 3,
      "minimum": 10.0,
      "maximum": 20.0,
      "mean": 25.0
    }
  ]
}
"""
    with pytest.raises(ValidationError) as exc_info:
        MetricsDocument.from_json_text(invalid_json)

    assert "mean must be between minimum and maximum" in str(exc_info.value)


def test_invalid_compare_budget_requires_limit() -> None:
    invalid_yaml = """
schema_version: "1"
budget_id: empty-threshold
thresholds:
  - metric_name: exact-p95
    unit: ms
"""
    with pytest.raises(ValidationError) as exc_info:
        CompareBudget.from_yaml_text(invalid_yaml)

    assert "budget thresholds must define at least one limit" in str(exc_info.value)


def test_invalid_repo_catalog_requires_small_medium_and_large() -> None:
    invalid_yaml = """
schema_version: "1"
generated_for_phase: phase1
selection_note: Missing a large-tier repo on purpose
repos:
  - repo_id: repo-small
    status: selected
    owner: example
    name: small
    repo_url: https://github.com/example/small
    clone_url: https://github.com/example/small.git
    rationale: Small repo
    expected_tier: S
    tier_verification_status: partial
    why_useful: Useful small repo
    license:
      spdx_id: MIT
      verification_status: verified
      source_url: https://github.com/example/small
    clone_strategy: shallow
    pinning_policy: pin to sha later
    source_urls:
      - https://github.com/example/small
    risks:
      - Tier is not measured yet.
    manual_verification:
      - Measure LOC.
  - repo_id: repo-medium
    status: selected
    owner: example
    name: medium
    repo_url: https://github.com/example/medium
    clone_url: https://github.com/example/medium.git
    rationale: Medium repo
    expected_tier: M
    tier_verification_status: partial
    why_useful: Useful medium repo
    license:
      spdx_id: MIT
      verification_status: verified
      source_url: https://github.com/example/medium
    clone_strategy: filter_blobless
    pinning_policy: pin to sha later
    source_urls:
      - https://github.com/example/medium
    risks:
      - Tier is not measured yet.
    manual_verification:
      - Measure LOC.
  - repo_id: repo-medium-2
    status: selected
    owner: example
    name: medium-two
    repo_url: https://github.com/example/medium-two
    clone_url: https://github.com/example/medium-two.git
    rationale: Another medium repo
    expected_tier: M
    tier_verification_status: partial
    why_useful: Useful repo
    license:
      spdx_id: MIT
      verification_status: verified
      source_url: https://github.com/example/medium-two
    clone_strategy: filter_blobless
    pinning_policy: pin to sha later
    source_urls:
      - https://github.com/example/medium-two
    risks:
      - Tier is not measured yet.
    manual_verification:
      - Measure LOC.
"""
    with pytest.raises(ValidationError) as exc_info:
        RealRepoCatalog.from_yaml_text(invalid_yaml)

    assert "repos must include at least one entry for each required tier" in str(exc_info.value)
