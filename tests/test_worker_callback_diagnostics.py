import json
import os
import shutil
import subprocess
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]


def test_smartfx_callback_results_and_denial_reasons_are_reportable_on_mac(tmp_path):
    compiler = shutil.which("clang++") or shutil.which("c++")
    if compiler is None:
        pytest.skip("a portable C++17 compiler is unavailable")
    executable = tmp_path / "worker_callback_diagnostics_selftest"
    subprocess.run(
        [
            compiler,
            "-std=c++17",
            "-D__cdecl=",
            # Compiling on Windows pulls in minwindef.h, whose min/max macros
            # break std::min in the runtime. minihost/CMakeLists.txt defines
            # these for every target; this ad-hoc command has to as well.
            "-DNOMINMAX",
            "-DWIN32_LEAN_AND_MEAN",
            "-I",
            str(ROOT / "minihost" / "src"),
            str(ROOT / "tests" / "native" / "worker_callback_diagnostics_selftest.cpp"),
            str(ROOT / "minihost" / "src" / "worker_smart_runtime.cpp"),
            "-o",
            str(executable),
        ],
        cwd=ROOT,
        check=True,
    )
    completed = subprocess.run(
        [str(executable)],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        env={**os.environ, "AEXCOMPAT_EXTENDED_DIAG": "1"},
    )
    report = json.loads(completed.stdout)
    diagnostics = report["callback_diagnostics"]
    history = report["callback_history"]
    assert len(history) == 4
    assert [entry["sequence"] for entry in history] == [0, 1, 2, 3]
    assert history[-1] == {
        "sequence": 3,
        "callback": "pre_checkout_layer",
        "result": 4,
        "reason": "temporal_checkout_denied",
    }
    assert diagnostics["checkout_output"] == {
        "calls": 2,
        "successes": 1,
        "failures": 1,
        "last_result": 4,
        "denials": {"invalid_arguments": 1},
    }
    assert diagnostics["checkout_pixels"]["denials"] == {"unknown_checkout": 1}
    assert diagnostics["pre_checkout_layer"]["denials"] == {
        "temporal_checkout_denied": 1
    }
    assert "extended_diag:checkout_output -> 0" in completed.stderr
    assert "extended_diag:checkout_pixels -> 4 (unknown_checkout)" in completed.stderr
    assert (
        "extended_diag:pre_checkout_layer -> 4 (temporal_checkout_denied)"
        in completed.stderr
    )
