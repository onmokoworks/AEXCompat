import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
KINDS = ("discovery", "classic", "smart")

def test_native_cleanup_and_output_guard_selftest():
    assert WORKER.exists(), f"missing worker: {WORKER}"
    for kind in KINDS:
        completed = subprocess.run(
            [str(WORKER), "--kind", kind, "--self-test-render-output-safety"],
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
