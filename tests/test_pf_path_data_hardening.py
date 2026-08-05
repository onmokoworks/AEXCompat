import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "worker_pf_path_runtime.cpp"
SELFTEST_SOURCE = ROOT / "minihost" / "src" / "worker_pf_path_selftests.cpp"
CALLBACK_SOURCE = ROOT / "minihost" / "src" / "worker_mask_runtime_callbacks.cpp"
CMAKE = ROOT / "minihost" / "CMakeLists.txt"

def _worker() -> Path:
    candidates = (
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    )
    return next(path for path in candidates if path.exists())

def test_path_hardening_runtime_self_test():
    completed = subprocess.run(
        [str(_worker()), "--self-test-pf-path-data-hardening"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(completed.stdout)
    assert report == {
        "pf_path_data_hardening": "passed",
        "created": 6,
        "disposed": 6,
        "live": 0,
        "balanced": True,
    }
