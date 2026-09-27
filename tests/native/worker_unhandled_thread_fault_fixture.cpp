#include <windows.h>

#include <cstdint>

// A stand-in for an AEX-owned thread: its faulting instruction is inside this
// DLL, outside the worker's selector SEH frame. It must remain unhandled.
extern "C" __declspec(dllexport) DWORD WINAPI worker_unhandled_thread_fault(void*) {
  *reinterpret_cast<volatile int*>(static_cast<uintptr_t>(1)) = 1;
  return 0;
}
