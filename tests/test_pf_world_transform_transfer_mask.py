import json
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]


def _worker():
    candidates = (
        ROOT / "target/minihost-build-v18/aex_render_worker.exe",
        ROOT / "target/minihost-build/aex_render_worker.exe",
    )
    return next((path for path in candidates if path.is_file()), None)


def test_transfer_mask_self_test_is_exposed_by_source():
    source = source_owners.worker_text()
    assert "verify_world_transform_transfer_mask" in source
    assert "--self-test-world-transform-transfer-mask" in source


def test_transfer_mask_runtime_matrix():
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [worker, "--self-test-world-transform-transfer-mask"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {"world_transform_transfer_mask": "passed"}
