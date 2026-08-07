import json
import shutil
import sys
import uuid
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "AE_REFERENCE_CAPTURE_AUTOMATION_RESULT_2026-07-15.json"
RUNNER = ROOT / "tools" / "capture-ae-reference.ps1"


def _wait_for_staged_input(marker: bytes, process, timeout: float = 60.0) -> None:
    """Block until the runner has staged its private copy of the input.

    The staging copy is the runner's LAST preflight step -- it happens after the
    OutputPng and result-path existence checks -- so its appearance is the only
    reliable signal that writing those files will not race the preflight. A
    fixed sleep is not: on a loaded hosted runner the runner can still be in its
    preflight after several seconds, and the test's write then makes it fail
    with "OutputPng already exists" instead of exercising the path under test
    (issue #175).

    `marker` must be unique to this run: a staging copy leaked by an earlier run
    would otherwise satisfy the wait immediately.
    """
    import subprocess
    import tempfile
    import time

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for candidate in Path(tempfile.gettempdir()).glob("aexcompat-ae-input-*.png"):
            try:
                if candidate.read_bytes() == marker:
                    return
            except OSError:
                pass
        if process.poll() is not None:
            stdout, stderr = process.communicate(timeout=5)
            raise AssertionError(f"capture runner exited before staging input: {stderr}")
        time.sleep(0.05)
    # The runner is still alive by definition here, so communicate() would raise
    # TimeoutExpired and hide the real reason. Kill it first, then report.
    process.kill()
    try:
        _, stderr = process.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        stderr = "<runner did not exit after kill>"
    raise AssertionError(f"capture runner did not stage input within {timeout}s: {stderr}")


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

    aex = tmp_path / "fixture.aex"
    aex.write_bytes(b"fixture-bytes")
    original_input = f"original-input-bytes-{uuid.uuid4().hex}".encode("ascii")
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
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, errors="replace")
    try:
        _wait_for_staged_input(original_input, process)
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
    assert recorded["output_png_sha256"] == hashlib.sha256(b"png-placeholder").hexdigest()
    assert json.loads(stdout)["input_sha256"] == recorded["input_sha256"]


@pytest.mark.skipif(
    sys.platform != "win32" or shutil.which("powershell") is None,
    reason="mock capture run requires Windows PowerShell and a Windows executable")
