#pragma once

#include <stddef.h>
#include <stdint.h>

#define AEXCOMPAT_HOST_CORE_ABI_VERSION 1u
#define AEXCOMPAT_HOST_CORE_ABI_DESCRIPTOR_MAGIC \
  UINT64_C(0x41455848434F5245)
#define AEXCOMPAT_HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1 UINT64_C(1)
#define AEXCOMPAT_HOST_CORE_SCENE_IDENTITY_ABI_VERSION 1u
#define AEXCOMPAT_HOST_CORE_SCENE_IDENTITY_ABI_DESCRIPTOR_MAGIC \
  UINT64_C(0x4145585343494431)
#define AEXCOMPAT_HOST_CORE_SCENE_IDENTITY_CAPABILITY_MATCH_V1 UINT64_C(1)
#define AEXCOMPAT_HOST_CORE_SCENE_OWNER_RELATION_ABI_VERSION 1u
#define AEXCOMPAT_HOST_CORE_SCENE_OWNER_RELATION_ABI_DESCRIPTOR_MAGIC \
  UINT64_C(0x4145584F574E5231)
#define AEXCOMPAT_HOST_CORE_SCENE_OWNER_RELATION_CAPABILITY_MATCH_V1 \
  UINT64_C(1)
#define AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_ABI_VERSION 1u
#define AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_ABI_DESCRIPTOR_MAGIC \
  UINT64_C(0x414558544F504F31)
#define AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPABILITY_SUMMARY_V1 \
  UINT64_C(1)
#define AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPABILITY_OWNED_SNAPSHOT_V1 \
  UINT64_C(2)
#define AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY 16u

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

typedef enum AexHostSceneObjectKind {
  AEX_HOST_SCENE_OBJECT_KIND_NONE = 0,
  AEX_HOST_SCENE_OBJECT_KIND_PROJECT = 1,
  AEX_HOST_SCENE_OBJECT_KIND_ITEM = 2,
  AEX_HOST_SCENE_OBJECT_KIND_COMPOSITION = 3,
  AEX_HOST_SCENE_OBJECT_KIND_FOLDER = 4,
  AEX_HOST_SCENE_OBJECT_KIND_FOOTAGE = 5,
  AEX_HOST_SCENE_OBJECT_KIND_LAYER = 6,
  AEX_HOST_SCENE_OBJECT_KIND_EFFECT = 7,
  AEX_HOST_SCENE_OBJECT_KIND_STREAM = 8,
  AEX_HOST_SCENE_OBJECT_KIND_KEYFRAME = 9,
  AEX_HOST_SCENE_OBJECT_KIND_VALUE = 10,
} AexHostSceneObjectKind;

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

typedef struct AexHostSceneIdentity {
  uint64_t project_id;
  uint64_t object_id;
  uint32_t generation;
  uint8_t kind;
  uint8_t reserved[3];
} AexHostSceneIdentity;

typedef struct AexHostSceneOwnerRelation {
  AexHostSceneIdentity object;
  AexHostSceneIdentity owner;
} AexHostSceneOwnerRelation;

typedef struct AexHostSceneTopologyEntry {
  AexHostSceneOwnerRelation relation;
  int32_t local_index;
  uint32_t reserved;
} AexHostSceneTopologyEntry;

typedef struct AexHostSceneTopologySnapshot {
  uint64_t project_id;
  uint32_t entry_count;
  uint32_t reserved;
  AexHostSceneTopologyEntry
      entries[AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY];
} AexHostSceneTopologySnapshot;

typedef struct AexHostSceneTopologySummary {
  uint64_t project_id;
  uint64_t fingerprint;
  uint32_t object_count;
  uint32_t edge_count;
  uint32_t root_count;
  uint32_t reserved;
} AexHostSceneTopologySummary;

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

typedef struct AexHostSceneIdentityAbiDescriptorV1 {
  uint64_t magic;
  uint32_t abi_version;
  uint32_t struct_size;
  uint32_t identity_size;
  uint32_t identity_alignment;
  uint64_t capabilities;
} AexHostSceneIdentityAbiDescriptorV1;

typedef struct AexHostSceneOwnerRelationAbiDescriptorV1 {
  uint64_t magic;
  uint32_t abi_version;
  uint32_t struct_size;
  uint32_t relation_size;
  uint32_t relation_alignment;
  uint64_t capabilities;
} AexHostSceneOwnerRelationAbiDescriptorV1;

typedef struct AexHostSceneTopologyAbiDescriptorV1 {
  uint64_t magic;
  uint32_t abi_version;
  uint32_t struct_size;
  uint32_t entry_size;
  uint32_t entry_alignment;
  uint32_t snapshot_size;
  uint32_t snapshot_alignment;
  uint32_t summary_size;
  uint32_t summary_alignment;
  uint32_t capacity;
  uint32_t reserved;
  uint64_t capabilities;
} AexHostSceneTopologyAbiDescriptorV1;

