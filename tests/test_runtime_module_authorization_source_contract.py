from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MAIN = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
ADMISSION = (ROOT / "minihost" / "src" / "worker_runtime_admission.cpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost" / "src" / "runtime_module_audit.cpp").read_text(
    encoding="utf-8"
)


def test_manifest_is_strictly_parsed_before_plugin_load():
    parser = SOURCE[SOURCE.index("bool parse_runtime_module_authorization"):]
    for marker in (
        "'A','E','X','R','M','A','1',0", "purpose != 1", "backend < 1 || backend > 4",
        "expiry <= now", "nonzero_session", "count > 128", "path_units > 32767",
        "declared_size == 0", "offset == bytes.size()", "paths.insert(key).second",
        "actual_size != declared_size", "actual_hash != declared_hash",
    ):
        assert marker in parser
    parse_call = ADMISSION.index("parse_runtime_module_authorization(plugin_path,")
    load = ADMISSION.index("HMODULE module = LoadLibraryExW(plugin_path.c_str()", parse_call)
    assert parse_call < load


def test_manifest_is_confined_but_policy_entries_are_exact_absolute_paths():
    assert "manifest_name.has_parent_path()" in SOURCE
    assert "same_path(manifest_path.parent_path(), plugin_root)" in SOURCE
    assert "!requested.is_absolute()" in SOURCE
    assert 'requested_text.rfind(L"\\\\\\\\?\\\\", 0)' in SOURCE
    assert "!same_path(normalized_requested.lexically_normal(), canonical)" in SOURCE
    assert "basenames.insert(basename).second" in SOURCE


def test_only_normally_unknown_modules_can_use_exact_policy_identity():
    audit = SOURCE[SOURCE.index("ModuleAuditSnapshot audit_loaded_modules"):
                   SOURCE.index("void accumulate_module_audit")]
    policy = audit.index("authorized_runtime_module(module_path)")
    assert audit.index("same_path(module_path, executable)") < policy
    assert audit.index("same_path(module_path.parent_path(), system32)") < policy
    assert "size == found->size" in SOURCE
    assert "digest == found->sha256" in SOURCE
    assert "snapshot.policy.push_back(basename)" in audit
    assert '\"policy\\\":" << names(snapshot.policy)' in SOURCE


def test_optional_argument_is_l2_params_only_and_exactly_positioned():
    assert "(argc == 4 || argc == 6)" in MAIN
    assert 'std::wstring(argv[4]) == L"--runtime-module-authorization-v1"' in MAIN
    assert "params_only_mode && argc == 6 && !runtime_module_authorization_mode" in MAIN


def test_hash_dependency_is_configured_before_authorization_and_fails_closed():
    configure = MAIN.index("configure_runtime_module_hash(&sha256)")
    admission = MAIN.index("admit_runtime(runtime_hooks, runtime_request, runtime_context)")
    assert configure < admission
    assert "RuntimeHostHooks runtime_hooks{&sha256" in MAIN
    assert "if (!g_file_sha256 || manifest_name.empty()" in SOURCE
    assert "found == g_authorized_runtime_modules.end() || !g_file_sha256" in SOURCE
