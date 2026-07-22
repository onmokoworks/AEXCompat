from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_sealed_runtime_loader_registration_has_owned_cleanup():
    admission = (ROOT / "minihost/src/worker_runtime_admission.cpp").read_text(encoding="utf-8")
    session = (ROOT / "minihost/src/worker_session.cpp").read_text(encoding="utf-8")
    assert "AddDllDirectory(plugin_path.parent_path().c_str())" in admission
    assert "LOAD_LIBRARY_SEARCH_SYSTEM32 |" in admission
    assert "LOAD_LIBRARY_SEARCH_USER_DIRS" in admission
    assert "RemoveDllDirectory(sealed_directory_cookie_)" in session
    assert "FreeLibrary(module_);\n  module_ = nullptr;\n  if (sealed_directory_cookie_)" in session


def test_delay_load_diagnostic_is_bounded_to_a_safe_dll_basename():
    dispatch = (ROOT / "minihost/src/worker_selector_dispatch.cpp").read_text(encoding="utf-8")
    report = (ROOT / "minihost/src/worker_render_report.cpp").read_text(encoding="utf-8")
    assert "kDelayLoadModuleNotFound" in dispatch
    assert "kMaxDependencyName = 260" in dispatch
    assert "byte == '.' || byte == '_' || byte == '-'" in dispatch
    assert "missing_dependency" in report


def test_generic_delay_load_fixture_covers_a_transitive_dependency():
    cmake = (ROOT / "instruments/pf-delay-load-probe/CMakeLists.txt").read_text(encoding="utf-8")
    probe = (ROOT / "instruments/pf-delay-load-probe/pf_delay_load_probe.cpp").read_text(
        encoding="utf-8"
    )
    dependency = (ROOT / "instruments/pf-delay-load-probe/delay_dependency.cpp").read_text(
        encoding="utf-8"
    )
    assert '"/DELAYLOAD:issue60_delay_dependency.dll"' in cmake
    assert "issue60_delay_dependency" in cmake
    assert "issue60_delay_transitive" in cmake
    assert "case PF_Cmd_SEQUENCE_SETUP" in probe
    assert "issue60_transitive_value()" in dependency
    assert "issue60_delay_load_gate" in cmake
