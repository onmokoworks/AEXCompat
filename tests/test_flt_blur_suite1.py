"""Behavioral tests for the private AE 2026 ``FLT Blur Suite`` version 1.

Bounded call-site observation of Box_Blur, Gaussian_Blur, RoughenEdges, and
Simple_Choker established a two-slot x64 table. Slot 0 is Gaussian blur and
slot 1 is repeated box blur. Both take borrowed source/destination worlds;
the source remains immutable and only the destination pixels are written.
"""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"


def test_native_flt_blur_suite1_passes_all_three_workers() -> None:
    expected = {"flt_blur_suite1": "passed"}
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-flt-blur-suite1"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout) == expected
