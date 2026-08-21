"""Behavioral tests for the private AE 2026 ``FLT Blur Suite`` version 1.

Bounded call-site observation of Box_Blur, Gaussian_Blur, RoughenEdges, and
Simple_Choker established a two-slot x64 table. Slot 0 is Gaussian blur and
slot 1 is repeated box blur. Both take borrowed source/destination worlds;
the source remains immutable and only the destination pixels are written.
"""

from pathlib import Path

from _native_selftest import worker_self_test


ROOT = Path(__file__).resolve().parents[1]


def test_native_flt_blur_suite1_passes_all_three_workers() -> None:
    expected = {"flt_blur_suite1": "passed"}
    for report in worker_self_test("--self-test-flt-blur-suite1").values():
        assert report == expected
