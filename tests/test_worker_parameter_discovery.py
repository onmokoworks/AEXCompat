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

from _render_session import HARNESS, assert_artifact_fresh, run_session_render

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-param-utils-animation-probe" / "pf_param_utils_animation_probe.cpp"
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
PROBE = (ROOT / "target" / "pf-param-utils-animation-probe-build" / "Release"
         / "pf_param_utils_animation_probe.aex")


def test_parameter_declaring_aex_passes_the_count_contract(tmp_path):
    if not WORKER.is_file():
        pytest.skip("aex_worker.exe is not built; run the minihost build")
    if not PROBE.is_file():
        pytest.skip("pf_param_utils_animation_probe.aex is not built")
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    input_path = tmp_path / "input.rgba"
    output_path = tmp_path / "output.rgba"
    input_path.write_bytes(bytes([255, 0, 0, 0]) * (7 * 5))
    report = run_session_render(
        tmp_path, PROBE, input_path, output_path, width=7, height=5,
    )
    assert report["render_error"] == 0
    assert output_path.is_file() and output_path.stat().st_size == 7 * 5 * 4
