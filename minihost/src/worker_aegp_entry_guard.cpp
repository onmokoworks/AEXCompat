#include "worker_aegp_entry_guard.hpp"

#include <windows.h>

namespace aexcompat::worker_runtime::aegp_entry_guard {
namespace {

constexpr uint32_t kMsvcCppException = UINT32_C(0xe06d7363);

bool approved_seh_code(uint32_t code) noexcept {
  return code == EXCEPTION_ACCESS_VIOLATION ||
      code == EXCEPTION_IN_PAGE_ERROR;
}

bool same_module(const void* left, const void* right) noexcept {
  if (!left || !right) return false;
  MEMORY_BASIC_INFORMATION left_info{};
  MEMORY_BASIC_INFORMATION right_info{};
  return VirtualQuery(left, &left_info, sizeof(left_info)) ==
             sizeof(left_info) &&
      VirtualQuery(right, &right_info, sizeof(right_info)) ==
             sizeof(right_info) &&
      left_info.AllocationBase == right_info.AllocationBase;
}

int32_t filter_exception(EXCEPTION_POINTERS* exception,
                         EntryPoint entry, uint32_t* code) noexcept {
  if (!exception || !exception->ExceptionRecord || !entry || !code)
    return EXCEPTION_CONTINUE_SEARCH;
  *code = static_cast<uint32_t>(
      exception->ExceptionRecord->ExceptionCode);
  if (*code == kMsvcCppException || !approved_seh_code(*code))
    return EXCEPTION_CONTINUE_SEARCH;
  return same_module(
      exception->ExceptionRecord->ExceptionAddress,
      reinterpret_cast<const void*>(entry))
      ? EXCEPTION_EXECUTE_HANDLER
      : EXCEPTION_CONTINUE_SEARCH;
}

// Catch synchronous C++ exceptions immediately around the external call so
// plugin frames and suite leases unwind before control returns to the SEH leaf.
// Under /EHsc this does not consume asynchronous Windows faults.
__declspec(noinline) int32_t invoke_with_cpp_boundary(
    EntryPoint entry, void* basic_suite, int32_t driver_major,
    int32_t driver_minor, int32_t plugin_id, void** global_refcon,
    bool* faulted) noexcept {
  try {
    return entry(basic_suite, driver_major, driver_minor, plugin_id,
                 global_refcon);
  } catch (...) {
    *faulted = true;
    return 4;
  }
}

// Keep SEH in a leaf with no C++ objects that require unwinding. Only
// attributed access/in-page violations are handled here; C++ EH is consumed
// by invoke_with_cpp_boundary and unrelated corruption continues search.
__declspec(noinline) int32_t invoke_with_seh(
    EntryPoint entry, void* basic_suite, int32_t driver_major,
    int32_t driver_minor, int32_t plugin_id, void** global_refcon,
    uint32_t* seh_code, bool* seh_faulted, bool* cpp_faulted) {
  __try {
    return invoke_with_cpp_boundary(
        entry, basic_suite, driver_major, driver_minor, plugin_id,
        global_refcon, cpp_faulted);
  } __except (filter_exception(
      GetExceptionInformation(), entry, seh_code)) {
    *seh_faulted = true;
    return 4;
  }
}

}  // namespace

Result invoke(EntryPoint entry, void* basic_suite, int32_t driver_major,
              int32_t driver_minor, int32_t plugin_id,
              void** global_refcon) noexcept {
  Result result;
  if (!entry || !basic_suite || !global_refcon) {
    result.fault = FaultKind::invalid_request;
    return result;
  }
  result.invoked = true;
  bool seh_faulted = false;
  bool cpp_faulted = false;
  result.error = invoke_with_seh(
      entry, basic_suite, driver_major, driver_minor, plugin_id,
      global_refcon, &result.seh_code, &seh_faulted, &cpp_faulted);
  if (seh_faulted) {
    result.fault = FaultKind::seh_exception;
  } else if (cpp_faulted) {
    result.fault = FaultKind::cpp_exception;
    result.seh_code = 0;
  }
  return result;
}

const char* fault_name(FaultKind fault) noexcept {
  switch (fault) {
    case FaultKind::none: return "none";
    case FaultKind::invalid_request: return "invalid_request";
    case FaultKind::cpp_exception: return "cpp_exception";
    case FaultKind::seh_exception: return "seh_exception";
  }
  return "unknown";
}

int32_t seh_filter_disposition_for_test(
    uint32_t code, const void* exception_address,
    const void* entry_address) noexcept {
  if (code == kMsvcCppException || !approved_seh_code(code))
    return EXCEPTION_CONTINUE_SEARCH;
  return same_module(exception_address, entry_address)
      ? EXCEPTION_EXECUTE_HANDLER
      : EXCEPTION_CONTINUE_SEARCH;
}

}  // namespace aexcompat::worker_runtime::aegp_entry_guard
