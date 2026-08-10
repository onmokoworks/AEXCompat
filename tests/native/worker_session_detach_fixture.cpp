#include <windows.h>

#include <fstream>

BOOL WINAPI DllMain(HINSTANCE, DWORD reason, LPVOID) {
  if (reason != DLL_PROCESS_DETACH) return TRUE;
  wchar_t path[32768]{};
  const DWORD length = GetEnvironmentVariableW(
      L"AEXCOMPAT_DETACH_MARKER", path, static_cast<DWORD>(std::size(path)));
  if (length > 0 && length < std::size(path)) {
    std::ofstream marker(path, std::ios::binary | std::ios::trunc);
    marker << "detached";
  }
  return TRUE;
}
