import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_GAMMA_CLASSIC_RENDER_GATE_RESULT_2026-07-18.json"
RUNNER = ROOT / "tools" / "run-gamma-classic-render-gate.ps1"


def _result():
    return json.loads(RESULT.read_text(encoding="utf-8-sig"))


def test_gate_authenticates_fixed_fixture_worker_and_input():
    result = _result()
    assert result["status"] == "passed"
    assert result["fixture"].endswith("Gamma_Table")
    artifacts = result["authenticated_artifacts"]
    assert artifacts["fixture"] == {
        "path": "target/sdk-fixtures/gamma/Gamma_Table.aex",
        "size_bytes": 45056,
        "sha256": "6fcb4946c77a9fcb4fab8e15dbbc54ffb4b1ab08656ce9acea39c44099b6dfa8",
    }
    assert artifacts["worker"]["path"] == "target/minihost-build/aex_render_worker.exe"
    assert artifacts["worker"]["size_bytes"] == 788480
    assert artifacts["worker"]["sha256"] == "5f97c3d41a7d9149feab14ac5f892f3bcb88a2d3d38dd8694f9b84f0ddefab47"
    assert artifacts["input"]["sha256"] == "c9ee521c7d71cbf6a41cb1a2d075add64676b912f5870098417b75ca727ba807"


def test_identity_and_changed_cases_each_have_two_audited_deterministic_runs():
    result = _result()
    assert result["execution"]["worker_processes"] == 4
    assert result["execution"]["fresh_authenticated_stage_per_run"] is True
    assert result["cases"]["identity"]["gamma"] == 1.0
    assert result["cases"]["changed"]["gamma"] == 1.5
    for case in result["cases"].values():
        assert case["deterministic"] is True
        assert len(case["runs"]) == 2
        assert case["runs"][0]["internal_output_sha256"] == case["runs"][1]["internal_output_sha256"]
        for run in case["runs"]:
            assert all(value == 0 for value in run["selector_errors"].values())
            assert run["module_audit"]["status"] == "passed"
            assert run["module_audit"]["unknown_count"] == 0
            assert run["module_audit"]["phase_count"] >= 3
            assert all(run["checks"].values())


def test_gamma_changes_output_and_one_is_identity():
    result = _result()
    identity = result["cases"]["identity"]["runs"][0]
    changed = result["cases"]["changed"]["runs"][0]
    assert result["cross_case"] == {
        "outputs_differ": True,
        "identity_matches_input_transport": True,
    }
    assert identity["transport_output_sha256"] == result["authenticated_artifacts"]["input"]["sha256"]
    assert identity["internal_output_sha256"] != changed["internal_output_sha256"]
    assert identity["transport_output_sha256"] != changed["transport_output_sha256"]


def test_runner_is_fail_closed_and_evidence_has_no_local_paths_or_stderr():
    source = RUNNER.read_text(encoding="utf-8")
    for marker in (
        "Resolve-AuthenticatedArtifact",
        "Get-FileHash",
        "aexcompat-trusted-worker-gamma-",
        "aexcompat-sealed-gamma-",
        "Start-Process",
        "-WindowStyle Hidden",
        "module_audit_passed",
        "selectors_success",
        "lifetimes_balanced",
        "guards_intact",
    ):
        assert marker in source
    serialized = RESULT.read_text(encoding="utf-8-sig")
    assert not re.search(r"[A-Za-z]:\\", serialized)
    assert "stderr" not in serialized.lower()
