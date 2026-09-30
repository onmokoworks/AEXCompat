from _native_selftest import run


def test_classic_report_keeps_rejected_suite_release_a_benign_warning():
    # #1182: a suite release without a matching acquire is contained by the
    # registry (returns rejection, touches no host state), so it is counted as a
    # diagnostic but never sets suite_fault_observed. The self-test verifies the
    # rejection is counted while the classic report keeps suite_fault_observed
    # false before and after.
    report = run(
        "worker_classic_suite_report_selftest.exe",
        "classic_suite_fault_report",
    )
    assert report["rejected_release_is_benign_warning"] is True
