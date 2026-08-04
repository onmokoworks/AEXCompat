import hashlib
import json
import subprocess
from pathlib import Path

from _render_session import HARNESS, assert_artifact_fresh, run_session_render

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-render-options4-tail-probe" / "pf_aegp_render_options4_tail_probe.cpp"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-render-options4-tail-probe-build" / "Release" / "pf_aegp_render_options4_tail_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"






def test_real_probe_exercises_render_options4_tail(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    assert INPUT.is_file()
    output = tmp_path / "render-options4-tail-output.rgba"
    report = run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 4
    assert report["guard_bytes_intact"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)
