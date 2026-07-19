from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
MAIN = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
RENDER_REPORT = (ROOT / "minihost" / "src" / "worker_render_report.cpp").read_text(
    encoding="utf-8"
)
ADMISSION = (ROOT / "minihost" / "src" / "worker_runtime_admission.cpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost" / "src" / "runtime_module_audit.cpp").read_text(
    encoding="utf-8"
)
HEADER = (ROOT / "minihost" / "src" / "runtime_module_audit.hpp").read_text(
    encoding="utf-8"
)
DISPATCH = (ROOT / "minihost" / "src" / "worker_selector_dispatch.cpp").read_text(
    encoding="utf-8"
)
SESSION = (ROOT / "minihost" / "src" / "worker_session.cpp").read_text(
    encoding="utf-8"
)


def test_sealed_workers_audit_immediately_after_load_before_symbol_lookup():
    load = ADMISSION.index("HMODULE module = LoadLibraryExW(plugin_path.c_str()")
    post_load = ADMISSION.index("audit.post_load = capture_module_audit()", load)
    assert load < post_load
    assert 'L"aexcompat-sealed-"' in ADMISSION
    assert "RuntimeContext runtime_context" in MAIN
    assert "GetProcAddress(module" in MAIN


def test_module_enumeration_is_bounded_and_incomplete_results_fail_closed():
    assert "constexpr std::size_t kMaxAuditedModules = 512" in SOURCE
    assert "EnumProcessModulesEx(GetCurrentProcess()" in SOURCE
    assert "needed > sizeof(modules)" in SOURCE
    assert "needed % sizeof(HMODULE) != 0" in SOURCE
    assert re.search(r"module_length == 0.*?canonical_path\(module_buffer\.data\(\), module_path\)",
                     SOURCE, re.DOTALL)
    assert 'snapshot.status = snapshot.unknown_count == 0 ? "passed" : "failed"' in SOURCE
    # Admission rejects the initial snapshot before ownership transfer. The
    # session owns every terminal snapshot and later cleanup path.
    assert '\\"status\\":\\"module_audit_failed\\"' in ADMISSION
    assert "FreeLibrary(module);" in ADMISSION
    assert "module_audit_failed" in SESSION


def test_only_worker_plugin_root_and_system32_are_allowed():
    assert 'L"aexcompat-trusted-worker-"' in SOURCE
    assert "same_path(module_path, executable)" in SOURCE
    assert "same_path(module_path.parent_path(), plugin_root)" in SOURCE
    assert "same_path(module_path.parent_path(), system32)" in SOURCE
    assert "++snapshot.unknown_count;" in SOURCE
    assert "snapshot.unknown_keys.push_back" in SOURCE


def test_report_exposes_schema_snapshots_counts_and_basenames_not_paths():
    serializer = SOURCE[SOURCE.index("std::string module_audit_snapshot_json"):]
    assert '"schema\\\":1' in serializer
    assert '"post_load\\\":"' in serializer
    assert '"pre_unload\\\":"' in serializer
    assert '"observed_union\\\":"' in serializer
    assert '"phase_count\\\":"' in serializer
    assert '"unknown_count\\\":"' in serializer
    assert "audit_basename(module_path)" in SOURCE
    assert "module_path.wstring()" not in serializer
    assert "plugin_root.wstring()" not in serializer
    assert "executable.wstring()" not in serializer


def test_pre_unload_audit_precedes_final_free_and_direct_workers_remain_optional():
    final_audit = SESSION.index("audit.pre_unload = capture_module_audit()")
    final_free = SESSION.index("FreeLibrary(module_)")
    assert final_audit < final_free
    assert 'std::string status{"not_required"}' in HEADER
    assert ': "not_required")' in SOURCE
    assert MAIN.count("finish_requested_parameters(report_snapshot") == 2
    assert '\\\"module_audit\\\"' in RENDER_REPORT


def test_all_effectmain_calls_share_the_cumulative_audit_boundary():
    assert "g_capture_audit();" in DISPATCH
    assert "g_audit_passed() ? error : kAuditFailure" in DISPATCH
    assert "return audited_effect_call(entry, command" in DISPATCH
    assert "return invoke_entry_seh(entry, command" in DISPATCH
    assert "configure_selector_dispatch_audit(&capture_module_audit_phase" in MAIN
    assert "#define entry(...) guarded_effect_call(entry, __VA_ARGS__)" in MAIN


def test_observed_modules_are_bounded_deduplicated_and_unknowns_are_sticky():
    accumulator = SOURCE[SOURCE.index("void accumulate_module_audit"):
                         SOURCE.index("std::string module_audit_snapshot_json")]
    assert "target.size() >= kMaxAuditedModules" in accumulator
    assert "keys.size() >= kMaxAuditedModules" in accumulator
    assert "std::find(keys.begin(), keys.end(), key)" in accumulator
    assert "observed_union.unknown_keys.size()" in accumulator
    assert 'observed_union.status =\n      g_module_audit.observed_union.unknown_count == 0 ? "passed" : "failed"' in accumulator


def test_gpu_boundaries_capture_before_begin_selector_setdown_and_end():
    smart = MAIN[
        MAIN.index("SmartResult smart_render_runtime"):
        MAIN.index("int smart_render_guarded_effect_main")
    ]
    assert smart.count("if (gpu_negotiation) capture_module_audit();") >= 3
    setdown = smart.index('stage:gpu_device_setdown_begin')
    assert smart.rfind("capture_module_audit();", 0, setdown) > smart.rfind(
        "write<void*>(setdown_extra", 0, setdown)
