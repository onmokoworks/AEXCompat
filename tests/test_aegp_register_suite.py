"""Behavioral coverage for PF-safe and AEGP Register Suite providers."""

import subprocess

from _native_selftest import ROOT, locate


def test_aegp_register_suite_native_selftest():
    selftest = locate(
        "worker_aegp_register_suite_selftest.exe", "AEXCOMPAT_REGISTER_SUITE_SELFTEST"
    )
    completed = subprocess.run(
        [str(selftest)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
