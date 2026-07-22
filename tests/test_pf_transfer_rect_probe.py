import hashlib
import json
import subprocess
from pathlib import Path

from _render_session import run_session_render

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-transfer-rect-probe" / "pf_transfer_rect_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-transfer-rect-probe.ps1"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-transfer-rect-probe-build" / "Release" / "pf_transfer_rect_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"
RUNTIME = ROOT / "minihost" / "src" / "worker_pf_world_transform_runtime.cpp"

def test_probe_builds_against_transfer_rect_slot5():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
                   cwd=ROOT, check=True, timeout=180)
    source = SOURCE.read_text(encoding="utf-8")
    assert "offsetof(PF_WorldTransformSuite1, transfer_rect) == 5 * sizeof(void*)" in source
    assert "PF_Xfer_IN_FRONT" in source and "PF_Xfer_DIFFERENCE" in source

def test_probe_contains_exact_partial_alpha_vectors():
    source = SOURCE.read_text(encoding="utf-8")
    assert "source_over{160, 164, 36, 120}" in source
    assert "difference{64, 100, 90, 150}" in source
    assert "PF_MF_Alpha_STRAIGHT" in source

def test_host_accepts_all_public_sdk_transfer_modes_and_rejects_reserved_values():
    source = RUNTIME.read_text(encoding="utf-8")
    assert "transfer_mode < 0 || transfer_mode > 38" in source
    assert "random_seed" in source
    assert "hash & 0x00ffffffu" in source

def test_real_probe_validates_transfer_pixels(tmp_path):
    output = tmp_path / "transfer-output.rgba"
    report = run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"] == "render_completed" and report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)
