"""Regression test for issue #177: dynamically discovered parameters.

The parameter count contract (`worker_effect_bootstrap.cpp`) must be
evaluated AFTER PARAMS_SETUP, because a generic AEX declares its parameters
through the add_param callback during that selector. The bootstrap-owner
extraction (f1e7323) briefly moved the expected count to a launch-time
snapshot, which rejected every parameter-declaring AEX with exit 3 while
parameter-free fixtures kept passing. This test drives the CURRENT build of
the render worker (not a stale side-by-side build) with a fixture that
declares one float slider.
"""
import hashlib
import json
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = (ROOT / "target" / "pf-param-utils-animation-probe-build" / "Release"
         / "pf_param_utils_animation_probe.aex")


def test_parameter_declaring_aex_passes_the_count_contract(tmp_path):
    if not WORKER.is_file():
        pytest.skip("aex_render_worker.exe is not built; run the minihost build")
    if not PROBE.is_file():
        pytest.skip("pf_param_utils_animation_probe.aex is not built")
    input_path = tmp_path / "input.rgba"
    output_path = tmp_path / "output.rgba"
    input_path.write_bytes(bytes([255, 0, 0, 0]) * (7 * 5))
    completed = subprocess.run(
        [str(WORKER), "--render-image", str(PROBE),
         hashlib.sha256(PROBE.read_bytes()).hexdigest(), "v5|",
         str(input_path), str(output_path), "7", "5", "12", "1", "24", "1"],
        cwd=ROOT, text=True, encoding="utf-8", errors="replace",
        capture_output=True, timeout=60, check=False)
    # Before the fix the worker exited 3 right after params_setup_end, so the
    # exit code alone is the regression signal; the parsed report pins the
    # render actually happening on top of the restored contract.
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["render_error"] == 0
    assert output_path.is_file() and output_path.stat().st_size == 7 * 5 * 4
