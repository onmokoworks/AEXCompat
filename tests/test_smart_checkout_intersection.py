from pathlib import Path

from _native_selftest import worker_self_test


ROOT = Path(__file__).resolve().parents[1]






def test_native_intersection_self_test_passes_all_three_workers() -> None:
    expected = {"pf_checkout_intersection": "passed"}
    for report in worker_self_test("--self-test-pf-checkout-intersection").values():
        assert report == expected
