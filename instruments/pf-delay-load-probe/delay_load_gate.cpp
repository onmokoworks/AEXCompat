#include <windows.h>
#include <delayimp.h>

#include <cctype>
#include <cstdio>
#include <cwchar>

namespace {
constexpr DWORD kDelayLoadModuleNotFound = 0xC06D007Eu;

int exception_filter(EXCEPTION_POINTERS* information, char* missing,
                     std::size_t capacity) {
  if (!information || !information->ExceptionRecord || !missing || capacity < 2)
    return EXCEPTION_EXECUTE_HANDLER;
  const EXCEPTION_RECORD* record = information->ExceptionRecord;
  if (record->ExceptionCode != kDelayLoadModuleNotFound ||
      record->NumberParameters < 1 || record->ExceptionInformation[0] == 0)
    return EXCEPTION_EXECUTE_HANDLER;
  __try {
    const auto* delay = reinterpret_cast<const DelayLoadInfo*>(
        record->ExceptionInformation[0]);
    const char* name = delay->szDll;
    std::size_t length = 0;
    if (!name) return EXCEPTION_EXECUTE_HANDLER;
    for (; length + 1 < capacity; ++length) {
      const unsigned char byte = static_cast<unsigned char>(name[length]);
      if (byte == 0) break;
      if (!(std::isalnum(byte) || byte == '.' || byte == '_' || byte == '-'))
        return EXCEPTION_EXECUTE_HANDLER;
      missing[length] = static_cast<char>(byte);
    }
    if (length > 0 && length + 1 < capacity) missing[length] = '\0';
  } __except(EXCEPTION_EXECUTE_HANDLER) {
  }
  return EXCEPTION_EXECUTE_HANDLER;
}
}  // namespace

int wmain(int argc, wchar_t** argv) {
  if (argc != 3) return 2;
  const bool expect_success = std::wcscmp(argv[2], L"success") == 0;
  if (!expect_success && std::wcscmp(argv[2], L"missing") != 0) return 2;
  if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 |
                                LOAD_LIBRARY_SEARCH_USER_DIRS)) return 3;
  DLL_DIRECTORY_COOKIE cookie = AddDllDirectory(argv[1]);
  if (!cookie) return 4;
  wchar_t plugin[MAX_PATH]{};
  if (std::swprintf(plugin, MAX_PATH, L"%ls\\pf_delay_load_probe.aex", argv[1]) < 0)
    return 5;
  HMODULE module = LoadLibraryExW(plugin, nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!module) return 6;
  const auto invoke = reinterpret_cast<int(__cdecl*)()>(
      GetProcAddress(module, "Issue60InvokeDelayLoad"));
  if (!invoke) return 7;
  int result = 0;
  char missing[261]{};
  __try {
    result = invoke();
  } __except(exception_filter(GetExceptionInformation(), missing, sizeof(missing))) {
  }
  FreeLibrary(module);
  RemoveDllDirectory(cookie);
  if (expect_success) return result == 0x60 ? 0 : 8;
  if (_stricmp(missing, "issue60_delay_dependency.dll") != 0) return 9;
  std::printf("missing_dependency=%s\n", missing);
  return 0;
}
