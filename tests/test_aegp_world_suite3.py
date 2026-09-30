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

def test_world_suite3_runtime_matrix():
    worker = _worker()
    assert worker is not None, "build aex_worker.exe (pwsh -File tools/build-native.ps1) before running the focused runtime test"
    result = subprocess.run(
        [str(worker), "--kind", "classic", "--self-test-aegp-world-suite3"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
    assert result.stdout.strip() == '{"aegp_world_suite3":"passed"}'
