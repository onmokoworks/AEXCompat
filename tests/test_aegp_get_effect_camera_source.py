import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"

def test_native_self_test_covers_all_three_release_workers():
    for worker_name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / worker_name
        assert worker.exists(), f"build {worker_name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-aegp-get-effect-camera"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        result = json.loads(completed.stdout)
        assert result == {
            "aegp_get_effect_camera": "passed",
            "camera_slot": 3,
            "camera_offset_x64": 24,
            "matrix_slot": 4,
            "matrix_offset_x64": 32,
            "classic": "tested",
            "smart": "tested",
        }
