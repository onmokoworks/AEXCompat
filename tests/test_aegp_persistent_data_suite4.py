"""Behavioral ABI test for AEGP Persistent Data Suite version 4."""

from pathlib import Path

from _native_selftest import worker_self_test


ROOT = Path(__file__).resolve().parents[1]


def test_native_aegp_persistent_data_suite4_passes_all_three_workers() -> None:
    expected = {"aegp_persistent_data_suite4": "passed"}
    for report in worker_self_test("--self-test-aegp-persistent-data-suite4").values():
        assert report == expected
