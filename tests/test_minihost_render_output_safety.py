import json
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
DISPATCH = ROOT / "minihost" / "src" / "worker_selector_dispatch.cpp"
SMART_FINALIZE = ROOT / "minihost" / "src" / "worker_smart_finalize.cpp"
PIXEL_BUFFER = ROOT / "minihost" / "src" / "render_pixel_buffer.cpp"
WORKERS = [
    ROOT / "target" / "minihost-build" / "aex_l2_worker.exe",
    ROOT / "target" / "minihost-build" / "aex_render_worker.exe",
    ROOT / "target" / "minihost-build" / "aex_smart_worker.exe",
]




def test_native_cleanup_and_output_guard_selftest():
    for worker in WORKERS:
        assert worker.exists(), f"missing worker: {worker}"
        completed = subprocess.run(
            [str(worker), "--self-test-render-output-safety"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        report = json.loads(completed.stdout)
        assert report["render_output_safety"] == "passed"
        assert report["cleanup_selector"] == "SMART_PRE_RENDER_CLEANUP"
        assert report["cleanup_error"] == 512
        assert report["cleanup_calls"] == 1
        assert report["guard_pages"] is True
        assert report["overrun_beyond_64_detected"] is True
