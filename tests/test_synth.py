"""Tests for the deterministic synthetic corpus generator."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from hyperbench.query_packs import load_query_artifacts, validate_query_artifacts
from hyperbench.schemas import QueryPack, SyntheticCorpusConfig
from hyperbench.synth import SyntheticGenerationError, generate_synthetic_corpus_bundle


def test_generate_synth_creates_expected_bundle_shape(tmp_path: Path) -> None:
    config = SyntheticCorpusConfig(
        config_id="synthetic-test-small",
        corpus_id="synthetic-test-small",
        repo_tier="S",
        seed=11,
        package_count=6,
        file_count=36,
        dependency_fanout=2,
        route_count=2,
        handler_count=2,
        config_file_count=2,
        test_file_count=2,
        auth_flow_count=2,
        edit_scenario_count=3,
        query_seed_count=5,
        required_package_roles=["auth", "session", "api", "worker", "tests", "web"],
        workspace_package_prefix="@hyperindex",
    )

    result = generate_synthetic_corpus_bundle(config, tmp_path / "bundle")

    assert result.manifest_path.exists()
    assert result.ground_truth_path.exists()
    assert result.query_pack_path.exists()
    assert result.golden_set_path.exists()
    assert result.query_pack_dir.exists()
    assert result.golden_set_dir.exists()
    assert result.edit_scenarios_path.exists()
    assert (result.repo_dir / "packages" / "auth" / "src" / "session" / "service.ts").exists()
    assert (result.repo_dir / "packages" / "api" / "src" / "routes" / "logout.ts").exists()
    assert (result.repo_dir / "packages" / "worker" / "src" / "jobs" / "password-reset.ts").exists()

    ground_truth = json.loads(result.ground_truth_path.read_text(encoding="utf-8"))
    assert ground_truth["hero_query"]["query"] == "where do we invalidate sessions?"
    assert "packages/auth/src/session/service.ts" in ground_truth["hero_query"]["canonical_path"]

    query_pack = QueryPack.from_path(result.query_pack_path)
    assert any(query.type == "semantic" for query in query_pack.queries)
    assert any(
        getattr(query, "text", "") == "where do we invalidate sessions?"
        for query in query_pack.queries
    )
    assert len(list(result.query_pack_dir.glob("*.json"))) == 4
    assert len(list(result.golden_set_dir.glob("*.json"))) == 4

    query_packs, golden_sets = load_query_artifacts(result.query_pack_dir, result.golden_set_dir)
    summary = validate_query_artifacts(query_packs, golden_sets)
    assert summary.query_pack_count == 4
    assert summary.golden_set_count == 4

    edit_scenarios = json.loads(result.edit_scenarios_path.read_text(encoding="utf-8"))
    assert len(edit_scenarios["scenarios"]) == 3
    assert edit_scenarios["scenarios"][0]["scenario_id"] == "hero-modify-invalidate-session"


def test_generate_synth_is_byte_for_byte_deterministic_for_same_seed(tmp_path: Path) -> None:
    config = SyntheticCorpusConfig(
        config_id="synthetic-test-medium",
        corpus_id="synthetic-test-medium",
        repo_tier="M",
        seed=21,
        package_count=7,
        file_count=42,
        dependency_fanout=2,
        route_count=3,
        handler_count=3,
        config_file_count=3,
        test_file_count=3,
        auth_flow_count=3,
        edit_scenario_count=4,
        query_seed_count=6,
        required_package_roles=["auth", "session", "api", "worker", "tests", "web"],
        workspace_package_prefix="@hyperindex",
    )

    output_a = tmp_path / "bundle-a"
    output_b = tmp_path / "bundle-b"
    generate_synthetic_corpus_bundle(config, output_a)
    generate_synthetic_corpus_bundle(config, output_b)

    assert _snapshot_files(output_a) == _snapshot_files(output_b)


def test_generate_synth_cli_command_exits_cleanly(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    from hyperbench.cli import main

    output_dir = tmp_path / "cli-bundle"
    exit_code = main(
        [
            "corpora",
            "generate-synth",
            "--config-path",
            "bench/configs/synthetic-corpus.yaml",
            "--output-dir",
            str(output_dir),
        ]
    )
    captured = capsys.readouterr()

    assert exit_code == 0
    assert "Synthetic corpus generated:" in captured.out
    assert (output_dir / "ground_truth.json").exists()
    assert (output_dir / "golden-set.json").exists()


def test_generate_synth_fails_when_file_count_is_too_small(tmp_path: Path) -> None:
    config = SyntheticCorpusConfig(
        config_id="synthetic-too-small",
        corpus_id="synthetic-too-small",
        repo_tier="S",
        seed=3,
        package_count=6,
        file_count=10,
        dependency_fanout=1,
        route_count=1,
        handler_count=1,
        config_file_count=1,
        test_file_count=1,
        auth_flow_count=1,
        required_package_roles=["auth", "session", "api", "worker", "tests", "web"],
        workspace_package_prefix="@hyperindex",
    )

    with pytest.raises(SyntheticGenerationError) as exc_info:
        generate_synthetic_corpus_bundle(config, tmp_path / "bundle")

    assert "file_count=10 is too small" in str(exc_info.value)


def _snapshot_files(root: Path) -> dict[str, bytes]:
    return {
        str(path.relative_to(root)): path.read_bytes()
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }
