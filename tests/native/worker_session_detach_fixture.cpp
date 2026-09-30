#include <windows.h>

#include <cstdlib>
#include <fstream>
#include <iterator>

BOOL WINAPI DllMain(HINSTANCE, DWORD reason, LPVOID reserved) {
  if (reason != DLL_PROCESS_DETACH) return TRUE;
  wchar_t delay[16]{};
  const DWORD delay_length = GetEnvironmentVariableW(
      L"AEXCOMPAT_DETACH_DELAY_MS", delay, static_cast<DWORD>(std::size(delay)));
  if (delay_length > 0 && delay_length < std::size(delay)) {
    wchar_t* end = nullptr;
    const unsigned long milliseconds = wcstoul(delay, &end, 10);
    if (end && *end == L'\0' && milliseconds <= 5000) Sleep(milliseconds);
  }
  wchar_t path[32768]{};
  const DWORD length = GetEnvironmentVariableW(
      L"AEXCOMPAT_DETACH_MARKER", path, static_cast<DWORD>(std::size(path)));
  if (length > 0 && length < std::size(path)) {
    std::ofstream marker(path, std::ios::binary | std::ios::trunc);
    marker << (reserved ? "process" : "explicit");
  }
  return TRUE;
}
