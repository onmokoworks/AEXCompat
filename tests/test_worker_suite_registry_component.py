from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = (source_owners.L2_MAIN.read_text(encoding="utf-8") +
        (source_owners.SRC / "worker_host_suite_wiring.cpp").read_text(encoding="utf-8"))
HEADER = (ROOT / "minihost" / "src" / "worker_suite_registry.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost" / "src" / "worker_suite_registry.cpp").read_text(
    encoding="utf-8"
)
ROUTER = (ROOT / "minihost/src/worker_host_suite_router.cpp").read_text(
    encoding="utf-8"
)
CATALOG = (ROOT / "minihost/src/worker_host_suite_catalog.cpp").read_text(encoding="utf-8")
CMAKE = (ROOT / "minihost" / "CMakeLists.txt").read_text(encoding="utf-8")
NATIVE = (ROOT / "tests" / "native" / "worker_suite_registry_selftest.cpp").read_text(
    encoding="utf-8"
)


def test_registry_is_a_genuine_compiled_owner_and_abi_wrappers_remain_in_main():
    core_sources = CMAKE[CMAKE.index("set(AEXCOMPAT_WORKER_RUNTIME_CORE_SOURCES") :
                         CMAKE.index("add_library(aex_worker_runtime_core")]
    assert core_sources.count("src/worker_suite_registry.cpp") == 1
    assert '#include "worker_suite_registry.hpp"' in MAIN
    assert "SuiteResolveResult resolve_legacy_host_suite(" not in MAIN
    acquire = MAIN[MAIN.rindex("int32_t __cdecl acquire_suite(") :]
    acquire = acquire[: acquire.index("int32_t __cdecl release_suite(")]
    assert "acquire_catalog_suite(" in acquire
    assert "acquire_host_suite(" in CATALOG
    assert "suite_registry().acquire(" in ROUTER
    assert "ProviderCatalog provider_catalog" in CATALOG
    assert "g_trace_writer" in acquire
    # The BasicSuite ABI slab lives in its owner TU.
    render_abi = source_owners.contract_text("l2_render_abi")
    assert "decltype(&acquire_suite) acquire" in render_abi
    assert "BasicSuite g_basic_suite{&acquire_suite, &release_suite}" in render_abi


def test_registry_owns_success_reject_unknown_and_release_protocols():
    acquire = SOURCE[SOURCE.index("int32_t SuiteRegistry::acquire") :
                     SOURCE.index("int32_t SuiteRegistry::release")]
    assert "if (!suite) { record(4); return 4; }" in acquire
    assert "*suite = nullptr" in acquire
    assert "!name || !resolver || !owned_name.readable || !owned_name.terminated" in acquire
    assert "case SuiteResolveResult::acquired" in acquire
    assert "lease_tracker_.acquire(safe_name, version)" in acquire
    assert "suite_acquire(safe_name, version, true)" in acquire
    assert "case SuiteResolveResult::rejected_bad_param" in acquire
    assert "case SuiteResolveResult::not_found" in acquire
    assert "reject_unknown(safe_name, version, trace_writer)" in acquire
    assert "lease_tracker_.release(safe_name, version)" in SOURCE
    assert "return released ? 0 : 1" in SOURCE


def test_scene_precedence_and_live_tls_conditional_exposure_stay_in_resolver():
    resolver = MAIN[MAIN.index("SuiteResolveResult resolve_scene_suite_provider(") :
                    MAIN.rindex("int32_t __cdecl acquire_suite(")]
    scene = resolver.index("if (scene_context())")
    assert "SceneSuiteAcquireResult::rejected" in resolver
    assert "return SuiteResolveResult::rejected_bad_param" in resolver
    assert "is_render_worker() && aexcompat::aegp_layer_render_runtime::active()" in MAIN
    assert "g_aegp_command_roundtrip_mode" in MAIN
    assert "render_options4_provider_available" in MAIN
    assert "render_suite2_provider_available" in MAIN
    assert "mask_suite_provider_available" in MAIN
    assert "return aexcompat::mask_runtime::model_enabled()" in MAIN
    assert "record_suite_acquire" not in resolver
    assert "reject_suite_acquire" not in resolver
    acquire = MAIN[MAIN.index("const StaticSuite component_suites[]") :]
    assert acquire.index("resolve_scene_suite_provider") < CATALOG.index(
        "resolve_static_provider"
    )
    assert "resolve_legacy_host_suite" not in acquire


