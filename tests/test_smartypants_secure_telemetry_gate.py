import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "SDK_SMARTYPANTS_SECURE_TELEMETRY_RESULT_2026-07-18.json"
RUNNER = ROOT / "tools" / "run-smartypants-secure-telemetry-gate.ps1"


def test_secure_smartypants_evidence_has_exact_runtime_telemetry():
    result = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    pre = result["smart_pre_render"]
    assert result["result"] == "passed"
    assert (pre["comp_suite_version"], pre["comp_suite_slot"]) == (21, 4)
    assert (pre["comp_bg_color_success_count"], pre["comp_bg_color_rejection_count"]) == (1, 0)
    assert (pre["guid_mix_in_call_count"], pre["guid_mix_in_success_count"], pre["guid_mix_in_rejection_count"]) == (1, 1, 0)
    assert pre["guid_mix_in_size"] == 32 <= pre["guid_mix_in_size_limit"] == 1048576
    assert pre["guid_mix_in_result"] == pre["error"] == 0


def test_secure_runtime_balances_resources_and_enforces_module_audit():
    result = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    render = result["render"]
    assert result["module_audit"] == {
        "required": True,
        "broker_validation": "passed",
        "basis": "secure_image_dispatch requires and validates the worker module audit before returning an ok classification",
    }
    assert render["worker_classification"] == "ok" and render["smart_render_error"] == 0
    assert render["suite_acquires"] == render["suite_releases"]
    assert render["suite_leases_balanced"] and render["handle_lifetimes_balanced"]
    assert render["world_lifetimes_balanced"] and render["output_pixels_valid"] and render["passed"]


def test_artifact_identities_are_path_free_and_event_is_not_misattributed():
    result = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    expected = {
        "release_harness": (9944064, "4a15e80c77289b10e40449823833f2ad56627bf1f7e96ba43b25d924f9e68cd4"),
        "canonical_smart_worker": (799232, "2ae221f0a58009b219b9d9aa5347b8be8225f85044a843a842a2ab1663b04754"),
        "sdk_smartypants_fixture": (31232, "47fe55f77600f041a3297b4558108fad5d6e886b6cc66eed2e51f88dac922ede"),
        "deterministic_input": (3100, "8e2b249fd979826a60ad089b783e84e8f65d9b692c157be065953775a8aa6c91"),
    }
    for role, (size, sha) in expected.items():
        assert result["authenticated_artifacts"][role] == {"size_bytes": size, "sha256": sha}
    assert ":\\" not in EVIDENCE.read_text(encoding="utf-8")
    assert result["event_boundary"]["dispatched"] is False
    assert result["event_boundary"]["attributed_to_event"] is False
    assert result["event_boundary"]["telemetry_origin"] == "PF_Cmd_SMART_PRE_RENDER only"


def test_runner_uses_only_release_harness_and_fixed_identity_checks():
    text = RUNNER.read_text(encoding="utf-8")
    assert "broker\\target\\release\\aexcompat-harness.exe" in text
    assert "Assert-Identity harness" in text and "Assert-Identity worker" in text
    assert "--render-experimental-smart" in text
    assert "aex_smart_worker.exe'" in text
    assert "& $worker" not in text
