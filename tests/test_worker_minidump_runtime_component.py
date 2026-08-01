from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.L2_SOURCE.read_text(encoding="utf-8")
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
    assert "src/worker_minidump_runtime.cpp" in CMAKE
    assert '#include "worker_minidump_runtime.hpp"' in MAIN
    # The minidump globals, DbgHelp use, and the top-level filter installation
    # all live in the component, never in l2_main.
    assert "g_minidump_dir" not in MAIN
    assert "g_minidump_attempted" not in MAIN
    assert "MiniDumpWriteDump" not in MAIN
    assert "SetUnhandledExceptionFilter(" not in MAIN
    assert "minidump::capture_seh_exception(" in MAIN
    # l2_main configures the crash path through the component's opt-in entry
    # point, not by handing it a directory.
    assert "configure_from_inherited_handle(" in MAIN
    assert "configure_directory(" not in MAIN


def test_minidump_component_preserves_crash_boundary_contracts():
    assert "bool configure_from_inherited_handle" in HEADER
    assert "LONG WINAPI top_level_crash_filter" in HEADER
    # The component installs the top-level filter itself, once opt-in succeeds.
    assert "SetUnhandledExceptionFilter(top_level_crash_filter)" in SOURCE
    assert 'LoadLibraryExW(L"dbghelp.dll", nullptr,' in SOURCE
    assert "LOAD_LIBRARY_SEARCH_SYSTEM32" in SOURCE
    # The worker never opens a dump file by path; the broker owns the file.
    assert "CREATE_NEW" not in SOURCE
    assert "CreateFileW(dump_path" not in SOURCE
    assert "g_minidump_attempted.exchange(true, std::memory_order_acq_rel)" in SOURCE
    assert "EXCEPTION_CONTINUE_SEARCH" in SOURCE
    for marker in (
        "dbghelp_unavailable",
        "entry_unavailable",
        "write_failed",
        "handle_invalid",
        "writer_unavailable",
        "writer_timeout",
        "capacity_exceeded",
        "minidump_written",
    ):
        assert marker in SOURCE


def test_minidump_configuration_remains_opt_in_and_handle_only():
    # Opt-in is off unless the broker supplies the inherited handle env, and the
    # worker only ever accepts an authenticated inherited pipe, never a path.
    configure = SOURCE[SOURCE.index("bool configure_from_inherited_handle"):]
    probe = configure.index('GetEnvironmentVariableW(L"AEXCOMPAT_MINIDUMP_HANDLE"')
    off_return = configure.index("return true;")
    # The default-off probe returns before any handle is claimed.
    assert probe < off_return
    assert "inherited_minidump_handle(L\"AEXCOMPAT_MINIDUMP_HANDLE\")" in configure
    assert "inherited_minidump_handle(L\"AEXCOMPAT_MINIDUMP_ACK_HANDLE\")" in configure
    assert "FILE_TYPE_PIPE" in SOURCE
    assert "g_minidump_dir" not in SOURCE


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
    # l2_main may use GetModuleFileNameW for the independent read-only AEX
    # string-table loader; only the minidump SEH classification API must stay
    # inside worker_minidump_runtime.
    assert "load_aex_string_table(module, aex_string_table)" in MAIN


def test_l2_and_selector_filters_delegate_without_owning_classification():
    assert "minidump::capture_seh_exception(" in MAIN
    assert "minidump::classify_seh_exception(" in SELECTOR
    assert "minidump::capture_seh_exception(" not in SELECTOR
