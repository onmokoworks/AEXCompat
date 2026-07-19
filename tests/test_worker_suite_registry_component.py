from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MAIN = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
HEADER = (ROOT / "minihost" / "src" / "worker_suite_registry.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost" / "src" / "worker_suite_registry.cpp").read_text(
    encoding="utf-8"
)
CMAKE = (ROOT / "minihost" / "CMakeLists.txt").read_text(encoding="utf-8")


def test_registry_is_a_genuine_compiled_owner_and_abi_wrappers_remain_in_main():
    assert CMAKE.count("src/worker_suite_registry.cpp") == 1
    assert '#include "worker_suite_registry.hpp"' in MAIN
    assert "SuiteResolveResult resolve_suite(" in MAIN
    acquire = MAIN[MAIN.index("int32_t __cdecl acquire_suite(") :]
    acquire = acquire[: acquire.index("int32_t __cdecl release_suite(")]
    assert "suite_registry().acquire(" in acquire
    assert "&resolve_suite" in acquire
    assert "g_trace_writer" in acquire
    basic = MAIN[MAIN.index("struct BasicSuite") : MAIN.index("int32_t invoke_sequence_selector")]
    assert "decltype(&acquire_suite) acquire" in basic
    assert "BasicSuite g_basic_suite{&acquire_suite, &release_suite}" in basic


def test_registry_owns_success_reject_unknown_and_release_protocols():
    acquire = SOURCE[SOURCE.index("int32_t SuiteRegistry::acquire") :
                     SOURCE.index("int32_t SuiteRegistry::release")]
    assert "if (!suite) return 4" in acquire
    assert "*suite = nullptr" in acquire
    assert "if (!name || !resolver) return 4" in acquire
    assert "case SuiteResolveResult::acquired" in acquire
    assert "lease_tracker_.acquire(name, version)" in acquire
    assert "suite_acquire(name, version, true)" in acquire
    assert "case SuiteResolveResult::rejected_bad_param" in acquire
    assert "case SuiteResolveResult::not_found" in acquire
    assert "return reject_unknown(name, version, trace_writer)" in acquire
    assert "lease_tracker_.release(name, version)" in SOURCE
    assert "return released ? 0 : 1" in SOURCE


def test_scene_precedence_and_live_tls_conditional_exposure_stay_in_resolver():
    resolver = MAIN[MAIN.index("SuiteResolveResult resolve_suite(") :
                    MAIN.index("int32_t __cdecl acquire_suite(")]
    scene = resolver.index("if (scene_context())")
    first_provider = resolver.index('std::strcmp(name, "AE Plugin Helper Suite")')
    assert scene < first_provider
    assert "SceneSuiteAcquireResult::rejected" in resolver
    assert "return SuiteResolveResult::rejected_bad_param" in resolver
    assert "is_render_worker() && g_loaded_effect_receipt_context.entry" in resolver
    assert "g_aegp_command_roundtrip_mode" in resolver
    assert "g_mask_model_enabled" in resolver
    assert "record_suite_acquire" not in resolver
    assert "reject_suite_acquire" not in resolver


def test_missing_suite_diagnostics_remain_bounded_sanitized_and_fail_closed():
    assert "constexpr std::size_t kMaxMissingSuites = 16" in SOURCE
    assert "constexpr std::size_t kMaxSuiteNameBytes = 96" in SOURCE
    assert "character >= 0x20 && character <= 0x7e" in SOURCE
    assert "if (!valid_name || version <= 0) return" in SOURCE
    assert "missing_suites_.size() < kMaxMissingSuites" in SOURCE
    assert "suite_acquire(safe_name, std::max<int32_t>(version, 0), false)" in SOURCE
    assert '"stage:suite_acquire_failed name="' in SOURCE
    assert "return 1" in SOURCE