def test_reference_capture_shutdown_never_touches_unrelated_ae_named_processes(tmp_path):
    # Counterexample for the shutdown path: an AE-named process that appears
    # AFTER the startup gate (a user launching After Effects mid-capture) must
    # survive the runner's post-result shutdown handling. The runner may only
    # wait on / kill the process identity it launched itself, never by name.
    import subprocess
    import time

    aex = tmp_path / "fixture.aex"
    aex.write_bytes(b"fixture-bytes")
    input_image = tmp_path / "input.png"
    shutdown_marker = f"input-bytes-{uuid.uuid4().hex}".encode("ascii")
    input_image.write_bytes(shutdown_marker)
    mock_ae = tmp_path / "mock-afterfx.exe"
    shutil.copyfile(Path("C:/Windows/System32/where.exe"), mock_ae)
    decoy_exe = tmp_path / "AfterFX.com"
    shutil.copyfile(Path("C:/Windows/System32/ping.exe"), decoy_exe)
    output = tmp_path / "capture.png"
    result_path = tmp_path / "capture.result.json"

    process = subprocess.Popen(
        ["powershell", "-NoProfile", "-File", str(RUNNER),
         "-AfterEffects", str(mock_ae), "-TestedAex", str(aex),
         "-InstalledAex", str(aex), "-InputImage", str(input_image),
         "-OutputPng", str(output), "-EffectName", "Fixture",
         "-TimeoutSeconds", "60"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, errors="replace")
    decoy = None
    try:
        _wait_for_staged_input(shutdown_marker, process)
        decoy = subprocess.Popen(
            [str(decoy_exe), "-n", "60", "127.0.0.1"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1)
        output.write_bytes(b"png-placeholder")
        result_path.write_text(
            '{"schema_version": 1, "status": "captured"}', encoding="utf-8")
        stdout, stderr = process.communicate(timeout=90)
        assert process.returncode == 0, stderr
        assert decoy.poll() is None, \
            "unrelated AE-named process was terminated by the capture runner"
    finally:
        process.kill()
        if decoy is not None:
            decoy.kill()


@pytest.mark.skipif(
    sys.platform != "win32" or shutil.which("powershell") is None,
    reason="mock capture run requires Windows PowerShell and a Windows executable")
def test_reference_capture_fails_closed_without_loaded_module_identity(tmp_path):
    import subprocess

    aex = tmp_path / "fixture.aex"
    aex.write_bytes(b"fixture-bytes")
    input_image = tmp_path / "input.png"
    mock_ae = tmp_path / "mock-afterfx.exe"
    shutil.copyfile(Path("C:/Windows/System32/where.exe"), mock_ae)
    output = tmp_path / "capture.png"
    result_path = tmp_path / "capture.result.json"
    # Unique per run. The staged copy the runner makes lives in %TEMP% and is
    # left behind whenever this test kills the runner, so a fixed marker made
    # the synchronization below match a *previous* run's leftover and return
    # immediately -- the test then wrote OutputPng before the runner had reached
    # its own preflight, and the runner failed with "OutputPng already exists"
    # instead of the identity gate this test is about (issue #175).
    input_marker = f"identity-gate-input-{uuid.uuid4().hex}".encode("ascii")
    input_image.write_bytes(input_marker)

    process = subprocess.Popen(
        ["powershell", "-NoProfile", "-File", str(RUNNER),
         "-AfterEffects", str(mock_ae), "-TestedAex", str(aex),
         "-InstalledAex", str(aex), "-InputImage", str(input_image),
         "-OutputPng", str(output), "-EffectName", "Fixture",
         "-RequireLoadedAexIdentity", "-TimeoutSeconds", "60"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, errors="replace")
    try:
        _wait_for_staged_input(input_marker, process)
        output.write_bytes(b"png-placeholder")
        result_path.write_text(
            '{"schema_version": 1, "status": "captured"}', encoding="utf-8")
        _, stderr = process.communicate(timeout=90)
    finally:
        process.kill()

    assert process.returncode != 0
    assert "without verified loaded AEX module identity" in stderr
    recorded = json.loads(result_path.read_text(encoding="utf-8-sig"))
    assert recorded["status"] == "identity_unverified"
    assert recorded["loaded_aex_identity"] == {
        "state": "unverified", "reason": "loaded_module_not_observed"}


@pytest.mark.skipif(
    sys.platform != "win32" or shutil.which("powershell") is None,
    reason="file identity helper is Windows-only")
def test_locked_file_identity_distinguishes_equal_bytes_at_different_paths(tmp_path):
    import subprocess

    first = tmp_path / "first.aex"
    second = tmp_path / "second.aex"
    first.write_bytes(b"same-plugin-bytes")
    second.write_bytes(b"same-plugin-bytes")
    helper = ROOT / "tools" / "windows-file-identity.ps1"
    script = tmp_path / "identity.ps1"
    script.write_text(
        "param($Helper,$First,$Second)\n"
        ". $Helper\n"
        "$a=[IO.File]::Open($First,'Open','Read','Read'); "
        "$b=[IO.File]::Open($Second,'Open','Read','Read')\n"
        "try { @((Get-LockedFileIdentity $a),(Get-LockedFileIdentity $b)) "
        "| ConvertTo-Json -Depth 4 } finally { $a.Dispose(); $b.Dispose() }\n",
        encoding="utf-8")
    output = subprocess.check_output(
        ["powershell", "-NoProfile", "-File", str(script),
         "-Helper", str(helper), "-First", str(first), "-Second", str(second)],
        text=True, errors="replace")
    identities = json.loads(output)
    assert identities[0]["sha256"] == identities[1]["sha256"]
    assert identities[0]["canonical_path_sha256"] != identities[1]["canonical_path_sha256"]
    assert identities[0]["file_id"] != identities[1]["file_id"]
