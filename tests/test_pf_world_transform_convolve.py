import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_convolve_runtime_clips_output_area():
    worker = ROOT / "target" / "minihost-build" / "aex_worker.exe"
    result = subprocess.run(
        [str(worker), "--kind", "classic", "--self-test-world-transform-convolve"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr
    assert '"world_transform_convolve":"passed"' in result.stdout
