"""Behavioral coverage for process-local companion suite registration."""

import subprocess

from _native_selftest import ROOT, locate


def test_dynamic_suite_registry_native_lifecycle():
    selftest = locate(
        "worker_dynamic_suite_registry_selftest.exe",
        "AEXCOMPAT_DYNAMIC_SUITE_REGISTRY_SELFTEST",
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
