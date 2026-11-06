"""Behavioral test for the host's ``AEGP Persistent Data Suite`` version 3.

Issue #881: ``DeepGlow2.aex`` acquires the suite in SEQUENCE_SETUP and, when
the acquire fails, returns ``PF_Err_INTERNAL_STRUCT_DAMAGED`` (512) and
abandons the setup. The worker reports that as the reserved session error -47
and the broker invalidates the session, so the plug-in never renders a frame.

The native self-test drives the published table the way a plug-in does: the
blob handle and its refusal of a foreign one, the documented default
write-through on a missing key, the documented buffer-too-small answer from
``AEGP_GetString``, the "stored value is not this big, so the default is used"
answer from ``AEGP_GetData``, the caller-owned handle from
``AEGP_GetDataHandle`` and its NULL/error3 for an empty value (including the
default-write side effect), zero-filled string tails, bounded output guards,
prefs UTF-16 termination without a trailing separator, section/key enumeration,
and deletion emptying a section out of the walk. Zero-capacity string refusal
is a defined host safety policy for an AE-crash input, not an AE return value.
"""

from pathlib import Path

from _native_selftest import worker_self_test


ROOT = Path(__file__).resolve().parents[1]


def test_native_aegp_persistent_data_suite3_passes_all_three_workers() -> None:
    expected = {"aegp_persistent_data_suite3": "passed"}
    for report in worker_self_test("--self-test-aegp-persistent-data-suite3").values():
        assert report == expected
