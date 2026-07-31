from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.contract_text("worker_runtime_admission")
HEADER = (ROOT / "minihost" / "src" / "worker_runtime_admission.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost" / "src" / "worker_runtime_admission.cpp").read_text(
    encoding="utf-8"
)
CMAKE = (ROOT / "minihost" / "CMakeLists.txt").read_text(encoding="utf-8")


def test_admission_is_a_common_worker_runtime_component_with_explicit_boundary():
    assert CMAKE.count("src/worker_runtime_admission.cpp") == 1
    assert "struct RuntimeHostHooks" in HEADER
    assert "struct RuntimeContext" in HEADER
    assert "RuntimeFileHash hash_file" in HEADER
    assert "RuntimeStdoutRedirect redirect_native_stdout" in HEADER
    assert "admit_worker_entry(" in MAIN
    assert "LoadLibraryExW(plugin_path.c_str()" not in MAIN


def test_gpu_preflight_uses_common_admission_for_plugin_load():
    gpu = MAIN[MAIN.index("--gpu-module-report-v1") :]
    assert "prepare_runtime_request(" in gpu
    assert "admit_runtime(" in gpu
    assert "LoadLibraryExW(plugin_path.c_str()" not in gpu


def test_admission_preserves_ordered_fail_closed_identity_and_audit_gates():
    hash_gate = SOURCE.index("hooks.hash_file(request.plugin_argument")
    manifest_gate = SOURCE.index("parse_runtime_module_authorization(plugin_path,")
    preflight = SOURCE.index("is_aegp_candidate_without_execution(plugin_path)")
    load = SOURCE.index("LoadLibraryExW(plugin_path.c_str()")
    audit = SOURCE.index("audit.post_load = capture_module_audit()")
    stdout = SOURCE.index("hooks.redirect_native_stdout()")
    assert hash_gate < manifest_gate < preflight < load < audit < stdout
    assert "return 10" in SOURCE
    assert "return 15" in SOURCE
    assert "return 14" in SOURCE
    assert "return 13" in SOURCE


def test_pf_admission_rejects_aegp_before_executable_load():
    preflight = SOURCE[SOURCE.index("bool pipl_resource_is_aegp"):
                      SOURCE.index("int admit_runtime")]
    assert "DONT_RESOLVE_DLL_REFERENCES" in preflight
    assert "pipl_resource_is_aegp" in preflight
    assert 'EnumResourceNamesW(preflight_module, L"PiPL"' in preflight
    assert '"MIB8", 4' in preflight
    assert '"dnik", 4' in preflight
    assert '"xgEA", 4' in preflight
    assert "FreeLibrary(preflight_module)" in preflight
    admission = SOURCE[SOURCE.index("if (!request.allow_aegp_plugin"):
                        SOURCE.index("if (!SetDefaultDllDirectories")]
    assert "!request.allow_aegp_plugin" in admission
    assert 'plugin_kind:aegp_candidate\\n' in admission
    assert "return 12" in admission


def test_only_explicit_aegp_modes_opt_in_to_aegp_admission():
    assert "bool allow_aegp_plugin{}" in HEADER
    assert "runtime_request.allow_aegp_plugin = g_aegp_init_mode;" in MAIN


def test_admission_keeps_runtime_path_data_out_of_serialized_diagnostics():
    assert "plugin_path.wstring" not in SOURCE[SOURCE.index("int admit_runtime"):]
    assert "module_audit_failed" in SOURCE
    assert "module_audit_json()" in SOURCE
