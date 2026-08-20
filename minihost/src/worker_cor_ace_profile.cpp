#include "worker_cor_ace_profile.hpp"

#include <windows.h>

#include <cstring>
#include <iostream>
#include <limits>
#include <mutex>
#include <vector>

namespace aexcompat::cor_ace {
namespace {

// Not `extern "C"`: with /EHsc the compiler is entitled to assume a function
// declared `extern "C"` never throws, and this one does (the ACE table guard).
// Same type as the test seam's, so installing one needs no cast.
using MakeProfile = ProfileFactory;

constexpr const char kMakeExport[] = "?Make@COR_ACE_Profile@@SAPEAV1@PEBXI@Z";

struct CacheEntry {
  // Owns the bytes for the process lifetime; see the header on
  // `MakeBufferProfile`. A vector move preserves the heap buffer's address, so
  // growing the cache does not move what ACE may be holding.
  std::vector<std::uint8_t> icc;
  void* profile{};
};

std::mutex g_mutex;
std::vector<CacheEntry> g_cache;
ProfileFactory g_test_factory{};
bool g_reported_created = false;
bool g_reported_synthetic = false;

// The refusal marker the broker's diagnostics parser actually reads
// (`callback_denials`). Without it a fail-closed 4 out of
// `AEGP_GetNewWorkingSpaceColorProfile` reaches the report as an unexplained
// frame error, which is the failure mode the marker exists to kill.
//
// One line per distinct reason per worker process, the same latch
// `pf_world_facade::report_refusal` uses and for the same reason: a plug-in
// that asks for the working-space profile once per frame would otherwise
// stream this line, and the bounded `stderr_tail` the report carries would
// hold nothing else. The parser dedups the (callback, reason) pair anyway, so
// the latch costs the report nothing.
enum class DenialReason {
  icc_size_range,
  no_factory_export,
  cache_full,
  cache_allocation,
  factory_refused,
  factory_threw,
  factory_faulted,
  // Not a reason: the bound the latch below is checked against, so appending a
  // reason keeps the assert honest instead of leaving it pinned to whichever
  // enumerator happened to be last when it was written.
  count,
};

const char* denial_text(DenialReason reason) {
  switch (reason) {
    case DenialReason::icc_size_range: return "icc_size_range";
    case DenialReason::no_factory_export: return "no_factory_export";
    case DenialReason::cache_full: return "cache_full";
    case DenialReason::cache_allocation: return "cache_allocation";
    case DenialReason::factory_refused: return "factory_refused";
    case DenialReason::factory_threw: return "factory_threw";
    case DenialReason::factory_faulted: return "factory_faulted";
    // Not a reason; listed so `-Wswitch` stays quiet without losing the
    // fallthrough below, which is what covers a value from a corrupted cast.
    case DenialReason::count: break;
  }
  return "unknown";
}

static_assert(static_cast<unsigned>(DenialReason::count) <= 32,
              "the latch below is a uint32_t bitmask indexed by this enum");
uint32_t g_reported_denials{};

void deny(DenialReason reason) {
  const uint32_t bit = 1u << static_cast<uint32_t>(reason);
  if ((g_reported_denials & bit) != 0) return;
  g_reported_denials |= bit;
  std::cerr << "stage:callback_denied callback=aegp_color_profile reason="
            << denial_text(reason) << "\n" << std::flush;
}

// COR is pinned on the first successful resolve. The cache hands raw pointers
// into COR's heap to plug-ins and never takes them back, and the function
// pointer below is held only for the duration of one call - but both assume the
// module stays mapped, and `GetModuleHandleW` takes no reference. Pinning turns
// that assumption into a fact. (This host does not unload plug-in images
// either, but COR.dll is not one: it arrives as a dependency and the loader is
// free to drop it.)
MakeProfile resolve_make_profile() {
  HMODULE cor{};
  if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN, L"COR.dll", &cor) || !cor)
    return nullptr;
  return reinterpret_cast<MakeProfile>(GetProcAddress(cor, kMakeExport));
}

