import subprocess
from pathlib import Path

from _render_session import HARNESS, assert_artifact_fresh, run_session_render


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-fast-blur-probe" / "pf_aegp_fast_blur_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-aegp-fast-blur-probe.ps1"
PROBE = ROOT / "target" / "pf-aegp-fast-blur-probe-build" / "Release" / "pf_aegp_fast_blur_probe.aex"
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_builds_against_world_suite3():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT, check=True, timeout=180,
    )
    assert PROBE.is_file()


def test_real_probe_blurs_owned_world_and_copies_nonzero_pixels(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    output = tmp_path / "fast-blur-output.rgba"
    report = run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 2
    pixels = output.read_bytes()
    assert len(pixels) == 37 * 23 * 4
    assert any(pixels)
