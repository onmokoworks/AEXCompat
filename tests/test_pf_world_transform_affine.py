import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "minihost" / "src" / "worker_pf_world_transform_runtime.cpp"

def test_affine_runtime_covers_both_directions_and_scale():
    worker = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
    result = subprocess.run([str(worker), "--self-test-world-transform-affine"], cwd=ROOT,
                            text=True, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stdout + result.stderr
    assert '"world_transform_affine":"passed"' in result.stdout