def test_missing_suite_diagnostics_remain_bounded_sanitized_and_fail_closed():
    assert "constexpr std::size_t kMaxMissingSuites = 16" in SOURCE
    assert "constexpr std::size_t kMaxSuiteNameBytes = 96" in SOURCE
    assert "constexpr std::size_t kMaxTelemetrySuiteNameBytes = 64" in SOURCE
    assert "constexpr int32_t kMaxSuiteVersion = 65535" in SOURCE
    assert "character >= 0x20 && character <= 0x7e" in SOURCE
    assert "!valid_schema_text(name, kMaxTelemetrySuiteNameBytes, true)" in SOURCE
    assert "missing_suites_.size() >= kMaxMissingSuites" in SOURCE
    assert "missing_suites_truncated_ = true" in SOURCE
    assert '\\"missing_suites_truncated\\":' in SOURCE
    assert "suite_acquire(safe_name, std::max<int32_t>(version, 0), false)" in SOURCE
    assert '"stage:suite_acquire_failed name="' in SOURCE
    assert "return 1" in SOURCE


def test_acquired_suite_unsupported_slots_are_bounded_and_identified():
    assert "enum class UnsupportedSuiteId" in HEADER
    assert "unsupported_suite_slot()" in HEADER
    assert "std::make_index_sequence<SlotCount>" in HEADER
    assert "constexpr std::size_t kMaxUnsupportedSuiteCalls = 32" in SOURCE
    assert "call.suite == suite && call.slot == slot" in SOURCE
    assert "unsupported_suite_calls_.size() >= kMaxUnsupportedSuiteCalls" in SOURCE
    assert "unsupported_suite_calls_truncated_ = true" in SOURCE
    assert '"stage:suite_slot_unsupported suite="' in SOURCE
    assert "unsupported_suite_calls_report_json" in SOURCE


def test_raw_plugin_name_is_copied_once_before_resolver_lease_or_trace_use():
    boundary = SOURCE[SOURCE.index("SuiteNameCopy copy_bounded_suite_name") :
                      SOURCE.index("}  // namespace")]
    assert "copy.length < kMaxSuiteNameBytes" in boundary
    assert "__try" in boundary
    assert "__except (EXCEPTION_EXECUTE_HANDLER)" in boundary
    acquire = SOURCE[SOURCE.index("int32_t SuiteRegistry::acquire") :
                     SOURCE.index("int32_t SuiteRegistry::release")]
    copied = acquire.index("owned_name = copy_bounded_suite_name(name)")
    resolved = acquire.index("resolver(resolver_context, safe_name")
    tracked = acquire.index("lease_tracker_.acquire(safe_name")
    traced = acquire.index("suite_acquire(safe_name")
    assert copied < resolved < tracked < traced
    assert "resolver(resolver_context, name" not in acquire
    assert "lease_tracker_.acquire(name" not in acquire
    release = SOURCE[SOURCE.index("int32_t SuiteRegistry::release") :
                     SOURCE.index("std::string SuiteRegistry::safe_missing_name")]
    assert "owned_name = copy_bounded_suite_name(name)" in release
    assert "lease_tracker_.release(safe_name" in release


def test_native_bounds_fixture_covers_guard_page_overlong_and_invalid_pointer():
    assert "worker_suite_registry_selftest" in CMAKE
    assert "PAGE_NOACCESS" in NATIVE
    assert "page_size - 96" in NATIVE
    assert "std::array<char, 98> overlong" in NATIVE
    assert "static_cast<uintptr_t>(1)" in NATIVE
    assert "g_resolver_calls == calls_before" in NATIVE
    assert "UnsupportedSuiteId::aegp_comp_21" in NATIVE
    assert "unsupported_suite_calls_report_json" in NATIVE
    assert '"maximum_telemetry_name_bytes\\":64' in NATIVE
    assert '"maximum_version\\":65535' in NATIVE
    assert '"maximum_timeline_events\\":512' in NATIVE
