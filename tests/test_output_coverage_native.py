"""Output-coverage mutations run against the compiled host checker."""

from _native_selftest import run


def test_output_coverage_native_mutations():
    run("worker_output_coverage_selftest.exe", "output_coverage_selftest")
