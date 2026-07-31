#include "aexcompat_host_core_abi.h"

#include <cstddef>

int main() {
  const bool layout_ok =
      AEXCOMPAT_HOST_CORE_ABI_VERSION == 1u &&
      sizeof(AexHostCallContext) == 24 &&
      offsetof(AexHostCallContext, session_id) == 8 &&
      offsetof(AexHostCallContext, caller_thread_token) == 16 &&
      sizeof(AexHostCallStatus) == 24 &&
      offsetof(AexHostCallStatus, code) == 8 &&
      offsetof(AexHostCallStatus, report_id) == 16 &&
      sizeof(AexHostOpaqueHandle) == 8;
  const bool codes_ok =
      AEX_HOST_OK == 0 &&
      AEX_HOST_WRONG_THREAD == 3 &&
      AEX_HOST_WRONG_OWNER == 5 &&
      AEX_HOST_STALE_HANDLE == 7 &&
      AEX_HOST_PANIC == 8 &&
      AEX_HOST_SEH_FAULT == 9;
  return layout_ok && codes_ok ? 0 : 1;
}