extern const AexHostCoreAbiDescriptorV1 aex_host_core_abi_descriptor_v1;
extern const AexHostSceneIdentityAbiDescriptorV1
    aex_host_core_scene_identity_abi_descriptor_v1;
extern const AexHostSceneOwnerRelationAbiDescriptorV1
    aex_host_core_scene_owner_relation_abi_descriptor_v1;
extern const AexHostSceneTopologyAbiDescriptorV1
    aex_host_core_scene_topology_abi_descriptor_v1;

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

typedef int32_t(AEXCOMPAT_HOST_CORE_CALL
                    *AexHostCoreSceneIdentityMatchV1Fn)(
    const AexHostSceneIdentity *current,
    const AexHostSceneIdentity *candidate);

typedef int32_t(AEXCOMPAT_HOST_CORE_CALL
                    *AexHostCoreSceneOwnerRelationMatchV1Fn)(
    const AexHostSceneOwnerRelation *current,
    const AexHostSceneOwnerRelation *candidate);

typedef int32_t(AEXCOMPAT_HOST_CORE_CALL
                    *AexHostCoreSceneTopologySummarizeV1Fn)(
    const AexHostSceneTopologySnapshot *snapshot,
    AexHostSceneTopologySummary *summary);

typedef int32_t(AEXCOMPAT_HOST_CORE_CALL
                    *AexHostCoreSceneTopologySnapshotCreateV1Fn)(
    const AexHostCallContext *context,
    const AexHostSceneTopologySnapshot *snapshot,
    AexHostOpaqueHandle *handle);

typedef int32_t(AEXCOMPAT_HOST_CORE_CALL
                    *AexHostCoreSceneTopologySnapshotQueryV1Fn)(
    const AexHostCallContext *context,
    AexHostOpaqueHandle handle,
    uint32_t index,
    AexHostSceneTopologyEntry *entry);

typedef int32_t(AEXCOMPAT_HOST_CORE_CALL
                    *AexHostCoreSceneTopologySnapshotSummaryV1Fn)(
    const AexHostCallContext *context,
    AexHostOpaqueHandle handle,
    AexHostSceneTopologySummary *summary);

typedef int32_t(AEXCOMPAT_HOST_CORE_CALL
                    *AexHostCoreSceneTopologySnapshotDestroyV1Fn)(
    const AexHostCallContext *context,
    AexHostOpaqueHandle handle);

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

int32_t AEXCOMPAT_HOST_CORE_CALL aex_host_core_scene_identity_match_v1(
    const AexHostSceneIdentity *current,
    const AexHostSceneIdentity *candidate);

int32_t AEXCOMPAT_HOST_CORE_CALL aex_host_core_scene_owner_relation_match_v1(
    const AexHostSceneOwnerRelation *current,
    const AexHostSceneOwnerRelation *candidate);

int32_t AEXCOMPAT_HOST_CORE_CALL aex_host_core_scene_topology_summarize_v1(
    const AexHostSceneTopologySnapshot *snapshot,
    AexHostSceneTopologySummary *summary);

int32_t AEXCOMPAT_HOST_CORE_CALL
aex_host_core_scene_topology_snapshot_create_v1(
    const AexHostCallContext *context,
    const AexHostSceneTopologySnapshot *snapshot,
    AexHostOpaqueHandle *handle);

int32_t AEXCOMPAT_HOST_CORE_CALL
aex_host_core_scene_topology_snapshot_query_v1(
    const AexHostCallContext *context,
    AexHostOpaqueHandle handle,
    uint32_t index,
    AexHostSceneTopologyEntry *entry);

int32_t AEXCOMPAT_HOST_CORE_CALL
aex_host_core_scene_topology_snapshot_summary_v1(
    const AexHostCallContext *context,
    AexHostOpaqueHandle handle,
    AexHostSceneTopologySummary *summary);

int32_t AEXCOMPAT_HOST_CORE_CALL
aex_host_core_scene_topology_snapshot_destroy_v1(
    const AexHostCallContext *context,
    AexHostOpaqueHandle handle);

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

static_assert(sizeof(AexHostSceneIdentity) == 24);
static_assert(alignof(AexHostSceneIdentity) == 8);
static_assert(offsetof(AexHostSceneIdentity, project_id) == 0);
static_assert(offsetof(AexHostSceneIdentity, object_id) == 8);
static_assert(offsetof(AexHostSceneIdentity, generation) == 16);
static_assert(offsetof(AexHostSceneIdentity, kind) == 20);
static_assert(offsetof(AexHostSceneIdentity, reserved) == 21);

static_assert(sizeof(AexHostSceneOwnerRelation) == 48);
static_assert(alignof(AexHostSceneOwnerRelation) == 8);
static_assert(offsetof(AexHostSceneOwnerRelation, object) == 0);
static_assert(offsetof(AexHostSceneOwnerRelation, owner) == 24);

static_assert(sizeof(AexHostSceneTopologyEntry) == 56);
static_assert(alignof(AexHostSceneTopologyEntry) == 8);
static_assert(offsetof(AexHostSceneTopologyEntry, relation) == 0);
static_assert(offsetof(AexHostSceneTopologyEntry, local_index) == 48);
static_assert(offsetof(AexHostSceneTopologyEntry, reserved) == 52);

