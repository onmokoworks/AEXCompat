"""Behavioral tests for the private AE 2026 ``AEFX ACE Suite`` version 1.

Issue #776: ``Photo Filter.aex`` refused every frame with error 516 ("Not able
to acquire AEFX Suite") because the host published no such suite. Observation
of its RENDER path (the worker's suite-call slot probe, then the caller's own
disassembly) identified slot 0, which widens packed 8-bit pixels into AE's
0..32768 16-bit range, and slot 2, which narrows them back. Every other slot
of the published table, slot 1 included, is a diagnosed unsupported stub,
since the observation bounds the real table from below only.

The quality argument is one byte: the caller writes it with ``sete dl`` and
leaves the rest of the register alone, so a word-sized parameter would carry
whatever was there before.
"""

from pathlib import Path

from _native_selftest import worker_self_test


ROOT = Path(__file__).resolve().parents[1]


def test_native_aefx_ace_suite1_passes_all_three_workers() -> None:
    expected = {"aefx_ace_suite1": "passed"}
    for report in worker_self_test("--self-test-aefx-ace-suite1").values():
        assert report == expected
