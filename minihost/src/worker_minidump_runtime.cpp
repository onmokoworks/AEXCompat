#include "worker_minidump_runtime.hpp"

// MiniDumpWriteDump types only; dbghelp.dll is loaded from System32 on crash.
#include <DbgHelp.h>

#include <atomic>
#include <array>
#include <cstdio>
#include <cwchar>

namespace aexcompat::worker_runtime::minidump {
namespace {

// Opt-in crash minidumps (issue #18): broker-validated directory passed via
// --minidump-v1. Default off; one create-new dump per process; local only.
std::filesystem::path g_minidump_dir;

// A plug-in can crash several of its own threads at once. Admit only the first
// writer, including failures, so crash handling cannot become a retry loop.
std::atomic<bool> g_minidump_attempted{false};

}  // namespace

bool configure_directory(const std::filesystem::path& directory) {
  std::error_code error;
  if (directory.empty() || !std::filesystem::is_directory(directory, error))
    return false;
  g_minidump_dir = directory;
  return true;
}

std::filesystem::path current_process_dump_path() {
  if (g_minidump_dir.empty()) return {};
  wchar_t name[64]{};
  std::swprintf(name, std::size(name), L"crash-%lu.dmp",
                GetCurrentProcessId());
  return g_minidump_dir / name;
}

bool attempted() { return g_minidump_attempted.load(); }

void classify_seh_exception(EXCEPTION_POINTERS* information,
                            SehDiagnosticsSink diagnostics) {
  diagnostics.code = information && information->ExceptionRecord
      ? information->ExceptionRecord->ExceptionCode : 0;
  const void* address = information && information->ExceptionRecord
      ? information->ExceptionRecord->ExceptionAddress : nullptr;
  diagnostics.address = reinterpret_cast<uint64_t>(address);
  diagnostics.module.clear();
  HMODULE module{};
  if (address && GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
          GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
          reinterpret_cast<LPCWSTR>(address), &module)) {
    std::array<wchar_t, MAX_PATH> path{};
    if (GetModuleFileNameW(module, path.data(),
                           static_cast<DWORD>(path.size())) > 0) {
      const std::wstring filename =
          std::filesystem::path(path.data()).filename().wstring();
      for (wchar_t ch : filename)
        diagnostics.module.push_back(ch >= 0x20 && ch <= 0x7e
            ? static_cast<char>(ch) : '?');
    }
  }
}

int capture_seh_exception(EXCEPTION_POINTERS* information,
                          SehDiagnosticsSink diagnostics) {
  write_crash_minidump(information);
  classify_seh_exception(information, diagnostics);
  return EXCEPTION_EXECUTE_HANDLER;
}

void write_crash_minidump(EXCEPTION_POINTERS* information) {
  if (g_minidump_dir.empty() || !information) return;
  if (g_minidump_attempted.exchange(true)) return;
  HMODULE dbghelp = LoadLibraryExW(L"dbghelp.dll", nullptr,
                                   LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!dbghelp) {
    std::fprintf(stderr, "stage:minidump_failed reason=dbghelp_unavailable\n");
    return;
  }
  using MiniDumpWriteDumpFn = BOOL(WINAPI*)(
      HANDLE, DWORD, HANDLE, MINIDUMP_TYPE,
      PMINIDUMP_EXCEPTION_INFORMATION, void*, void*);
  const auto write_dump = reinterpret_cast<MiniDumpWriteDumpFn>(
      GetProcAddress(dbghelp, "MiniDumpWriteDump"));
  if (!write_dump) {
    std::fprintf(stderr, "stage:minidump_failed reason=entry_unavailable\n");
    return;
  }
  const std::filesystem::path dump_path = current_process_dump_path();
  // CREATE_NEW never overwrites an existing dump, even after process-id reuse.
  HANDLE file = CreateFileW(dump_path.c_str(), GENERIC_WRITE, 0, nullptr,
                            CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) {
    std::fprintf(stderr, "stage:minidump_failed reason=create_failed code=%lu\n",
                 GetLastError());
    return;
  }
  MINIDUMP_EXCEPTION_INFORMATION exception_info{};
  exception_info.ThreadId = GetCurrentThreadId();
  exception_info.ExceptionPointers = information;
  exception_info.ClientPointers = FALSE;
  const BOOL written = write_dump(GetCurrentProcess(), GetCurrentProcessId(),
                                  file, MiniDumpNormal, &exception_info,
                                  nullptr, nullptr);
  LARGE_INTEGER size{};
  GetFileSizeEx(file, &size);
  CloseHandle(file);
  if (written) {
    std::fprintf(stderr, "stage:minidump_written name=crash-%lu.dmp bytes=%lld\n",
                 GetCurrentProcessId(), static_cast<long long>(size.QuadPart));
  } else {
    std::fprintf(stderr, "stage:minidump_failed reason=write_failed code=%lu\n",
                 GetLastError());
    DeleteFileW(dump_path.c_str());
  }
}

LONG WINAPI top_level_crash_filter(EXCEPTION_POINTERS* information) {
  write_crash_minidump(information);
  return EXCEPTION_CONTINUE_SEARCH;
}

}  // namespace aexcompat::worker_runtime::minidump
