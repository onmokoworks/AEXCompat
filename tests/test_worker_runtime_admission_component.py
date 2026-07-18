from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MAIN = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
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
    assert "admit_runtime(runtime_hooks, runtime_request, runtime_context)" in MAIN
    assert "LoadLibraryExW(plugin_path.c_str()" not in MAIN


def test_admission_preserves_ordered_fail_closed_identity_and_audit_gates():
    hash_gate = SOURCE.index("hooks.hash_file(request.plugin_argument")
    manifest_gate = SOURCE.index("parse_runtime_module_authorization(plugin_path,")
    load = SOURCE.index("LoadLibraryExW(plugin_path.c_str()")
    audit = SOURCE.index("audit.post_load = capture_module_audit()")
    stdout = SOURCE.index("hooks.redirect_native_stdout()")
    assert hash_gate < manifest_gate < load < audit < stdout
    assert "return 10" in SOURCE
    assert "return 15" in SOURCE
    assert "return 14" in SOURCE
    assert "return 13" in SOURCE


def test_admission_keeps_runtime_path_data_out_of_serialized_diagnostics():
    assert "plugin_path.wstring" not in SOURCE[SOURCE.index("int admit_runtime"):]
    assert "module_audit_failed" in SOURCE
    assert "module_audit_json()" in SOURCE
