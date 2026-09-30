from pathlib import Path

from _native_selftest import worker_self_test

ROOT = Path(__file__).resolve().parents[1]

def test_native_geometry_rect_self_test_passes_all_three_workers() -> None:
    expected = {"pf_smart_geometry_rects": "passed"}
    for report in worker_self_test("--self-test-pf-smart-geometry-rects").values():
        assert report == expected
