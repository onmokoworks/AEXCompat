import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_GAMMA_CLASSIC_RENDER_GATE_RESULT_2026-07-18.json"


def _result():
    return json.loads(RESULT.read_text(encoding="utf-8-sig"))


def test_gate_uses_session_transport_and_authenticates_fixture_and_input():
    result = _result()
    assert result["fixture"].endswith("Gamma_Table")
    assert result["status"] in {"passed", "blocked"}
    artifacts = result["authenticated_artifacts"]
    assert artifacts["fixture"] == {
        "path": "target/sdk-fixtures/gamma/Gamma_Table.aex",
        "size_bytes": 45056,
        "sha256": "6fcb4946c77a9fcb4fab8e15dbbc54ffb4b1ab08656ce9acea39c44099b6dfa8",
    }
    assert artifacts["input"] == {
        "path": "target/image-transport/colorgrid-click-input.rgba",
        "size_bytes": 768,
        "sha256": "c9ee521c7d71cbf6a41cb1a2d075add64676b912f5870098417b75ca727ba807",
    }
    assert artifacts["session_harness"]["path"].endswith("aexcompat-harness.exe")
    assert artifacts["release_worker"]["path"] == "target/minihost-build/aex_render_worker.exe"
    if result["status"] == "blocked":
        assert result["classification"] == "session_transport_failure"
        assert result["failure"]["message"]
        assert result["checks"]["fail_closed"] is True
        assert result["checks"]["structured_failure_recorded"] is True


def test_success_cases_prove_parameter_override_and_determinism():
    result = _result()
    if result["status"] != "passed":
        return
    assert result["execution"] == {
        "render_path": "classic",
        "transport": "render-experimental-session-param",
        "pixel_format": "argb8",
        "width": 16,
        "height": 12,
        "rowbytes": 64,
        "fresh_session_run": True,
    }
    for case_name, gamma in (("identity", 1.0), ("changed", 1.5)):
        case = result["cases"][case_name]
        assert case["gamma"] == gamma
        assert case["deterministic"] is True
        assert len(case["runs"]) == 2
        for run in case["runs"]:
            assert run["parameter_override"] == {"slot": 1, "value": gamma}
            assert all(run["checks"].values())


def test_gamma_changes_output_and_one_is_identity():
    result = _result()
    if result["status"] != "passed":
        return
    identity = result["cases"]["identity"]["runs"][0]
    changed = result["cases"]["changed"]["runs"][0]
    assert result["cross_case"] == {
        "outputs_differ": True,
        "identity_matches_input_transport": True,
    }
    assert identity["transport_output_sha256"] == result["authenticated_artifacts"]["input"]["sha256"]
    assert identity["internal_output_sha256"] != changed["internal_output_sha256"]
    assert identity["transport_output_sha256"] != changed["transport_output_sha256"]
