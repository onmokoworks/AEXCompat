#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <type_traits>

namespace aexcompat::worker_runtime::compute_cache {

// Public AEGP Compute Cache Suite v1 ABI. These signatures mirror the Adobe
// SDK declarations (AEGP_CCComputeClassIdP is const char*, the two refcons and
// the checkout receipt are void*, AEGP_GUID is four A_long values, and the
// wait flag is a C++ bool).
using A_Err = int32_t;
using A_long = int32_t;
using AEGP_CCComputeClassIdP = const char*;
using AEGP_CCComputeOptionsRefconP = void*;
using AEGP_CCComputeValueRefconP = void*;
using AEGP_CCCheckoutReceiptP = void*;

struct AEGP_GUID {
  A_long bytes[4];
};
using AEGP_CCComputeKey = AEGP_GUID;
using AEGP_CCComputeKeyP = AEGP_CCComputeKey*;

using GenerateKey = A_Err(__cdecl*)(AEGP_CCComputeOptionsRefconP,
                                    AEGP_CCComputeKeyP);
using Compute = A_Err(__cdecl*)(AEGP_CCComputeOptionsRefconP,
                                AEGP_CCComputeValueRefconP*);
using ApproxSizeValue = std::size_t(__cdecl*)(AEGP_CCComputeValueRefconP);
using DeleteComputeValue = void(__cdecl*)(AEGP_CCComputeValueRefconP);

struct AEGP_ComputeCacheCallbacks {
  GenerateKey generate_key;
  Compute compute;
  ApproxSizeValue approx_size_value;
  DeleteComputeValue delete_compute_value;
};

using ClassRegister = A_Err(__cdecl*)(
    AEGP_CCComputeClassIdP, const AEGP_ComputeCacheCallbacks*);
using ClassUnregister = A_Err(__cdecl*)(AEGP_CCComputeClassIdP);
using ComputeIfNeededAndCheckout = A_Err(__cdecl*)(
    AEGP_CCComputeClassIdP, AEGP_CCComputeOptionsRefconP, bool,
    AEGP_CCCheckoutReceiptP*);
using CheckoutCached = A_Err(__cdecl*)(
    AEGP_CCComputeClassIdP, AEGP_CCComputeOptionsRefconP,
    AEGP_CCCheckoutReceiptP*);
using GetReceiptComputeValue = A_Err(__cdecl*)(
    AEGP_CCCheckoutReceiptP, AEGP_CCComputeValueRefconP*);
using CheckinComputeReceipt = A_Err(__cdecl*)(AEGP_CCCheckoutReceiptP);

struct AEGP_ComputeCacheSuite1 {
  ClassRegister AEGP_ClassRegister;
  ClassUnregister AEGP_ClassUnregister;
  ComputeIfNeededAndCheckout AEGP_ComputeIfNeededAndCheckout;
  CheckoutCached AEGP_CheckoutCached;
  GetReceiptComputeValue AEGP_GetReceiptComputeValue;
  CheckinComputeReceipt AEGP_CheckinComputeReceipt;
};

inline constexpr char kSuiteName[] = "AEGP Compute Cache";
inline constexpr int32_t kSuiteVersion1 = 1;
inline constexpr A_Err kErrNone = 0;
inline constexpr A_Err kErrGeneric = 1;
inline constexpr A_Err kErrStruct = 2;
inline constexpr A_Err kErrParameter = 3;
inline constexpr A_Err kErrAlloc = 4;
inline constexpr A_Err kErrNotInCacheOrComputePending = 22;

inline constexpr std::size_t kMaxClassIdBytes = 1024;
inline constexpr std::size_t kMaxClasses = 128;
inline constexpr std::size_t kMaxEntries = 4096;
inline constexpr std::size_t kMaxActiveReceipts = 8192;
inline constexpr std::size_t kMaxReceiptTokens = 65536;
inline constexpr std::size_t kMaxValueBytes = 256u * 1024u * 1024u;
inline constexpr std::size_t kMaxTotalValueBytes = 512u * 1024u * 1024u;
inline constexpr std::size_t kMaxTelemetryRecords = 128;

static_assert(sizeof(A_long) == 4);
static_assert(sizeof(AEGP_GUID) == 4 * sizeof(A_long));
static_assert(sizeof(AEGP_ComputeCacheCallbacks) == 4 * sizeof(void*));
static_assert(sizeof(AEGP_ComputeCacheSuite1) == 6 * sizeof(void*));
static_assert(offsetof(AEGP_ComputeCacheSuite1, AEGP_ClassRegister) == 0);
static_assert(offsetof(AEGP_ComputeCacheSuite1, AEGP_ClassUnregister) ==
              1 * sizeof(void*));
static_assert(
    offsetof(AEGP_ComputeCacheSuite1, AEGP_ComputeIfNeededAndCheckout) ==
    2 * sizeof(void*));
static_assert(offsetof(AEGP_ComputeCacheSuite1, AEGP_CheckoutCached) ==
              3 * sizeof(void*));
static_assert(offsetof(AEGP_ComputeCacheSuite1, AEGP_GetReceiptComputeValue) ==
              4 * sizeof(void*));
static_assert(offsetof(AEGP_ComputeCacheSuite1, AEGP_CheckinComputeReceipt) ==
              5 * sizeof(void*));

const AEGP_ComputeCacheSuite1* suite() noexcept;
const void* provide_suite1(void*) noexcept;

// Called after the plug-in's GLOBAL_SETDOWN selector has returned but before
// its module can be unloaded. Returns false if live receipts or computations
// make cleanup unsafe.
bool teardown_owner_from_entry(const void* entry) noexcept;

void reset_telemetry() noexcept;
std::string telemetry_report_json();

// Native-test isolation. Production teardown is owner-scoped and uses the
// function above.
bool reset_for_selftest() noexcept;

}  // namespace aexcompat::worker_runtime::compute_cache
