from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")


def test_sealed_workers_audit_immediately_after_load_before_symbol_lookup():
    load = SOURCE.index("HMODULE module = LoadLibraryExW(plugin_path.c_str()")
    post_load = SOURCE.index("g_module_audit.post_load = capture_module_audit()", load)
    lookup = SOURCE.index("GetProcAddress(module", load)
    assert load < post_load < lookup
    assert 'plugin_path.parent_path(), L"aexcompat-sealed-"' in SOURCE
    assert 'g_module_audit.required' in SOURCE


def test_module_enumeration_is_bounded_and_incomplete_results_fail_closed():
    assert "constexpr std::size_t kMaxAuditedModules = 512" in SOURCE
    assert "EnumProcessModulesEx(GetCurrentProcess()" in SOURCE
    assert "needed > sizeof(modules)" in SOURCE
    assert "needed % sizeof(HMODULE) != 0" in SOURCE
    assert re.search(r"module_length == 0.*?canonical_path\(module_buffer\.data\(\), module_path\)",
                     SOURCE, re.DOTALL)
    assert 'snapshot.status = snapshot.unknown_count == 0 ? "passed" : "failed"' in SOURCE
    assert SOURCE.count("module_audit_failed") >= 2


def test_only_worker_plugin_root_and_system32_are_allowed():
    assert 'L"aexcompat-trusted-worker-"' in SOURCE
    assert "same_path(module_path, executable)" in SOURCE
    assert "same_path(module_path.parent_path(), plugin_root)" in SOURCE
    assert "same_path(module_path.parent_path(), system32)" in SOURCE
    assert "++snapshot.unknown_count;" in SOURCE
    assert "snapshot.unknown_keys.push_back" in SOURCE


def test_report_exposes_schema_snapshots_counts_and_basenames_not_paths():
    serializer = SOURCE[SOURCE.index("std::string module_audit_snapshot_json"):
                        SOURCE.index("void bump_render_project_timestamp")]
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
    final_audit = SOURCE.rindex("g_module_audit.pre_unload = capture_module_audit()")
    final_free = SOURCE.rindex("FreeLibrary(module)")
    assert final_audit < final_free
    assert 'std::string status{"not_required"}' in SOURCE
    assert ': "not_required")' in SOURCE
    assert SOURCE.count('",\\\"module_audit\\\":" << module_audit_json()') >= 3


def test_all_effectmain_calls_share_the_cumulative_audit_boundary():
    wrapper = SOURCE[SOURCE.index("int32_t audited_effect_call"):
                     SOURCE.index("struct ParamRecord")]
    assert "capture_module_audit_phase();" in wrapper
    assert "module_audit_passed() ? error : 512" in wrapper
    assert "return audited_effect_call(entry, command" in wrapper
    assert "return invoke_entry_seh(entry, command" in wrapper
    assert "#define entry(...) guarded_effect_call(entry, __VA_ARGS__)" in wrapper


def test_observed_modules_are_bounded_deduplicated_and_unknowns_are_sticky():
    accumulator = SOURCE[SOURCE.index("void accumulate_module_audit"):
                         SOURCE.index("std::string module_audit_snapshot_json")]
    assert "target.size() >= kMaxAuditedModules" in accumulator
    assert "keys.size() >= kMaxAuditedModules" in accumulator
    assert "std::find(keys.begin(), keys.end(), key)" in accumulator
    assert "observed_union.unknown_keys.size()" in accumulator
    assert 'observed_union.status =\n      g_module_audit.observed_union.unknown_count == 0 ? "passed" : "failed"' in accumulator


def test_gpu_boundaries_capture_before_begin_selector_setdown_and_end():
    smart = SOURCE[SOURCE.index("SmartResult smart_render_once"):
                   SOURCE.index("void report(", SOURCE.index("SmartResult smart_render_once"))]
    assert smart.count("if (gpu_negotiation) capture_module_audit();") >= 3
    setdown = smart.index('stage:gpu_device_setdown_begin')
    assert smart.rfind("capture_module_audit();", 0, setdown) > smart.rfind(
        "write<void*>(setdown_extra", 0, setdown)
