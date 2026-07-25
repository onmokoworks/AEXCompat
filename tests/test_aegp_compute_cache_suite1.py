import os
import re
import subprocess
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost/src/worker_aegp_compute_cache.hpp"
PROVIDER = ROOT / "minihost/src/worker_aegp_compute_cache.cpp"
WIRING = ROOT / "minihost/src/worker_host_suite_wiring.cpp"
REPORT_HEADER = ROOT / "minihost/src/worker_report.hpp"
REPORT_SOURCE = ROOT / "minihost/src/worker_report.cpp"
L2_MAIN = ROOT / "minihost/src/l2_main.cpp"
BROKER = ROOT / "broker/crates/broker/src/image_render.rs"
DISCOVER_SWEEP = ROOT / "bridges/aviutl2-multifilter/examples/discover_sweep.rs"
PROBE_HEADER = ROOT / "minihost/src/worker_suite_call_slot_probe.hpp"
PROBE_SOURCE = ROOT / "minihost/src/worker_suite_call_slot_probe.cpp"
CMAKE = ROOT / "minihost/CMakeLists.txt"


def _sdk_header(name: str) -> Path:
    sdk_root = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
    if not sdk_root:
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    header = Path(sdk_root) / "Examples" / "Headers" / name
    if not header.is_file():
        pytest.skip(f"After Effects SDK header is unavailable: {header}")
    return header


def _selftest() -> Path:
    configured = os.environ.get("AEXCOMPAT_COMPUTE_CACHE_SELFTEST")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-build/Release/worker_aegp_compute_cache_selftest.exe",
        ROOT / "target/minihost-build/worker_aegp_compute_cache_selftest.exe",
    ]
    return next(
        (candidate for candidate in candidates if candidate and candidate.is_file()),
        candidates[-1],
    )


def test_compute_cache_sdk_header_freezes_public_v1_contract():
    sdk = _sdk_header("AE_ComputeCacheSuite.h").read_text(encoding="utf-8")
    errors = _sdk_header("A.h").read_text(encoding="utf-8")
    assert 'kAEGPComputeCacheSuite' in sdk
    assert '"AEGP Compute Cache"' in sdk
    assert "kAEGPComputeCacheSuiteVersion1" in sdk
    assert "typedef const char *AEGP_CCComputeClassIdP;" in sdk
    assert "A_long bytes[4];" in sdk
    for callback in (
        "generate_key",
        "compute",
        "approx_size_value",
        "delete_compute_value",
    ):
        assert callback in sdk
    slots = (
        "AEGP_ClassRegister",
        "AEGP_ClassUnregister",
        "AEGP_ComputeIfNeededAndCheckout",
        "AEGP_CheckoutCached",
        "AEGP_GetReceiptComputeValue",
        "AEGP_CheckinComputeReceipt",
    )
    suite = sdk[sdk.index("typedef struct AEGP_ComputeCacheSuite1"):]
    positions = [
        re.search(rf"\(\*{slot}\)\s*\(", suite).start()
        for slot in slots
    ]
    assert positions == sorted(positions)
    assert "bool" in sdk[sdk.index("AEGP_ComputeIfNeededAndCheckout"):
                         sdk.index("AEGP_CheckoutCached")]
    assert "A_Err_NOT_IN_CACHE_OR_COMPUTE_PENDING" in errors


