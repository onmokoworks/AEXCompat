#pragma once

#include <cstddef>
#include <cstdint>
#include <string>

// AEGP Persistent Data Suite v3 (`AE_GeneralPlugOld.h`, frozen in AE 10.0).
//
// Observed caller: `DeepGlow2.aex` acquires it in SEQUENCE_SETUP and, when the
// acquire fails, returns PF_Err_INTERNAL_STRUCT_DAMAGED (512) and abandons the
// setup, which the worker reports as the reserved session error -47 and the
// broker turns into an invalidated session (issue #881). The suite is a
// key/value store the host owns, so implementing it is a general host
// capability, not a per-plug-in accommodation.
//
// Scope of this implementation: the blob lives for the worker process. AE
// writes its application blob to the preferences file, so a value a plug-in
// sets there survives a restart; here it does not, and a plug-in that stores
// "have I run before" state sees a first run on every session. That is a
// recorded AE-equivalence gap rather than a hidden approximation - persisting
// the blob would put host state on disk under the plug-in's own token, which
// is a separate decision from making the suite exist.
namespace aexcompat::worker_runtime::persistent_data {

// The SDK spellings this suite is declared with (`A.h`).
using A_Err = int32_t;
using A_long = int32_t;
using A_u_long = uint32_t;
using A_char = char;
using A_FpLong = double;
using A_Boolean = unsigned char;
using AEGP_PluginID = A_long;
// Opaque in the SDK (`struct _AEGP_PersistentBlob**`); opaque here too. Only
// the handle this host hands out is accepted back, so a stale or foreign
// pointer is refused rather than dereferenced.
using AEGP_PersistentBlobH = void*;
using AEGP_MemHandle = void*;

static_assert(sizeof(A_Boolean) == 1);
static_assert(sizeof(A_FpLong) == 8);

// `A.h` error codes. A_Err_MISSING_SUITE is not reachable from here.
inline constexpr A_Err kErrNone = 0;
inline constexpr A_Err kErrGeneric = 1;
inline constexpr A_Err kErrStruct = 2;
inline constexpr A_Err kErrParameter = 3;
inline constexpr A_Err kErrAlloc = 4;

inline constexpr char kSuiteName[] = "AEGP Persistent Data Suite";
inline constexpr int32_t kSuiteVersion3 = 3;

// Bounds. A plug-in drives every one of these, so each has a ceiling that
// turns a runaway or malformed caller into a refused call with a diagnostic
// instead of unbounded host allocation. The values are generous next to what
// a preferences blob holds and small next to the worker's memory limit.
//
// A string value gets the same ceiling as an opaque one: a plug-in serializes
// state into either, and refusing at a lower bound on the string side would be
// a host-only limit AE does not have. Only keys are held to a shorter bound,
// because a key is a name.
inline constexpr std::size_t kMaxKeyBytes = 255;
inline constexpr std::size_t kMaxSections = 256;
inline constexpr std::size_t kMaxKeysPerSection = 1024;
inline constexpr std::size_t kMaxValueBytes = 1u << 20;
// Counts the stored key and value bytes, not the containers holding them: the
// per-entry `std::string`/`std::vector` overhead and the allocator's own are
// outside it. With every other bound at its ceiling those add tens of MB on
// top, which the worker's Job Object memory limit is what actually contains.
// This bound exists to stop one plug-in filling the blob, not to be an
// accounting of the process's footprint.
inline constexpr std::size_t kMaxBlobBytes = 16u << 20;

// How a value was written. AE stores its blob as text, so its Get/Set pairs
// convert between the stored text and the requested type; that conversion
// table is not observable from the SDK headers, and guessing one would make
// this host disagree with AE in a way no diagnostic would catch. Instead each
// entry remembers the setter that wrote it: a Get of the same kind round-trips
// exactly, and a Get of a different kind is a recorded type mismatch that
// answers with the caller's default. `data` covers both AEGP_SetData and
// AEGP_SetDataHandle, which write the same opaque bytes.
enum class ValueKind : uint8_t { data = 0, string = 1, integer = 2, floating = 3 };

using GetApplicationBlob = A_Err(__cdecl*)(AEGP_PersistentBlobH*);
using GetNumSections = A_Err(__cdecl*)(AEGP_PersistentBlobH, A_long*);
using GetSectionKeyByIndex = A_Err(__cdecl*)(AEGP_PersistentBlobH, A_long,
                                             A_long, A_char*);
using DoesKeyExist = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                     const A_char*, A_Boolean*);
using GetNumKeys = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*, A_long*);
using GetValueKeyByIndex = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                           A_long, A_long, A_char*);
using GetDataHandle = A_Err(__cdecl*)(AEGP_PluginID, AEGP_PersistentBlobH,
                                      const A_char*, const A_char*,
                                      AEGP_MemHandle, AEGP_MemHandle*);
