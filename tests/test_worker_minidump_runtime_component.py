from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.L2_MAIN.read_text(encoding="utf-8")
HEADER = (ROOT / "minihost/src/worker_minidump_runtime.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost/src/worker_minidump_runtime.cpp").read_text(
    encoding="utf-8"
)
SELECTOR = (ROOT / "minihost/src/worker_selector_dispatch.cpp").read_text(
    encoding="utf-8"
)
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")


def test_minidump_writer_is_a_common_worker_runtime_component():
    assert CMAKE.count("src/worker_minidump_runtime.cpp") == 1
    assert '#include "worker_minidump_runtime.hpp"' in MAIN
    assert "g_minidump_dir" not in MAIN
    assert "g_minidump_attempted" not in MAIN
    assert "MiniDumpWriteDump" not in MAIN
    assert "minidump::capture_seh_exception(" in MAIN
    assert "configure_directory(" in MAIN
    assert "SetUnhandledExceptionFilter(" in MAIN


def test_minidump_component_preserves_crash_boundary_contracts():
    assert "bool configure_directory" in HEADER
    assert "LONG WINAPI top_level_crash_filter" in HEADER
    assert 'LoadLibraryExW(L"dbghelp.dll", nullptr,' in SOURCE
    assert "LOAD_LIBRARY_SEARCH_SYSTEM32" in SOURCE
    assert "CREATE_NEW" in SOURCE
    assert "g_minidump_attempted.exchange(true)" in SOURCE
    assert "EXCEPTION_CONTINUE_SEARCH" in SOURCE
    for marker in (
        "dbghelp_unavailable",
        "entry_unavailable",
        "create_failed",
        "write_failed",
        "minidump_written",
    ):
        assert marker in SOURCE


def test_minidump_configuration_remains_opt_in_and_directory_only():
    configure = SOURCE[SOURCE.index("bool configure_directory"):]
    assert "directory.empty()" in configure
    assert "std::filesystem::is_directory(directory, error)" in configure
    assert "g_minidump_dir = directory" in configure
    assert configure.index("is_directory") < configure.index("g_minidump_dir =")


def test_seh_classification_uses_an_explicit_diagnostics_sink():
    assert "struct SehDiagnosticsSink" in HEADER
    assert "uint32_t& code" in HEADER
    assert "uint64_t& address" in HEADER
    assert "std::string& module" in HEADER
    assert "void classify_seh_exception" in SOURCE
    assert "GetModuleHandleExW(" in SOURCE
    assert "GetModuleFileNameW(" in SOURCE
    assert "GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT" in SOURCE
    assert "diagnostics.module.push_back" in SOURCE
    assert "GetModuleHandleExW(" not in MAIN
    assert "GetModuleFileNameW(" not in MAIN


def test_l2_and_selector_filters_delegate_without_owning_classification():
    assert "minidump::capture_seh_exception(" in MAIN
    assert "minidump::classify_seh_exception(" in SELECTOR
    assert "GetModuleHandleExW(" not in SELECTOR
    assert "GetModuleFileNameW(" not in SELECTOR
