"""Behavioral registry coverage, including the legacy flags regression (#1833)."""
import subprocess
import sys
import pytest
from _native_selftest import ROOT, locate, run, worker_self_test

pytestmark = pytest.mark.skipif(sys.platform != "win32", reason="Windows native world registry")

def test_world_reclamation():
    report = run("worker_world_reclamation_selftest.exe", "world_reclamation")
    assert report["reclaimed"] == 66
    assert report["reclaimed_bytes"] == 128 * 1024 * 1024 + 304
    assert report["checks"] >= 90

def test_legacy_new_world_flags():
    result = subprocess.run([str(locate("worker_legacy_new_world_flags_selftest.exe"))],
                            cwd=ROOT, capture_output=True, text=True, timeout=30)
    assert result.returncode == 0, result.stderr

def test_world_reclamation_lifecycle():
    for report in worker_self_test("--self-test-world-reclamation-lifecycle").values():
        assert report["world_reclamation_lifecycle"] == "passed"