using GetData = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                const A_char*, A_u_long, const void*, void*);
using GetString = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                  const A_char*, const A_char*, A_u_long,
                                  A_char*, A_u_long*);
using GetLong = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                const A_char*, A_long, A_long*);
using GetFpLong = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                  const A_char*, A_FpLong, A_FpLong*);
using SetDataHandle = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                      const A_char*, const AEGP_MemHandle);
using SetData = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                const A_char*, A_u_long, const void*);
using SetString = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                  const A_char*, const A_char*);
using SetLong = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                const A_char*, A_long);
using SetFpLong = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                  const A_char*, A_FpLong);
using DeleteEntry = A_Err(__cdecl*)(AEGP_PersistentBlobH, const A_char*,
                                    const A_char*);
using GetPrefsDirectory = A_Err(__cdecl*)(AEGP_MemHandle*);

// Slot order is the declaration order in `AEGP_PersistentDataSuite3`. Every
// slot is implemented, so this table has no diagnostic stub: a caller that
// reaches any slot reaches host code.
struct Suite3 {
  GetApplicationBlob AEGP_GetApplicationBlob;
  GetNumSections AEGP_GetNumSections;
  GetSectionKeyByIndex AEGP_GetSectionKeyByIndex;
  DoesKeyExist AEGP_DoesKeyExist;
  GetNumKeys AEGP_GetNumKeys;
  GetValueKeyByIndex AEGP_GetValueKeyByIndex;
  GetDataHandle AEGP_GetDataHandle;
  GetData AEGP_GetData;
  GetString AEGP_GetString;
  GetLong AEGP_GetLong;
  GetFpLong AEGP_GetFpLong;
  SetDataHandle AEGP_SetDataHandle;
  SetData AEGP_SetData;
  SetString AEGP_SetString;
  SetLong AEGP_SetLong;
  SetFpLong AEGP_SetFpLong;
  DeleteEntry AEGP_DeleteEntry;
  GetPrefsDirectory AEGP_GetPrefsDirectory;
};

static_assert(sizeof(Suite3) == 18 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetApplicationBlob) == 0);
static_assert(offsetof(Suite3, AEGP_GetNumSections) == 1 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetSectionKeyByIndex) == 2 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_DoesKeyExist) == 3 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetNumKeys) == 4 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetValueKeyByIndex) == 5 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetDataHandle) == 6 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetData) == 7 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetString) == 8 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetLong) == 9 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetFpLong) == 10 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_SetDataHandle) == 11 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_SetData) == 12 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_SetString) == 13 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_SetLong) == 14 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_SetFpLong) == 15 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_DeleteEntry) == 16 * sizeof(void*));
static_assert(offsetof(Suite3, AEGP_GetPrefsDirectory) == 17 * sizeof(void*));

const void* provide_suite3(void*) noexcept;

// How the blob was driven, and every refusal. A kind mismatch and a refused
// call are the two ways this host can answer a plug-in differently from AE, so
// each is counted here and, when AEXCOMPAT_EXTENDED_DIAG is set, written to
// stderr as it happens. The counters are not in the worker report: nothing
// downstream decides anything on them, and the per-call stderr line is what
// makes a diverging preferences round-trip reproducible.
struct Telemetry {
  uint32_t get_calls{};
  uint32_t set_calls{};
  uint32_t delete_calls{};
  uint32_t defaults_written{};
  uint32_t kind_mismatches{};
  uint32_t rejected_calls{};
  uint32_t prefs_directory_calls{};
  uint32_t prefs_directory_unavailable{};
  uint32_t sections{};
  uint32_t keys{};
  // Key plus value bytes, matching what `kMaxBlobBytes` bounds.
  uint64_t stored_bytes{};
};

Telemetry telemetry() noexcept;
void reset_for_selftest() noexcept;
bool selftest();

}  // namespace aexcompat::worker_runtime::persistent_data
