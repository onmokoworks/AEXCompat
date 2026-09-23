import importlib.util
import os
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace

import pytest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "run-python-ci-tests.py"
SPEC = importlib.util.spec_from_file_location("run_python_ci_tests", SCRIPT)
assert SPEC and SPEC.loader
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


def test_trusted_python_ci_partitions_classic_evidence_exactly_once():
    early = RUNNER.pytest_arguments("early", sdk_ready=False)
    main = RUNNER.pytest_arguments("main", sdk_ready=True)
    evidence = RUNNER.pytest_arguments("classic-evidence", sdk_ready=True)

    assert main.count(RUNNER.CLASSIC_FAILURE_EVIDENCE_NODE) == 1
    node_index = main.index(RUNNER.CLASSIC_FAILURE_EVIDENCE_NODE)
    assert main[node_index - 1] == "--deselect"
    assert evidence == [
        "-q",
        "-rs",
        RUNNER.CLASSIC_FAILURE_EVIDENCE_NODE,
        "--run-built-artifact-tests",
    ]
    assert main.count(RUNNER.SDK_BACKWARDS_BUILD_NODE) == 1
    sdk_index = main.index(RUNNER.SDK_BACKWARDS_BUILD_NODE)
    assert main[sdk_index - 1] == "--deselect"
    assert main.count(RUNNER.SDK_GRABBA_BUILD_NODE) == 1
    grabba_index = main.index(RUNNER.SDK_GRABBA_BUILD_NODE)
    assert main[grabba_index - 1] == "--deselect"
    assert main.count(RUNNER.AEGP_RENDER_OPTIONS_LIFECYCLE_NODE) == 1
    render_options_index = main.index(RUNNER.AEGP_RENDER_OPTIONS_LIFECYCLE_NODE)
    assert main[render_options_index - 1] == "--deselect"
    assert main.count(RUNNER.PF_ADV_TIME_RELEASE_BUILD_NODE) == 1
    adv_time_index = main.index(RUNNER.PF_ADV_TIME_RELEASE_BUILD_NODE)
    assert main[adv_time_index - 1] == "--deselect"
    assert "--run-sdk-tests" in main
    assert "--run-built-artifact-tests" in main
    assert "--validate-local-artifact-manifest" in main
    assert "--run-sdk-tests" not in early
    assert "--run-built-artifact-tests" not in early
    assert "--validate-local-artifact-manifest" in early


def test_fork_python_ci_keeps_evidence_in_main_policy_run():
    main = RUNNER.pytest_arguments("main", sdk_ready=False)

    assert RUNNER.CLASSIC_FAILURE_EVIDENCE_NODE not in main
    assert RUNNER.SDK_BACKWARDS_BUILD_NODE not in main
    assert RUNNER.SDK_GRABBA_BUILD_NODE not in main
    assert RUNNER.AEGP_RENDER_OPTIONS_LIFECYCLE_NODE not in main
    assert RUNNER.PF_ADV_TIME_RELEASE_BUILD_NODE not in main
    assert "--deselect" not in main
    assert "--run-sdk-tests" not in main
    assert "--run-built-artifact-tests" not in main
    assert "--validate-local-artifact-manifest" in main
    with pytest.raises(ValueError, match="requires trusted built artifacts"):
        RUNNER.pytest_arguments("classic-evidence", sdk_ready=False)


def _collect_nodes(arguments):
    # Collection is a real pytest invocation: the conftest manifests add marks
    # dynamically, so comparing command strings alone cannot prove coverage.
    arguments = list(arguments)
    for flag in ("-n", "--dist"):
        index = arguments.index(flag)
        del arguments[index:index + 2]
    arguments = [value for value in arguments if not value.startswith("--durations=")]
    result = subprocess.run(
        [sys.executable, "-m", "pytest", "--collect-only", *arguments],
        cwd=ROOT,
        env={**os.environ, "PYTHONUTF8": "1"},
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    return {
        line for line in result.stdout.splitlines()
        if line.startswith("tests/") and "::" in line
    }


def test_early_and_main_collect_every_previous_ci_node_exactly_once():
    early = RUNNER.pytest_arguments("early", sdk_ready=False)
    main = RUNNER.pytest_arguments("main", sdk_ready=True)
    unsplit = list(main)
    marker_index = unsplit.index("-m")
    del unsplit[marker_index:marker_index + 2]

    before = _collect_nodes(unsplit)
    early_nodes = _collect_nodes(early)
    main_nodes = _collect_nodes(main)

    assert before
    assert early_nodes
    assert main_nodes
    assert early_nodes.isdisjoint(main_nodes)
    assert early_nodes | main_nodes == before


@pytest.mark.parametrize("partition", ("early", "main", "classic-evidence"))
def test_runner_preserves_pytest_output_and_failure(partition, tmp_path, capsys):
    calls = []

    def fake_run(command, **kwargs):
        calls.append((command, kwargs))
        return SimpleNamespace(
            stdout="pytest stdout\n", stderr="pytest stderr\n", returncode=7
        )

    output = tmp_path / f"{partition}.txt"
    result = RUNNER.run_partition(
        partition,
        sdk_ready=True,
        output_path=output,
        runner=fake_run,
    )

    assert result == 7
    assert calls == [
        (
            [
                RUNNER.sys.executable,
                "-m",
                "pytest",
                *RUNNER.pytest_arguments(partition, sdk_ready=True),
            ],
            {"text": True, "capture_output": True},
        )
    ]
    expected = "pytest stdout\npytest stderr\n"
    assert output.read_text(encoding="utf-8") == expected
    assert capsys.readouterr().out == expected
