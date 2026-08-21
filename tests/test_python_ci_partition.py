import importlib.util
from pathlib import Path
from types import SimpleNamespace

import pytest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "run-python-ci-tests.py"
SPEC = importlib.util.spec_from_file_location("run_python_ci_tests", SCRIPT)
assert SPEC and SPEC.loader
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


def test_trusted_python_ci_partitions_classic_evidence_exactly_once():
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
    assert "--run-sdk-tests" in main
    assert "--run-built-artifact-tests" in main
    assert "--validate-local-artifact-manifest" in main


def test_fork_python_ci_keeps_evidence_in_main_policy_run():
    main = RUNNER.pytest_arguments("main", sdk_ready=False)

    assert RUNNER.CLASSIC_FAILURE_EVIDENCE_NODE not in main
    assert RUNNER.SDK_BACKWARDS_BUILD_NODE not in main
    assert "--deselect" not in main
    assert "--run-sdk-tests" not in main
    assert "--run-built-artifact-tests" not in main
    assert "--validate-local-artifact-manifest" in main
    with pytest.raises(ValueError, match="requires trusted built artifacts"):
        RUNNER.pytest_arguments("classic-evidence", sdk_ready=False)


@pytest.mark.parametrize("partition", ("main", "classic-evidence"))
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
