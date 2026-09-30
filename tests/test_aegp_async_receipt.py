import json
import os
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]

def _worker() -> pathlib.Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        pathlib.Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)

def test_async_ready_receipt_runtime_lifecycle():
    worker = _worker()
    assert worker is not None, "build aex_worker.exe (pwsh -File tools/build-native.ps1) before running the focused runtime test"
    result = subprocess.run(
        [str(worker), "--kind", "classic", "--self-test-aegp-async-receipt"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
    report = json.loads(result.stdout)
    assert report["aegp_async_receipt"] == "passed"
    assert report["created"] == report["checked_in"] == 4
    assert report["live"] == 0
    assert report["live_bytes"] == 0
    assert report["invalid_operations"] >= 4
