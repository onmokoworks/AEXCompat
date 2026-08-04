import json
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
OWNER_SOURCES = (
    SOURCE,
    ROOT / "minihost" / "src" / "worker_entry_wiring.cpp",
    ROOT / "minihost" / "src" / "worker_smart_runtime.cpp",
    ROOT / "minihost" / "src" / "worker_smart_setup.cpp",
    ROOT / "minihost" / "src" / "worker_smart_dispatch.cpp",
    ROOT / "minihost" / "src" / "worker_render_report.cpp",
)
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
