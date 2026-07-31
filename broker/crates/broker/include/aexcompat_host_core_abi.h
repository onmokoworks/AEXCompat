#pragma once

#include <stddef.h>
#include <stdint.h>

#define AEXCOMPAT_HOST_CORE_ABI_VERSION 1u
#define AEXCOMPAT_HOST_CORE_ABI_DESCRIPTOR_MAGIC \
  UINT64_C(0x41455848434F5245)
#define AEXCOMPAT_HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1 UINT64_C(1)

#if defined(_WIN32)
#define AEXCOMPAT_HOST_CORE_CALL __cdecl
#else
#define AEXCOMPAT_HOST_CORE_CALL
#endif

#if defined(__cplusplus)
extern "C" {
#endif

typedef enum AexHostErrorCode {
  AEX_HOST_OK = 0,
  AEX_HOST_INVALID_ARGUMENT = 1,
  AEX_HOST_INVALID_STATE = 2,
  AEX_HOST_WRONG_THREAD = 3,
  AEX_HOST_INVALID_HANDLE = 4,
  AEX_HOST_WRONG_OWNER = 5,
  AEX_HOST_WRONG_KIND = 6,
  AEX_HOST_STALE_HANDLE = 7,
  AEX_HOST_PANIC = 8,
  AEX_HOST_SEH_FAULT = 9,
  AEX_HOST_CAPACITY_EXCEEDED = 10,
} AexHostErrorCode;

typedef enum AexHostReportPhase {
  AEX_HOST_REPORT_PHASE_BOUNDARY = 1,
  AEX_HOST_REPORT_PHASE_HANDLE = 2,
  AEX_HOST_REPORT_PHASE_SESSION = 3,
} AexHostReportPhase;

typedef enum AexHostReportOutcome {
  AEX_HOST_REPORT_OUTCOME_PASSED = 1,
  AEX_HOST_REPORT_OUTCOME_REJECTED = 2,
  AEX_HOST_REPORT_OUTCOME_FAULTED = 3,
} AexHostReportOutcome;

typedef enum AexHostHandleKind {
  AEX_HOST_HANDLE_KIND_NONE = 0,
  AEX_HOST_HANDLE_KIND_SCENE = 1,
  AEX_HOST_HANDLE_KIND_WORLD = 2,
  AEX_HOST_HANDLE_KIND_PARAMETER = 3,
  AEX_HOST_HANDLE_KIND_SESSION = 4,
  AEX_HOST_HANDLE_KIND_REPORT = 5,
} AexHostHandleKind;

typedef enum AexHostSessionState {
  AEX_HOST_SESSION_STATE_NONE = 0,
  AEX_HOST_SESSION_STATE_CREATED = 1,
  AEX_HOST_SESSION_STATE_OPEN = 2,
  AEX_HOST_SESSION_STATE_IN_CALLBACK = 3,
  AEX_HOST_SESSION_STATE_CLOSED = 4,
  AEX_HOST_SESSION_STATE_FAULTED = 5,
} AexHostSessionState;

typedef struct AexHostCallContext {
  uint32_t abi_version;
  uint32_t struct_size;
  uint64_t session_id;
  uint64_t caller_thread_token;
} AexHostCallContext;

typedef struct AexHostCallStatus {
  uint32_t abi_version;
  uint32_t struct_size;
  int32_t code;
  uint32_t reserved;
  uint64_t report_id;
} AexHostCallStatus;

typedef struct AexHostOpaqueHandle {
  uint64_t value;
} AexHostOpaqueHandle;

typedef struct AexHostReportSnapshot {
  uint32_t abi_version;
  uint32_t struct_size;
  uint32_t schema_version;
  uint32_t phase;
  uint32_t outcome;
  int32_t error_code;
  uint32_t handle_kind;
  uint32_t session_state;
  uint64_t report_id;
  uint64_t handles_created;
  uint64_t handles_disposed;
  uint64_t callbacks_attempted;
  uint64_t callbacks_completed;
} AexHostReportSnapshot;

typedef struct AexHostCoreAbiDescriptorV1 {
  uint64_t magic;
  uint32_t abi_version;
  uint32_t struct_size;
  uint32_t call_context_size;
  uint32_t call_context_alignment;
  uint32_t call_status_size;
  uint32_t call_status_alignment;
  uint32_t opaque_handle_size;
  uint32_t opaque_handle_alignment;
  uint32_t report_snapshot_size;
  uint32_t report_snapshot_alignment;
  uint64_t capabilities;
} AexHostCoreAbiDescriptorV1;

extern const AexHostCoreAbiDescriptorV1 aex_host_core_abi_descriptor_v1;

typedef int32_t(AEXCOMPAT_HOST_CORE_CALL *AexHostCoreSessionCreateV1Fn)(
    const AexHostCallContext *context,
    AexHostOpaqueHandle *session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report);

typedef int32_t(AEXCOMPAT_HOST_CORE_CALL *AexHostCoreSessionCallV1Fn)(
    const AexHostCallContext *context,
    AexHostOpaqueHandle session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report);

int32_t AEXCOMPAT_HOST_CORE_CALL aex_host_core_session_create_v1(
    const AexHostCallContext *context,
    AexHostOpaqueHandle *session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report);

int32_t AEXCOMPAT_HOST_CORE_CALL aex_host_core_session_open_v1(
    const AexHostCallContext *context,
    AexHostOpaqueHandle session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report);

int32_t AEXCOMPAT_HOST_CORE_CALL aex_host_core_session_begin_callback_v1(
    const AexHostCallContext *context,
    AexHostOpaqueHandle session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report);

int32_t AEXCOMPAT_HOST_CORE_CALL aex_host_core_session_end_callback_v1(
    const AexHostCallContext *context,
    AexHostOpaqueHandle session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report);

int32_t AEXCOMPAT_HOST_CORE_CALL aex_host_core_session_close_v1(
    const AexHostCallContext *context,
    AexHostOpaqueHandle session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report);

int32_t AEXCOMPAT_HOST_CORE_CALL aex_host_core_session_dispose_v1(
    const AexHostCallContext *context,
    AexHostOpaqueHandle session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report);

#if defined(__cplusplus)
}
#endif

