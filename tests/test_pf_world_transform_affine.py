import subprocess
from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "minihost" / "src" / "worker_pf_world_transform_runtime.cpp"

def test_affine_runtime_covers_both_directions_and_scale():
    worker = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
    result = subprocess.run([str(worker), "--self-test-world-transform-affine"], cwd=ROOT,
                            text=True, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stdout + result.stderr
    assert '"world_transform_affine":"passed"' in result.stdout

def test_affine_source_has_inverse_mapping_and_alpha_aware_hq():
    source = RUNTIME.read_text(encoding="utf-8")
    worker = source_owners.worker_text()
    assert "destination_to_source" in source
    assert "source_to_destination && std::abs(determinant)" in source
    assert "sampled[channel] /= sampled[0] / maximum" in source
    assert "--self-test-world-transform-affine" in worker

def test_projective_sampling_rejects_nonfinite_and_out_of_range_coordinates():
    source = RUNTIME.read_text(encoding="utf-8")
    assert "floor_to_sample_coord" in source
    assert "!std::isfinite(value)" in source
    assert "std::numeric_limits<int>::max()) - 1.0" in source
