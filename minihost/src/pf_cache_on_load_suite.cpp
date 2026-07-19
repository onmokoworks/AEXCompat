#include "pf_cache_on_load_suite.hpp"

#include <atomic>
#include <cstddef>

namespace aexcompat::suites {
namespace {

constexpr int32_t kPfBadCallbackParam = 516;
std::atomic<void*> g_owned_effect_ref{};
std::atomic<bool> g_no_cache_on_load{};
std::atomic<uint64_t> g_no_cache_on_load_updates{};

int32_t __cdecl set_no_cache_on_load(void* effect_ref, int32_t effect_available) {
  const void* owned_effect_ref =
      g_owned_effect_ref.load(std::memory_order_acquire);
  if (!owned_effect_ref || effect_ref != owned_effect_ref ||
      (effect_available != 0 && effect_available != 1))
    return kPfBadCallbackParam;
  // The minihost has no persistent startup plug-in cache; retain the policy so
  // every worker observes the same explicit host decision.
  g_no_cache_on_load.store(effect_available != 0, std::memory_order_release);
  g_no_cache_on_load_updates.fetch_add(1, std::memory_order_relaxed);
  return 0;
}

PfCacheOnLoadSuite1 g_cache_on_load_suite1{&set_no_cache_on_load};

}  // namespace

static_assert(sizeof(PfCacheOnLoadSuite1) == sizeof(void*));
static_assert(offsetof(PfCacheOnLoadSuite1, set_no_cache_on_load) == 0);

void configure_cache_on_load_suite(void* owned_effect_ref) noexcept {
  g_owned_effect_ref.store(owned_effect_ref, std::memory_order_release);
}

PfCacheOnLoadSuite1& cache_on_load_suite() noexcept {
  return g_cache_on_load_suite1;
}

}  // namespace aexcompat::suites
