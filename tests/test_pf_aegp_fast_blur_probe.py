import hashlib
import json
import subprocess
from pathlib import Path

from _render_session import run_session_render


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-fast-blur-probe" / "pf_aegp_fast_blur_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-aegp-fast-blur-probe.ps1"
PROBE = ROOT / "target" / "pf-aegp-fast-blur-probe-build" / "Release" / "pf_aegp_fast_blur_probe.aex"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_builds_against_world_suite3():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT, check=True, timeout=180,
    )
    assert PROBE.is_file()
    source = SOURCE.read_text(encoding="utf-8")
    assert "sizeof(AEGP_WorldSuite3) == 13 * sizeof(void*)" in source
    assert "offsetof(AEGP_WorldSuite3, AEGP_FastBlur) == 9 * sizeof(void*)" in source


def test_probe_covers_impulses_blur_and_invalid_owned_world_lifecycle():
    source = SOURCE.read_text(encoding="utf-8")
    for operation in ("AEGP_New", "AEGP_GetBaseAddr8", "AEGP_GetRowBytes", "AEGP_FastBlur", "AEGP_Dispose"):
        assert f"->{operation}" in source
    for impulse in (
        "row(3)[5] = {255, 240, 80, 20}",
        "row(1)[2] = {192, 12, 160, 48}",
        "row(5)[8] = {128, 32, 64, 224}",
    ):
        assert impulse in source
    assert "nonzero > 3 && alpha_sum > 0" in source
    assert "AEGP_FastBlur(-1.0" in source
    assert "AEGP_FastBlur(2.0, PF_MF_Alpha_STRAIGHT, PF_Quality_HI, nullptr)" in source
    assert "AEGP_WorldH stale = world" in source
    assert "AEGP_FastBlur(2.0, PF_MF_Alpha_STRAIGHT, PF_Quality_HI, stale)" in source
    assert "AEGP_Dispose(stale) == A_Err_NONE" in source


def test_real_probe_blurs_owned_world_and_copies_nonzero_pixels(tmp_path):
    output = tmp_path / "fast-blur-output.rgba"
    report = run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 2
    pixels = output.read_bytes()
    assert len(pixels) == 37 * 23 * 4
    assert any(pixels)
