import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build" / "Release" / "aex_l2_worker.exe",
        ROOT / "target" / "minihost-build" / "aex_l2_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_utils_handle_callbacks_are_reachable_through_in_data_utils():
    worker = _worker()
    assert worker is not None, "build a worker before running the runtime test"
    completed = subprocess.run(
        [worker, "--self-test-pf-utils-handle-callbacks"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {
        "pf_utils_handle_callbacks": "passed",
        "reached_via_in_data_utils": True,
        "offsets": [160, 168, 176, 184, 440, 464],
    }
