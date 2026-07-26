#pragma once

#include <cstdint>

namespace aexcompat::worker_runtime::aegp_entry_guard {

using EntryPoint = int32_t(__cdecl*)(void*, int32_t, int32_t, int32_t, void**);

enum class FaultKind : uint8_t {
  none = 0,
  invalid_request = 1,
  cpp_exception = 2,
  seh_exception = 3,
};

struct Result {
  int32_t error{4};
  FaultKind fault{FaultKind::none};
  uint32_t seh_code{};
  bool invoked{};
};

Result invoke(EntryPoint entry, void* basic_suite, int32_t driver_major,
              int32_t driver_minor, int32_t plugin_id,
              void** global_refcon) noexcept;
const char* fault_name(FaultKind fault) noexcept;
int32_t seh_filter_disposition_for_test(
    uint32_t code, const void* exception_address,
    const void* entry_address) noexcept;

}  // namespace aexcompat::worker_runtime::aegp_entry_guard
