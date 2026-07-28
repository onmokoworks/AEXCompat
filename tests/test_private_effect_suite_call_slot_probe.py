from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
HEADER = (ROOT / "minihost/src/worker_suite_call_slot_probe.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost/src/worker_suite_call_slot_probe.cpp").read_text(
    encoding="utf-8"
)
WIRING = (ROOT / "minihost/src/worker_host_suite_wiring.cpp").read_text(
    encoding="utf-8"
)
SELECTOR = (ROOT / "minihost/src/worker_selector_dispatch.cpp").read_text(
    encoding="utf-8"
)
NATIVE = (ROOT / "tests/native/worker_suite_registry_selftest.cpp").read_text(
    encoding="utf-8"
)
BROKER = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")


def test_private_effect_v3_v5_probe_is_explicit_bounded_and_not_a_provider():
    assert 'kPrivateEffectSuiteName[] = "PF AE Private Effect Suite"' in HEADER
    assert "kPrivateEffectSuiteVersion3 = 3" in HEADER
    assert "kPrivateEffectSuiteVersion5 = 5" in HEADER
    assert "kProbeSlotCount = 32" in HEADER
    assert "kMaxProbeTargets = 8" in HEADER
    assert "not a suite implementation" in HEADER
    assert 'L"AEXCOMPAT_SUITE_CALL_SLOT_PROBE"' in SOURCE
    assert 'L"PF AE Private Effect Suite@3"' in SOURCE
    assert 'L"PF AE Private Effect Suite@5"' in SOURCE
    assert "value[end] != L';'" in SOURCE
    assert "configuration.target_count >= kMaxProbeTargets" in SOURCE
    assert "private_effect_probe3_available" in WIRING
    assert "private_effect_probe5_available" in WIRING
    assert "provide_private_effect_probe3" in WIRING
    assert "provide_private_effect_probe5" in WIRING
    assert "3D Camera Tracker" not in HEADER + SOURCE + WIRING


def test_every_slot_has_a_distinct_noncontinuable_identity_trap():
    assert "identifying_trap<Target, Slots>" in SOURCE
    assert "std::make_index_sequence<kProbeSlotCount>" in SOURCE
    assert "kProbeExceptionTargetStride" in SOURCE
    assert "RaiseException(exception_code, EXCEPTION_NONCONTINUABLE" in SOURCE
    assert "call.call_count" in SOURCE
    assert '\\"registers\\"' in SOURCE
    assert '\\"stack\\"' in SOURCE
    assert '\\"argument_word_count\\"' in SOURCE
    assert '\\"nonzero_word_count\\"' in SOURCE
    assert '\\"caller_rva\\"' in SOURCE
    assert "call.arguments" not in SOURCE
    assert "static_cast<ULONG_PTR>(rcx)" not in SOURCE


def test_selector_boundary_contains_probe_seh_and_broker_keeps_failure_evidence():
    assert "__try" in SELECTOR
    assert "__except(capture_seh_exception(GetExceptionInformation()))" in SELECTOR
    assert "result = kAuditFailure" in SELECTOR
    assert '"suite_call_slot_probe": null' in BROKER
    assert "fn propagate_suite_call_slot_probe(" in BROKER
    assert "MAX_SUITE_CALL_SLOT_PROBE_TARGETS" in BROKER
    assert 'probe.get("targets")' in BROKER
    assert '"configuration_truncated"' in BROKER
    assert BROKER.count(
        "propagate_suite_call_slot_probe(&mut diagnostics"
    ) >= 2


def test_native_synthetic_caller_pins_slot_shape_without_raw_values():
    assert "invoke_probe_slot" in NATIVE
    assert "slots[7] != slots[8]" in NATIVE
    assert "slots[9] != slots[10]" in NATIVE
    assert "kProbeExceptionBase + 7" in NATIVE
    assert "kProbeExceptionBase + kProbeExceptionTargetStride + 9" in NATIVE
    assert '\\"rcx\\":\\"nonzero\\"' in NATIVE
    assert '\\"stack\\":[\\"nonzero\\"' in NATIVE
    for value in (
        "0x0000000000000011",
        "0x0000000000000022",
        "0x0000000000000033",
        "0x0000000000000044",
        "0x0000000000000055",
        "0x0000000000000066",
        "0x0000000000000077",
        "0x0000000000000088",
    ):
        assert value in NATIVE
        assert f'find("\\"{value}\\"") == std::string::npos' in NATIVE
