#pragma once

#include <stddef.h>
#include <stdint.h>

#define AEXCOMPAT_HOST_CORE_ABI_VERSION 1u

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
#endif
