import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _selftest() -> Path:
    configured = os.environ.get("AEXCOMPAT_COMPUTE_CACHE_SELFTEST")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-build/Release/worker_aegp_compute_cache_selftest.exe",
        ROOT / "target/minihost-build/worker_aegp_compute_cache_selftest.exe",
    ]
    return next(
        (candidate for candidate in candidates if candidate and candidate.is_file()),
        candidates[-1],
    )




def test_compute_cache_native_selftest_target_and_runtime():
    selftest = _selftest()
    assert selftest.is_file(), "build worker_aegp_compute_cache_selftest first"
    result = subprocess.run(
        [str(selftest)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
