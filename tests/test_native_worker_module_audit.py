from pathlib import Path
import re
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.L2_MAIN.read_text(encoding="utf-8")
ENTRY_BOOTSTRAP = (ROOT / "minihost" / "src" / "worker_entry_bootstrap.cpp").read_text(
    encoding="utf-8"
)
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
SMART_DISPATCH = (ROOT / "minihost" / "src" / "worker_smart_dispatch.cpp").read_text(
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


def test_only_worker_plugin_root_system32_and_winsxs_assembly_children_are_allowed():
    assert 'L"aexcompat-trusted-worker-"' in SOURCE
    assert "same_path(module_path, executable)" in SOURCE
    assert "same_path(module_path.parent_path(), plugin_root)" in SOURCE
    assert "same_path(module_path.parent_path(), system32)" in SOURCE
    assert "GetWindowsDirectoryW" in SOURCE
    assert 'L"WinSxS"' in SOURCE
    assert "is_winsxs_module(module_path, winsxs_root)" in SOURCE
    assert "contains_reparse_component" in SOURCE
    assert "++snapshot.unknown_count;" in SOURCE
    assert "snapshot.unknown_keys.push_back" in SOURCE


def test_driverstore_classification_mirrors_the_winsxs_rule():
    """The OS DriverStore (issue #362) classifies like a WinSxS assembly:
    only direct children of System32\\DriverStore\\FileRepository\\<package>,
    with the same reparse-point rejection, and an empty root (a machine
    without the store) admitting nothing rather than failing closed."""
    helper = SOURCE[SOURCE.index("bool is_driverstore_module"):
                    SOURCE.index("std::string audit_basename")]
    assert "if (driverstore_root.empty()) return false;" in helper
    # Only the package directory's direct children, exactly the winsxs shape.
    assert "!module_path.filename().empty()" in helper
    assert "!package.filename().empty()" in helper
    assert "same_path(package.parent_path(), driverstore_root)" in helper
    assert "contains_reparse_component(module_path)" in helper
    # The root derives from the system directory's DriverStore\FileRepository
    # and is cleared when it cannot be trusted.
    assert 'system32 / L"DriverStore" / L"FileRepository"' in SOURCE
    assert "driverstore_root.clear()" in SOURCE
    # Classification order: winsxs, then driverstore, then policy, then
    # unknown — driverstore never widens past the policy check.
    winsxs = SOURCE.index("snapshot.winsxs.push_back")
    driverstore = SOURCE.index("snapshot.driverstore.push_back")
    policy = SOURCE.index("snapshot.policy.push_back")
    unknown = SOURCE.index("snapshot.unknown_keys.push_back", policy)
    assert winsxs < driverstore < policy < unknown
    # The observed union accumulates the new category like every other.
    assert ("append_unique(g_module_audit.observed_union.driverstore, "
            "snapshot.driverstore)") in SOURCE


def test_report_exposes_schema_snapshots_counts_and_basenames_not_paths():
    serializer = SOURCE[SOURCE.index("std::string module_audit_snapshot_json"):]
    assert '"schema\\\":1' in serializer
    assert '"post_load\\\":"' in serializer
    assert '"pre_unload\\\":"' in serializer
    assert '"observed_union\\\":"' in serializer
    assert '"phase_count\\\":"' in serializer
    assert '"unknown_count\\\":"' in serializer
    assert "audit_basename(module_path)" in SOURCE
    assert '"winsxs\\\":"' in serializer
    assert '"unknown\\\":"' in serializer
    # The driverstore field sits between winsxs and policy in every snapshot.
    assert serializer.index('"winsxs\\\":"') < serializer.index(
        '"driverstore\\\":"') < serializer.index('"policy\\\":"')
    assert "audit_basename(std::filesystem::path(key))" in serializer
    assert "module_path.wstring()" not in serializer
    assert "plugin_root.wstring()" not in serializer
    assert "executable.wstring()" not in serializer


def test_pre_unload_audit_precedes_final_free_and_direct_workers_remain_optional():
    final_audit = SESSION.index("audit.pre_unload = capture_module_audit()")
    final_free = SESSION.index("FreeLibrary(module_)")
    assert final_audit < final_free
    assert 'std::string status{"not_required"}' in HEADER
    assert ': "not_required")' in SOURCE
    # One call remains in orchestration; the extracted report owner performs
    # the other terminal-report call.
    assert (MAIN + RENDER_REPORT).count("finish_requested_parameters(") >= 2
    assert '\\\"module_audit\\\"' in RENDER_REPORT


def test_all_effectmain_calls_share_the_cumulative_audit_boundary():
    assert "g_capture_audit();" in DISPATCH
    assert "g_audit_passed() ? error : kAuditFailure" in DISPATCH
    assert "result = audited_effect_call(" in DISPATCH
    assert "set_suite_timeline_selector(previous_suite_selector)" in DISPATCH
    assert "return invoke_entry_seh(entry, command" in DISPATCH
    assert "configure_selector_dispatch_audit(hooks.audit_capture" in ENTRY_BOOTSTRAP
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
    smart = SMART_DISPATCH
    assert smart.count("if (plan.gpu_negotiation) hooks.capture_module_audit();") >= 3
    setdown = smart.index('stage:gpu_device_setdown_begin')
    assert smart.rfind("capture_module_audit();", 0, setdown) > smart.rfind(
        "write<void*>(setdown_extra", 0, setdown)