#if defined(__cplusplus)
static_assert(sizeof(AexHostCallContext) == 24);
static_assert(alignof(AexHostCallContext) == 8);
static_assert(offsetof(AexHostCallContext, abi_version) == 0);
static_assert(offsetof(AexHostCallContext, struct_size) == 4);
static_assert(offsetof(AexHostCallContext, session_id) == 8);
static_assert(offsetof(AexHostCallContext, caller_thread_token) == 16);

static_assert(sizeof(AexHostCallStatus) == 24);
static_assert(alignof(AexHostCallStatus) == 8);
static_assert(offsetof(AexHostCallStatus, abi_version) == 0);
static_assert(offsetof(AexHostCallStatus, struct_size) == 4);
static_assert(offsetof(AexHostCallStatus, code) == 8);
static_assert(offsetof(AexHostCallStatus, reserved) == 12);
static_assert(offsetof(AexHostCallStatus, report_id) == 16);

static_assert(sizeof(AexHostOpaqueHandle) == 8);
static_assert(alignof(AexHostOpaqueHandle) == 8);

static_assert(sizeof(AexHostReportSnapshot) == 72);
static_assert(alignof(AexHostReportSnapshot) == 8);
static_assert(offsetof(AexHostReportSnapshot, error_code) == 20);
static_assert(offsetof(AexHostReportSnapshot, report_id) == 32);
static_assert(offsetof(AexHostReportSnapshot, callbacks_completed) == 64);

static_assert(sizeof(AexHostCoreAbiDescriptorV1) == 56);
static_assert(alignof(AexHostCoreAbiDescriptorV1) == 8);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, magic) == 0);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, abi_version) == 8);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, struct_size) == 12);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, call_context_size) == 16);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, call_context_alignment) ==
              20);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, call_status_size) == 24);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, call_status_alignment) ==
              28);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, opaque_handle_size) == 32);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, opaque_handle_alignment) ==
              36);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, report_snapshot_size) == 40);
static_assert(
    offsetof(AexHostCoreAbiDescriptorV1, report_snapshot_alignment) == 44);
static_assert(offsetof(AexHostCoreAbiDescriptorV1, capabilities) == 48);
#endif
