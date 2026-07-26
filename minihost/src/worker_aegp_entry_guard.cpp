#include "worker_aegp_entry_guard.hpp"

#include <windows.h>

namespace aexcompat::worker_runtime::aegp_entry_guard {
namespace {

// Keep SEH in a leaf with no objects that require C++ unwinding. This boundary
// contains access violations and other host-call faults without mixing __try
// with C++ destructors.
int32_t invoke_with_seh(EntryPoint entry, void* basic_suite,
                        int32_t driver_major, int32_t driver_minor,
                        int32_t plugin_id, void** global_refcon,
                        uint32_t* seh_code, bool* faulted) noexcept {
  __try {
    return entry(basic_suite, driver_major, driver_minor, plugin_id,
                 global_refcon);
  } __except ((*seh_code = static_cast<uint32_t>(GetExceptionCode()),
               EXCEPTION_EXECUTE_HANDLER)) {
    *faulted = true;
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
  try {
    bool seh_faulted = false;
    result.error = invoke_with_seh(
        entry, basic_suite, driver_major, driver_minor, plugin_id,
        global_refcon, &result.seh_code, &seh_faulted);
    if (seh_faulted) result.fault = FaultKind::seh_exception;
  } catch (...) {
    result.error = 4;
    result.fault = FaultKind::cpp_exception;
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

}  // namespace aexcompat::worker_runtime::aegp_entry_guard
