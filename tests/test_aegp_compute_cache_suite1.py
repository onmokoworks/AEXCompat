import subprocess

from _native_selftest import ROOT, locate


def test_compute_cache_native_selftest_target_and_runtime():
    selftest = locate(
        "worker_aegp_compute_cache_selftest.exe", "AEXCOMPAT_COMPUTE_CACHE_SELFTEST"
    )
    result = subprocess.run(
        [str(selftest)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
