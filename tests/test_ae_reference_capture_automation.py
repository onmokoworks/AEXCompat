import json
import shutil
import sys
from pathlib import Path

import pytest


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
    assert "Get-Process AfterFX,'AfterFX.com',aerender,aerendercore" in runner
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
    assert "'-m -r \"{0}\"'" in RUNNER.read_text(encoding="utf-8")
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


@pytest.mark.skipif(
    sys.platform != "win32" or shutil.which("powershell") is None,
    reason="mock capture run requires Windows PowerShell and a Windows executable")
def test_reference_capture_result_records_prelaunch_input_identities(tmp_path):
    # Behavioral check (no After Effects needed): the runner is executed with
    # a mock AE binary; while the "capture" is in flight the input image is
    # replaced, and the JSX side effects (result JSON + PNG) are simulated.
    # The runner must record the hashes taken BEFORE launch - the bytes the
    # real AE would have rendered - not the replaced file.
    import hashlib
    import subprocess
    import time

    aex = tmp_path / "fixture.aex"
    aex.write_bytes(b"fixture-bytes")
    original_input = b"original-input-bytes"
    input_image = tmp_path / "input.png"
    input_image.write_bytes(original_input)
    mock_ae = tmp_path / "mock-afterfx.exe"
    shutil.copyfile(Path("C:/Windows/System32/where.exe"), mock_ae)
    output = tmp_path / "capture.png"
    result_path = tmp_path / "capture.result.json"

    process = subprocess.Popen(
        ["powershell", "-NoProfile", "-File", str(RUNNER),
         "-AfterEffects", str(mock_ae), "-TestedAex", str(aex),
         "-InstalledAex", str(aex), "-InputImage", str(input_image),
         "-OutputPng", str(output), "-EffectName", "Fixture",
         "-TimeoutSeconds", "60"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        time.sleep(5)  # let the runner hash the inputs and enter its poll loop
        input_image.write_bytes(b"replaced-while-ae-was-running")
        output.write_bytes(b"png-placeholder")
        result_path.write_text(
            '{"schema_version": 1, "status": "captured"}', encoding="utf-8")
        stdout, stderr = process.communicate(timeout=90)
    finally:
        process.kill()
    assert process.returncode == 0, stderr

    recorded = json.loads(result_path.read_text(encoding="utf-8-sig"))
    assert recorded["input_sha256"] == hashlib.sha256(original_input).hexdigest()
    assert recorded["tested_aex_sha256"] == hashlib.sha256(b"fixture-bytes").hexdigest()
    assert json.loads(stdout)["input_sha256"] == recorded["input_sha256"]
