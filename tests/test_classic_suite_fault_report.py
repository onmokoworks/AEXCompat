from _native_selftest import run


def test_classic_report_emits_explicit_suite_fault_evidence():
    report = run(
        "worker_classic_suite_report_selftest.exe",
        "classic_suite_fault_report",
    )
    assert report["false_and_true_mutations"] is True