static_assert(sizeof(AexHostSceneTopologySnapshot) == 912);
static_assert(alignof(AexHostSceneTopologySnapshot) == 8);
static_assert(offsetof(AexHostSceneTopologySnapshot, project_id) == 0);
static_assert(offsetof(AexHostSceneTopologySnapshot, entry_count) == 8);
static_assert(offsetof(AexHostSceneTopologySnapshot, reserved) == 12);
static_assert(offsetof(AexHostSceneTopologySnapshot, entries) == 16);

static_assert(sizeof(AexHostSceneTopologySummary) == 32);
static_assert(alignof(AexHostSceneTopologySummary) == 8);
static_assert(offsetof(AexHostSceneTopologySummary, project_id) == 0);
static_assert(offsetof(AexHostSceneTopologySummary, fingerprint) == 8);
static_assert(offsetof(AexHostSceneTopologySummary, object_count) == 16);
static_assert(offsetof(AexHostSceneTopologySummary, edge_count) == 20);
static_assert(offsetof(AexHostSceneTopologySummary, root_count) == 24);
static_assert(offsetof(AexHostSceneTopologySummary, reserved) == 28);

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

static_assert(sizeof(AexHostSceneIdentityAbiDescriptorV1) == 32);
static_assert(alignof(AexHostSceneIdentityAbiDescriptorV1) == 8);
static_assert(
    offsetof(AexHostSceneIdentityAbiDescriptorV1, magic) == 0);
static_assert(
    offsetof(AexHostSceneIdentityAbiDescriptorV1, abi_version) == 8);
static_assert(
    offsetof(AexHostSceneIdentityAbiDescriptorV1, struct_size) == 12);
static_assert(
    offsetof(AexHostSceneIdentityAbiDescriptorV1, identity_size) == 16);
static_assert(
    offsetof(AexHostSceneIdentityAbiDescriptorV1, identity_alignment) == 20);
static_assert(
    offsetof(AexHostSceneIdentityAbiDescriptorV1, capabilities) == 24);

static_assert(sizeof(AexHostSceneOwnerRelationAbiDescriptorV1) == 32);
static_assert(alignof(AexHostSceneOwnerRelationAbiDescriptorV1) == 8);
static_assert(
    offsetof(AexHostSceneOwnerRelationAbiDescriptorV1, magic) == 0);
static_assert(
    offsetof(AexHostSceneOwnerRelationAbiDescriptorV1, abi_version) == 8);
static_assert(
    offsetof(AexHostSceneOwnerRelationAbiDescriptorV1, struct_size) == 12);
static_assert(
    offsetof(AexHostSceneOwnerRelationAbiDescriptorV1, relation_size) == 16);
static_assert(
    offsetof(AexHostSceneOwnerRelationAbiDescriptorV1,
             relation_alignment) == 20);
static_assert(
    offsetof(AexHostSceneOwnerRelationAbiDescriptorV1, capabilities) == 24);

static_assert(sizeof(AexHostSceneTopologyAbiDescriptorV1) == 56);
static_assert(alignof(AexHostSceneTopologyAbiDescriptorV1) == 8);
static_assert(offsetof(AexHostSceneTopologyAbiDescriptorV1, magic) == 0);
static_assert(offsetof(AexHostSceneTopologyAbiDescriptorV1, abi_version) == 8);
static_assert(offsetof(AexHostSceneTopologyAbiDescriptorV1, struct_size) == 12);
static_assert(offsetof(AexHostSceneTopologyAbiDescriptorV1, entry_size) == 16);
static_assert(
    offsetof(AexHostSceneTopologyAbiDescriptorV1, entry_alignment) == 20);
static_assert(
    offsetof(AexHostSceneTopologyAbiDescriptorV1, snapshot_size) == 24);
static_assert(
    offsetof(AexHostSceneTopologyAbiDescriptorV1, snapshot_alignment) == 28);
static_assert(
    offsetof(AexHostSceneTopologyAbiDescriptorV1, summary_size) == 32);
static_assert(
    offsetof(AexHostSceneTopologyAbiDescriptorV1, summary_alignment) == 36);
static_assert(offsetof(AexHostSceneTopologyAbiDescriptorV1, capacity) == 40);
static_assert(offsetof(AexHostSceneTopologyAbiDescriptorV1, reserved) == 44);
static_assert(
    offsetof(AexHostSceneTopologyAbiDescriptorV1, capabilities) == 48);

static_assert(AEX_HOST_SCENE_OBJECT_KIND_PROJECT == 1);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_ITEM == 2);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_COMPOSITION == 3);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_FOLDER == 4);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_FOOTAGE == 5);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_LAYER == 6);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_EFFECT == 7);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_STREAM == 8);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_KEYFRAME == 9);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_VALUE == 10);
static_assert(AEX_HOST_SCENE_OBJECT_KIND_NONE == 0);
#endif
