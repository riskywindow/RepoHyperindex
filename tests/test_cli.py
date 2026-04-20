"""CLI tests for the Phase 1 Hyperbench harness."""

import pytest
from hyperbench.cli import build_parser, main


def test_build_parser_uses_expected_program_name() -> None:
    parser = build_parser()
    assert parser.prog == "hyperbench"


def test_main_help_exits_cleanly(capsys: pytest.CaptureFixture[str]) -> None:
    exit_code = main([])
    captured = capsys.readouterr()
    assert exit_code == 0
    assert "hyperbench" in captured.out


def test_status_command_exits_cleanly(capsys: pytest.CaptureFixture[str]) -> None:
    exit_code = main(["status"])
    captured = capsys.readouterr()
    assert exit_code == 0
    assert "Phase 1 harness is installed and ready" in captured.out


def test_corpora_validate_command_exits_cleanly(capsys: pytest.CaptureFixture[str]) -> None:
    exit_code = main(["corpora", "validate"])
    captured = capsys.readouterr()
    assert exit_code == 0
    assert "Validation succeeded." in captured.out


def test_corpora_bootstrap_dry_run_exits_cleanly(capsys: pytest.CaptureFixture[str]) -> None:
    exit_code = main(["corpora", "bootstrap", "--dry-run"])
    captured = capsys.readouterr()
    assert exit_code == 0
    assert "Bootstrap dry-run plan:" in captured.out
    assert "missing pinned_ref" in captured.out


def test_corpora_generate_synth_help_exits_cleanly(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc_info:
        main(["corpora", "generate-synth", "--help"])
    captured = capsys.readouterr()
    assert exc_info.value.code == 0
    assert "hyperbench corpora generate-synth" in captured.out
    assert "--config-path" in captured.out


def test_run_help_exits_cleanly(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc_info:
        main(["run", "--help"])
    captured = capsys.readouterr()
    assert exc_info.value.code == 0
    assert "hyperbench run" in captured.out
    assert "--adapter" in captured.out
    assert "daemon-semantic" in captured.out
    assert "--daemon-build-temperature" in captured.out


def test_report_help_exits_cleanly(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc_info:
        main(["report", "--help"])
    captured = capsys.readouterr()
    assert exc_info.value.code == 0
    assert "hyperbench report" in captured.out
    assert "--run-dir" in captured.out


def test_compare_help_exits_cleanly(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc_info:
        main(["compare", "--help"])
    captured = capsys.readouterr()
    assert exc_info.value.code == 0
    assert "hyperbench compare" in captured.out
    assert "--baseline-run-dir" in captured.out
