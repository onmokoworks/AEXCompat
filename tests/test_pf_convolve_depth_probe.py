import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-convolve-depth-probe" / "pf_convolve_depth_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-convolve-depth-probe.ps1"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-convolve-depth-probe-build" / "Release" / "pf_convolve_depth_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_builds_against_world_transform_suite1():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
                   cwd=ROOT, check=True, timeout=180)
    source = SOURCE.read_text(encoding="utf-8")
    assert "const PF_WorldTransformSuite1* transforms" in source
    assert "sizeof(PF_WorldTransformSuite1) == 7 * sizeof(void*)" in source
    assert "offsetof(PF_WorldTransformSuite1, convolve) == 2 * sizeof(void*)" in source
    assert "kPFWorldTransformSuiteVersion1" in source


def test_probe_has_exact_identity_border_and_alias_vectors():
    source = SOURCE.read_text(encoding="utf-8")
    assert "identity[4] = 2295" in source
    assert "identity, unnormalized" in source
    assert "normalized = unnormalized | PF_KernelFlag_NORMALIZED" in source
    assert "0,-255,0,-255,1275,-255,0,-255,0" in source
    assert "kBlur{{13,23,18,30,50,37,27,43,31}}" in source
    assert "kSharpen{{0,10,70,70,50,130,230,190,255}}" in source
    assert "PF_KernelFlag_TRANSPARENT_BORDERS" in source
    assert "&source, blur, normalized" in source
    assert "format != PF_PixelFormat_ARGB32" in source


def test_real_probe_validates_argb8_convolution_and_aliasing(tmp_path):
    assert WORKER.is_file() and PROBE.is_file() and INPUT.is_file()
    output = tmp_path / "convolve-depth-output.rgba"
    completed = subprocess.run(
        [str(WORKER), "--render-image", str(PROBE), hashlib.sha256(PROBE.read_bytes()).hexdigest(),
         "v5|", str(INPUT), str(output), "37", "23", "0", "1", "1", "1"],
        cwd=ROOT, text=True, encoding="utf-8", errors="replace", capture_output=True, timeout=30)
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 2
    assert report["guard_bytes_intact"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)
