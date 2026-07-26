#include "worker_aegp_entry_guard.hpp"

#include <windows.h>

#include <cassert>
#include <cstdint>
#include <stdexcept>
#include <string>

namespace {

constexpr uint32_t kMsvcCppException = UINT32_C(0xe06d7363);
constexpr uint32_t kUnrelatedException = UINT32_C(0xe000b26f);

struct SuiteCounters {
  uint32_t acquires{};
  uint32_t releases{};
};

struct SuiteLease {
  SuiteCounters* counters{};
  explicit SuiteLease(SuiteCounters& value) noexcept : counters(&value) {
    ++counters->acquires;
  }
  ~SuiteLease() {
    if (counters) ++counters->releases;
  }
};

int32_t __cdecl throwing_entry(
    void* basic_suite, int32_t, int32_t, int32_t, void**) {
  auto& counters = *static_cast<SuiteCounters*>(basic_suite);
  SuiteLease lease(counters);
  throw std::runtime_error("expected entrypoint exception");
}

int32_t __cdecl attributed_access_violation(
    void*, int32_t, int32_t, int32_t, void**) {
  *static_cast<volatile uint32_t*>(nullptr) = 1;
  return 0;
}

int32_t __cdecl unrelated_exception(
    void*, int32_t, int32_t, int32_t, void**) {
  RaiseException(kUnrelatedException, 0, 0, nullptr);
  return 0;
}

bool run_unrelated_child() {
  wchar_t executable[32768]{};
  const DWORD length = GetModuleFileNameW(
      nullptr, executable, static_cast<DWORD>(std::size(executable)));
  if (!length || length >= std::size(executable)) return false;
  std::wstring command = L"\"";
  command.append(executable, length);
  command += L"\" --raise-unrelated";
  STARTUPINFOW startup{};
  startup.cb = sizeof(startup);
  PROCESS_INFORMATION process{};
  if (!CreateProcessW(
          executable, command.data(), nullptr, nullptr, FALSE,
          CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process))
    return false;
  const DWORD wait = WaitForSingleObject(process.hProcess, 30000);
  DWORD exit_code = 0;
  const bool exited = wait == WAIT_OBJECT_0 &&
      GetExitCodeProcess(process.hProcess, &exit_code);
  CloseHandle(process.hThread);
  CloseHandle(process.hProcess);
  return exited && exit_code == kUnrelatedException;
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
  SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
  if (argc == 2 && std::wstring(argv[1]) == L"--raise-unrelated") {
    void* refcon = nullptr;
    int basic = 1;
    (void)aexcompat::worker_runtime::aegp_entry_guard::invoke(
        &unrelated_exception, &basic, 24, 0, 1, &refcon);
    return 99;
  }

  using namespace aexcompat::worker_runtime::aegp_entry_guard;
  SuiteCounters counters{};
  void* refcon = nullptr;
  const Result cpp = invoke(
      &throwing_entry, &counters, 24, 0, 1, &refcon);
  assert(cpp.invoked);
  assert(cpp.error == 4);
  assert(cpp.fault == FaultKind::cpp_exception);
  assert(cpp.seh_code == 0);
  assert(counters.acquires == 1);
  assert(counters.releases == 1);

  int basic = 1;
  const Result access = invoke(
      &attributed_access_violation, &basic, 24, 0, 1, &refcon);
  assert(access.invoked);
  assert(access.error == 4);
  assert(access.fault == FaultKind::seh_exception);
  assert(access.seh_code == EXCEPTION_ACCESS_VIOLATION);

  assert(seh_filter_disposition_for_test(
             kMsvcCppException,
             reinterpret_cast<const void*>(&throwing_entry),
             reinterpret_cast<const void*>(&throwing_entry)) ==
         EXCEPTION_CONTINUE_SEARCH);
  assert(seh_filter_disposition_for_test(
             STATUS_STACK_BUFFER_OVERRUN,
             reinterpret_cast<const void*>(&attributed_access_violation),
             reinterpret_cast<const void*>(&attributed_access_violation)) ==
         EXCEPTION_CONTINUE_SEARCH);
  assert(seh_filter_disposition_for_test(
             EXCEPTION_ACCESS_VIOLATION,
             reinterpret_cast<const void*>(&attributed_access_violation),
             reinterpret_cast<const void*>(&attributed_access_violation)) ==
         EXCEPTION_EXECUTE_HANDLER);
  assert(seh_filter_disposition_for_test(
             EXCEPTION_ACCESS_VIOLATION,
             reinterpret_cast<const void*>(&RaiseException),
             reinterpret_cast<const void*>(&attributed_access_violation)) ==
         EXCEPTION_CONTINUE_SEARCH);
  assert(run_unrelated_child());
  return 0;
}
