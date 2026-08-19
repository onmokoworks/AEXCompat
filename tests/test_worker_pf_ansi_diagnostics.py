import json
import os
import subprocess

import pytest

from _native_selftest import ROOT, locate_optional


NAME = "worker_pf_ansi_diagnostics_selftest.exe"


def test_pf_ansi_callbacks_report_clamps_without_polluting_history():
    if os.name != "nt":
        pytest.skip("the PF ANSI runtime uses the Windows CRT")
    executable = locate_optional(NAME)
    if executable is None:
        if os.environ.get("CI"):
            raise AssertionError(f"CI minihost build did not produce {NAME}")
        pytest.skip(f"build {NAME} in target/minihost-build")

    completed = subprocess.run(
        [str(executable)],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
        env={**os.environ, "AEXCOMPAT_EXTENDED_DIAG": "1"},
    )
    report = json.loads(completed.stdout)
    assert report["callback_diagnostics"] == {
        "ansi": {
            "calls": 29,
            "successes": 19,
            "failures": 10,
            "last_result": 4,
            "denials": {"invalid_arguments": 2, "clamped": 8},
        }
    }
    assert report["callback_history"] == []
    assert "extended_diag:ansi -> 4 (clamped)" in completed.stderr
