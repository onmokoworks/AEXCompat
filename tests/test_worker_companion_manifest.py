"""Behavioral validation of the authenticated companion manifest parser."""

import subprocess

from _native_selftest import ROOT, locate


def test_companion_manifest_rejects_shape_identity_and_bound_mutations():
    selftest = locate(
        "worker_companion_manifest_selftest.exe",
        "AEXCOMPAT_COMPANION_MANIFEST_SELFTEST",
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
