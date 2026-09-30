import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)










def test_pf_pixel_data_native_depth_and_registry_matrix():
    executable = worker()
    assert executable is not None, "build aex_worker.exe (pwsh -File tools/build-native.ps1) before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--kind", "classic", "--self-test-pf-pixel-data"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_pixel_data_suite":"passed"}'
