import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)










def test_mask_composition_runtime_self_test():
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(worker), "--self-test-pf-mask-composition"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    report = json.loads(completed.stdout)
    assert report == {
        "pf_mask_composition": "passed",
        "composition_calls": 10,
        "balanced": True,
    }
