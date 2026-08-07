import subprocess
from pathlib import Path

from _render_session import HARNESS, assert_artifact_fresh, run_session_render

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-convolve-depth-probe" / "pf_convolve_depth_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-convolve-depth-probe.ps1"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-convolve-depth-probe-build" / "Release" / "pf_convolve_depth_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_builds_against_world_transform_suite1():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
                   cwd=ROOT, check=True, timeout=180)
    assert PROBE.is_file()


def test_real_probe_validates_argb8_convolution_and_aliasing(tmp_path):
    assert WORKER.is_file() and PROBE.is_file() and INPUT.is_file()
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    output = tmp_path / "convolve-depth-output.rgba"
    report = run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 2
    assert report["guard_bytes_intact"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)
