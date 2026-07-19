import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def test_blend_runtime_is_alias_safe():
    worker = ROOT / "target/minihost-build/aex_render_worker.exe"
    result = subprocess.run([str(worker), "--self-test-world-transform-blend"], cwd=ROOT,
                            text=True, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stdout + result.stderr
    assert '"world_transform_blend":"passed"' in result.stdout

def test_blend_uses_registered_formats_and_snapshots_both_sources():
    source = (ROOT / "minihost/src/worker_pf_suites.cpp").read_text(encoding="utf-8")
    worker = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
    assert "first_info.pixel_format != second_info.pixel_format" in source
    assert "first_copy.resize" in source and "second_copy.resize" in source
    assert "--self-test-world-transform-blend" in worker

def test_transfer_rect_implements_sdk_blend_mode_families():
    source = (ROOT / "minihost/src/worker_pf_suites.cpp").read_text(encoding="utf-8")
    for marker in [
        "case 4: case 29:", "case 5:", "case 6:", "case 7:", "case 8:",
        "case 9:", "case 10:", "case 11:", "case 12: case 26:",
        "transfer_mode >= 13 && transfer_mode <= 16",
        "transfer_mode >= 17 && transfer_mode <= 20", "transfer_mode == 21",
        "transfer_mode == 22", "case 23: case 27:", "case 24: case 28:",
        "case 25:", "case 30:", "case 31:", "case 32:", "case 33:",
        "case 34:", "transfer_mode == 35", "case 37:", "case 38:",
    ]:
        assert marker in source

def test_transfer_rect_native_vectors_cover_blend_and_alpha_modes():
    worker = ROOT / "target/minihost-build/aex_render_worker.exe"
    result = subprocess.run([str(worker), "--self-test-world-transform-transfer-mask"],
                            cwd=ROOT, text=True, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stdout + result.stderr
    assert '"world_transform_transfer_mask":"passed"' in result.stdout
