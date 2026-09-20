#include <windows.h>

extern "C" __declspec(dllexport) int runtime_module_path_cache_fixture_value() {
  return 37;
}

BOOL APIENTRY DllMain(HMODULE, DWORD, void*) { return TRUE; }
