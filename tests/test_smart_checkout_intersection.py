import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"






def test_native_intersection_self_test_passes_all_three_workers() -> None:
    expected = {"pf_checkout_intersection": "passed"}
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-pf-checkout-intersection"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected
