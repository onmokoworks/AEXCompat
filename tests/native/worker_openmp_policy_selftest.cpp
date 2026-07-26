#include "worker_openmp_policy.hpp"

#include <windows.h>

#include <cstdlib>
#include <string>

int main() {
  if (_wputenv_s(L"OMP_NUM_THREADS", L"7") != 0) return 1;

  std::string diagnostic;
  if (!aexcompat::worker_runtime::openmp::install_deterministic_policy(
          diagnostic) ||
      diagnostic != "OMP_NUM_THREADS=1")
    return 2;

  wchar_t environment_value[2]{};
  if (GetEnvironmentVariableW(L"OMP_NUM_THREADS", environment_value, 2) != 1 ||
      environment_value[0] != L'1')
    return 3;

  const HMODULE vcomp = LoadLibraryExW(
      L"vcomp140.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!vcomp) return 4;
  const auto get_max_threads = reinterpret_cast<int(__cdecl*)()>(
      GetProcAddress(vcomp, "omp_get_max_threads"));
  const bool deterministic = get_max_threads && get_max_threads() == 1;
  FreeLibrary(vcomp);
  return deterministic ? 0 : 5;
}
