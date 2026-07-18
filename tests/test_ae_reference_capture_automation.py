import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "AE_REFERENCE_CAPTURE_AUTOMATION_RESULT_2026-07-15.json"
RUNNER = ROOT / "tools" / "capture-ae-reference.ps1"
SCRIPT = ROOT / "tools" / "ae-reference-capture.jsx"


def test_reference_capture_is_fail_closed_while_user_ae_session_is_running():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    assert result["status"] == "ready_waiting_for_no_running_ae_session"
    assert result["installed_hash_audit"]["exact_installed_matches"] == 4
    assert all(result["safety_gates"].values())
    refusal = result["refusal_observation"]
    assert refusal["capture_refused"] is True
    assert refusal["output_created"] is False
    assert refusal["result_created"] is False
    assert refusal["existing_ae_process_terminated"] is False


def test_reference_capture_contract_is_hash_bound_create_new_and_temporary():
    runner = RUNNER.read_text(encoding="utf-8")
    script = SCRIPT.read_text(encoding="utf-8")
    assert "Get-Process AfterFX,aerender,aerendercore" in runner
    assert "Installed AEX hash does not match tested AEX" in runner
    assert "OutputPng already exists" in runner
    assert "Test-Path -LiteralPath $resultPath" in runner
    assert "[DateTime]::UtcNow -lt $deadline" in runner
    assert "After Effects reference capture timed out without a result" in runner
    assert "-r \"{0}\"" in runner
    assert "Resolve-Path -LiteralPath $ScriptPath" in runner
    assert "refusing to modify a non-empty or saved After Effects project" in script
    assert "safeToQuit" in script
    assert 'addProperty(env("AEXCOMPAT_AE_EFFECT"))' in script
    assert "comp.saveFrameToPng(comp.time, outputFile)" in script
    assert 'AEXCOMPAT_AE_NO_EFFECT' in script
    assert 'effect_applied: !noEffect' in script
    assert "CloseOptions.DO_NOT_SAVE_CHANGES" in script


def test_reference_capture_color_pipeline_pin_is_optional_and_fail_closed():
    runner = RUNNER.read_text(encoding="utf-8")
    script = SCRIPT.read_text(encoding="utf-8")
    # The runner only exports the pin variables when explicitly requested, so
    # existing captures keep the fresh-project defaults byte-for-byte, and it
    # clears inherited ambient values so the environment cannot pin silently.
    assert "Remove-Item Env:AEXCOMPAT_AE_WORKING_SPACE -ErrorAction SilentlyContinue" in runner
    assert "Remove-Item Env:AEXCOMPAT_AE_LINEARIZE -ErrorAction SilentlyContinue" in runner
    assert "if ($WorkingSpace) { $env:AEXCOMPAT_AE_WORKING_SPACE = $WorkingSpace }" in runner
    assert "if ($LinearizeWorkingSpace) { $env:AEXCOMPAT_AE_LINEARIZE = $LinearizeWorkingSpace }" in runner
    assert "'AEXCOMPAT_AE_WORKING_SPACE','AEXCOMPAT_AE_LINEARIZE'" in runner
    # The JSX verifies every pin by readback and records the observed state.
    assert "working space did not apply" in script
    assert "linearize working space did not apply" in script
    assert "working_space: app.project.workingSpace" in script
    assert "linearize_working_space: app.project.linearizeWorkingSpace" in script
