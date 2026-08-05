"""Behavioral tests for the private AE 2026 ``AEFX ACE Suite`` version 1.

Issue #776: ``Photo Filter.aex`` refused every frame with error 516 ("Not able
to acquire AEFX Suite") because the host published no such suite. Observation
of its RENDER path (the worker's suite-call slot probe, then the caller's own
disassembly) fixed a three-slot x64 table whose slot 0 widens packed 8-bit
pixels into AE's 0..32768 16-bit range and whose slot 2 narrows them back;
slot 1 is never called and stays a diagnosed unsupported slot.

The quality argument is one byte: the caller writes it with ``sete dl`` and
leaves the rest of the register alone, so a word-sized parameter would carry
whatever was there before.
"""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
WORKERS = ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe")


def test_native_aefx_ace_suite1_passes_all_three_workers() -> None:
    expected = {"aefx_ace_suite1": "passed"}
    for name in WORKERS:
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-aefx-ace-suite1"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout) == expected