// A structured fault is not a C++ exception, so `catch (...)` does not contain
// one. The ICC bytes reach ACE's own parser from a plug-in
// (`AEGP_GetNewColorProfileFromICCProfile`), and this host's own ICC validation
// checks a header, not a parser's worth of structure - so the call gets the
// same `__try` every other undocumented vendor entry point in this worker gets
// (`bravo_call_guarded`, `sweetpea_init_guarded`, `load_library_guarded`).
//
// Split in two because MSVC forbids `__try` and C++ `catch` in one function:
// the SEH frame is the inner one, so a fault is contained before the C++ frame
// ever sees it, and a COR throw passes through the `__try` untouched to the
// `catch` outside.
int seh_filter(EXCEPTION_POINTERS* info) {
  const DWORD code =
      info && info->ExceptionRecord ? info->ExceptionRecord->ExceptionCode : 0;
  // A C++ throw arrives as SEH too; it has to keep going so the `catch (...)`
  // one frame out handles it, or COR's documented failure signal would be
  // swallowed here and reported as a fault.
  if (code == 0xE06D7363u) return EXCEPTION_CONTINUE_SEARCH;
  return EXCEPTION_EXECUTE_HANDLER;
}

// Deliberately not `noexcept`: a COR throw has to pass through this frame to
// the `catch (...)` outside, and `noexcept` would turn that into `terminate`.
void* make_profile_seh_guarded(MakeProfile make, const void* data,
                               unsigned int size, bool& faulted) {
  __try {
    return make(data, size);
  } __except (seh_filter(GetExceptionInformation())) {
    faulted = true;
    return nullptr;
  }
}

void* call_make_profile(MakeProfile make, const void* data, unsigned int size,
                        DenialReason& reason) {
  bool faulted = false;
  try {
    void* profile = make_profile_seh_guarded(make, data, size, faulted);
    if (faulted) reason = DenialReason::factory_faulted;
    return profile;
  } catch (...) {
    reason = DenialReason::factory_threw;
    return nullptr;
  }
}

}  // namespace

void set_profile_factory_for_test(ProfileFactory factory) {
  std::lock_guard<std::mutex> lock(g_mutex);
  g_test_factory = factory;
  g_cache.clear();
  g_reported_created = false;
  g_reported_synthetic = false;
  g_reported_denials = 0;
}

void note_synthetic_handle_issued() {
  std::lock_guard<std::mutex> lock(g_mutex);
  if (g_reported_synthetic) return;
  g_reported_synthetic = true;
  std::cerr << "stage:cor_ace_profile status=synthetic\n" << std::flush;
}

bool profile_factory_available() {
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    if (g_test_factory) return true;
  }
  return resolve_make_profile() != nullptr;
}

void* profile_from_icc(const std::uint8_t* icc, std::size_t size) {
  // One lock for the whole function, held across the factory call. The factory
  // runs at most once per distinct ICC blob, so the serialization costs nothing
  // that matters, and it keeps every path under one obvious rule instead of
  // some inside the lock and some outside it.
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto fail = [](DenialReason reason) -> void* {
    deny(reason);
    return nullptr;
  };
  if (!icc || size == 0 || size > (std::numeric_limits<unsigned int>::max)())
    return fail(DenialReason::icc_size_range);
  const MakeProfile make = g_test_factory ? g_test_factory : resolve_make_profile();
  if (!make) return fail(DenialReason::no_factory_export);

  for (const CacheEntry& entry : g_cache) {
    if (entry.icc.size() != size ||
        std::memcmp(entry.icc.data(), icc, size) != 0)
      continue;
    return entry.profile;
  }
  if (g_cache.size() >= kMaxCachedProfiles) return fail(DenialReason::cache_full);
  try {
    g_cache.push_back(CacheEntry{std::vector<std::uint8_t>(icc, icc + size), nullptr});
  } catch (...) {
    return fail(DenialReason::cache_allocation);
  }
  CacheEntry& entry = g_cache.back();
  DenialReason reason = DenialReason::factory_refused;
  entry.profile = call_make_profile(make, entry.icc.data(),
                                    static_cast<unsigned int>(size), reason);
  if (!entry.profile) {
    // Nothing was built, so nothing can be referencing the bytes.
    g_cache.pop_back();
    return fail(reason);
  }
  if (!g_reported_created) {
    g_reported_created = true;
    std::cerr << "stage:cor_ace_profile status=created\n" << std::flush;
  }
  return entry.profile;
}

}  // namespace aexcompat::cor_ace
