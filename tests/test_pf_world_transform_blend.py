import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def test_blend_runtime_is_alias_safe():
    worker = ROOT / "target/minihost-build/aex_worker.exe"
    result = subprocess.run([str(worker), "--kind", "classic", "--self-test-world-transform-blend"], cwd=ROOT,
                            text=True, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stdout + result.stderr
    assert '"world_transform_blend":"passed"' in result.stdout



def test_transfer_rect_native_vectors_cover_blend_and_alpha_modes():
    worker = ROOT / "target/minihost-build/aex_worker.exe"
    result = subprocess.run([str(worker), "--kind", "classic", "--self-test-world-transform-transfer-mask"],
                            cwd=ROOT, text=True, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stdout + result.stderr
    assert '"world_transform_transfer_mask":"passed"' in result.stdout