def test_compute_cache_provider_has_exact_six_slot_abi_and_bounds():
    header = HEADER.read_text(encoding="utf-8")
    provider = PROVIDER.read_text(encoding="utf-8")
    assert "using AEGP_CCComputeClassIdP = const char*;" in header
    assert "using AEGP_CCComputeOptionsRefconP = void*;" in header
    assert "using AEGP_CCComputeValueRefconP = void*;" in header
    assert "using AEGP_CCCheckoutReceiptP = void*;" in header
    assert "A_long bytes[4];" in header
    assert "bool," in header
    assert "sizeof(AEGP_ComputeCacheSuite1) == 6 * sizeof(void*)" in header
    for slot in range(6):
        assert f"{slot} * sizeof(void*)" in header or (
            slot == 0
            and "offsetof(AEGP_ComputeCacheSuite1, AEGP_ClassRegister) == 0"
            in header
        )
    assert "kErrNotInCacheOrComputePending = 22" in header
    assert "kMaxClassIdBytes = 1024" in header
    assert "kMaxClasses = 128" in header
    assert "kMaxEntries = 4096" in header
    assert "kMaxActiveReceipts = 8192" in header
    assert "kMaxReceiptTokens = 65536" in header
    assert "kMaxValueBytes = 256u * 1024u * 1024u" in header
    assert "kMaxTotalValueBytes = 512u * 1024u * 1024u" in header
    assert "kMaxTelemetryRecords = 128" in header
    assert "std::condition_variable" in provider
    assert "entry->changed.wait(lock)" in provider
    assert "invoke_generate_key(owner->callbacks.generate_key" in provider
    assert "invoke_compute(owner->callbacks.compute" in provider
    assert "invoke_delete(owner->callbacks.delete_compute_value" in provider


def test_compute_cache_provider_is_registered_and_probe_does_not_intercept():
    wiring = WIRING.read_text(encoding="utf-8")
    probe = (
        PROBE_HEADER.read_text(encoding="utf-8")
        + PROBE_SOURCE.read_text(encoding="utf-8")
    )
    assert "compute_cache::kSuiteName" in wiring
    assert "compute_cache::kSuiteVersion1" in wiring
    assert "compute_cache::provide_suite1" in wiring
    assert "AEGP Compute Cache" not in probe
    assert "aegp_compute_cache_probe" not in probe


def test_compute_cache_report_and_broker_wiring_are_bounded_and_exact_key():
    report_header = REPORT_HEADER.read_text(encoding="utf-8")
    report_source = REPORT_SOURCE.read_text(encoding="utf-8")
    l2_main = L2_MAIN.read_text(encoding="utf-8")
    broker = BROKER.read_text(encoding="utf-8")
    discover = DISCOVER_SWEEP.read_text(encoding="utf-8")
    assert "compute_cache_timeline_json" in report_header
    assert "c.compute_cache_timeline_json" in report_source
    assert "compute_cache::telemetry_report_json()" in l2_main
    assert "MAX_COMPUTE_CACHE_TIMELINE_RECORDS: usize = 128" in broker
    assert "fn propagate_compute_cache_timeline" in broker
    assert "propagate_compute_cache_timeline(&mut diagnostics, report);" in broker
    assert (
        "propagate_compute_cache_timeline(&mut diagnostics, &final_report);"
        in broker
    )
    assert 'worker_report.get("compute_cache_timeline")' in broker
    assert 'diagnostics["compute_cache_timeline"]' in broker
    assert 'record.len() == 7' in broker
    assert '.filter(|slot| *slot <= 5)' in broker
    assert '"compute_cache_timeline",' in discover
    assert "attach_inspection_diagnostics(&mut extra, &diagnostics)" in discover
    assert "attach_inspection_diagnostics(&mut loaded_extra, &diagnostics)" in discover


def test_compute_cache_native_selftest_target_and_runtime():
    cmake = CMAKE.read_text(encoding="utf-8")
    test_source = (
        ROOT / "tests/native/worker_aegp_compute_cache_selftest.cpp"
    ).read_text(encoding="utf-8")
    assert "add_executable(worker_aegp_compute_cache_selftest" in cmake
    assert "worker_aegp_compute_cache_selftest.cpp" in cmake
    assert "/UNDEBUG" in cmake
    for slot in (
        "AEGP_ClassRegister",
        "AEGP_ClassUnregister",
        "AEGP_ComputeIfNeededAndCheckout",
        "AEGP_CheckoutCached",
        "AEGP_GetReceiptComputeValue",
        "AEGP_CheckinComputeReceipt",
    ):
        assert slot in test_source
    selftest = _selftest()
    assert selftest.is_file(), "build worker_aegp_compute_cache_selftest first"
    result = subprocess.run(
        [str(selftest)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
