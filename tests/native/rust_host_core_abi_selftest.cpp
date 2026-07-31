#include "aexcompat_host_core_abi.h"

#include <cstddef>

namespace {

constexpr AexHostCoreAbiDescriptorV1 kExpectedDescriptor{
    AEXCOMPAT_HOST_CORE_ABI_DESCRIPTOR_MAGIC,
    AEXCOMPAT_HOST_CORE_ABI_VERSION,
    static_cast<uint32_t>(sizeof(AexHostCoreAbiDescriptorV1)),
    static_cast<uint32_t>(sizeof(AexHostCallContext)),
    static_cast<uint32_t>(alignof(AexHostCallContext)),
    static_cast<uint32_t>(sizeof(AexHostCallStatus)),
    static_cast<uint32_t>(alignof(AexHostCallStatus)),
    static_cast<uint32_t>(sizeof(AexHostOpaqueHandle)),
    static_cast<uint32_t>(alignof(AexHostOpaqueHandle)),
    static_cast<uint32_t>(sizeof(AexHostReportSnapshot)),
    static_cast<uint32_t>(alignof(AexHostReportSnapshot)),
    AEXCOMPAT_HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1,
};

static_assert(kExpectedDescriptor.magic ==
              AEXCOMPAT_HOST_CORE_ABI_DESCRIPTOR_MAGIC);
static_assert(kExpectedDescriptor.abi_version ==
              AEXCOMPAT_HOST_CORE_ABI_VERSION);
static_assert(kExpectedDescriptor.struct_size == 56);
static_assert(kExpectedDescriptor.call_context_size == 24);
static_assert(kExpectedDescriptor.call_context_alignment == 8);
static_assert(kExpectedDescriptor.call_status_size == 24);
static_assert(kExpectedDescriptor.call_status_alignment == 8);
static_assert(kExpectedDescriptor.opaque_handle_size == 8);
static_assert(kExpectedDescriptor.opaque_handle_alignment == 8);
static_assert(kExpectedDescriptor.report_snapshot_size == 72);
static_assert(kExpectedDescriptor.report_snapshot_alignment == 8);
static_assert(kExpectedDescriptor.capabilities ==
              AEXCOMPAT_HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1);

}  // namespace

int main() {
  const bool layout_ok =
      AEXCOMPAT_HOST_CORE_ABI_VERSION == 1u &&
      sizeof(AexHostCallContext) == 24 &&
      offsetof(AexHostCallContext, session_id) == 8 &&
      offsetof(AexHostCallContext, caller_thread_token) == 16 &&
      sizeof(AexHostCallStatus) == 24 &&
      offsetof(AexHostCallStatus, code) == 8 &&
      offsetof(AexHostCallStatus, report_id) == 16 &&
      sizeof(AexHostOpaqueHandle) == 8 &&
      sizeof(AexHostReportSnapshot) == 72 &&
      offsetof(AexHostReportSnapshot, error_code) == 20 &&
      offsetof(AexHostReportSnapshot, report_id) == 32 &&
      offsetof(AexHostReportSnapshot, callbacks_completed) == 64 &&
      sizeof(AexHostCoreAbiDescriptorV1) == 56 &&
      alignof(AexHostCoreAbiDescriptorV1) == 8 &&
      offsetof(AexHostCoreAbiDescriptorV1, magic) == 0 &&
      offsetof(AexHostCoreAbiDescriptorV1, abi_version) == 8 &&
      offsetof(AexHostCoreAbiDescriptorV1, struct_size) == 12 &&
      offsetof(AexHostCoreAbiDescriptorV1, call_context_size) == 16 &&
      offsetof(AexHostCoreAbiDescriptorV1, call_context_alignment) == 20 &&
      offsetof(AexHostCoreAbiDescriptorV1, call_status_size) == 24 &&
      offsetof(AexHostCoreAbiDescriptorV1, call_status_alignment) == 28 &&
      offsetof(AexHostCoreAbiDescriptorV1, opaque_handle_size) == 32 &&
      offsetof(AexHostCoreAbiDescriptorV1, opaque_handle_alignment) == 36 &&
      offsetof(AexHostCoreAbiDescriptorV1, report_snapshot_size) == 40 &&
      offsetof(AexHostCoreAbiDescriptorV1, report_snapshot_alignment) == 44 &&
      offsetof(AexHostCoreAbiDescriptorV1, capabilities) == 48;
  const bool descriptor_ok =
      kExpectedDescriptor.magic ==
          AEXCOMPAT_HOST_CORE_ABI_DESCRIPTOR_MAGIC &&
      kExpectedDescriptor.abi_version ==
          AEXCOMPAT_HOST_CORE_ABI_VERSION &&
      kExpectedDescriptor.struct_size ==
          sizeof(AexHostCoreAbiDescriptorV1) &&
      kExpectedDescriptor.call_context_size == sizeof(AexHostCallContext) &&
      kExpectedDescriptor.call_context_alignment ==
          alignof(AexHostCallContext) &&
      kExpectedDescriptor.call_status_size == sizeof(AexHostCallStatus) &&
      kExpectedDescriptor.call_status_alignment ==
          alignof(AexHostCallStatus) &&
      kExpectedDescriptor.opaque_handle_size == sizeof(AexHostOpaqueHandle) &&
      kExpectedDescriptor.opaque_handle_alignment ==
          alignof(AexHostOpaqueHandle) &&
      kExpectedDescriptor.report_snapshot_size ==
          sizeof(AexHostReportSnapshot) &&
      kExpectedDescriptor.report_snapshot_alignment ==
          alignof(AexHostReportSnapshot) &&
      kExpectedDescriptor.capabilities ==
          AEXCOMPAT_HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1;
  const bool codes_ok =
      AEX_HOST_OK == 0 &&
      AEX_HOST_WRONG_THREAD == 3 &&
      AEX_HOST_WRONG_OWNER == 5 &&
      AEX_HOST_STALE_HANDLE == 7 &&
      AEX_HOST_PANIC == 8 &&
      AEX_HOST_SEH_FAULT == 9 &&
      AEX_HOST_REPORT_PHASE_SESSION == 3 &&
      AEX_HOST_REPORT_OUTCOME_PASSED == 1 &&
      AEX_HOST_HANDLE_KIND_SESSION == 4 &&
      AEX_HOST_SESSION_STATE_CREATED == 1 &&
      AEX_HOST_SESSION_STATE_FAULTED == 5;
  return layout_ok && descriptor_ok && codes_ok ? 0 : 1;
}
