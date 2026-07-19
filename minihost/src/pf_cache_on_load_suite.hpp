#pragma once

#include <cstdint>

namespace aexcompat::suites {

using SetNoCacheOnLoad = int32_t(__cdecl*)(void*, int32_t);

struct PfCacheOnLoadSuite1 {
  SetNoCacheOnLoad set_no_cache_on_load;
};

void configure_cache_on_load_suite(void* owned_effect_ref) noexcept;
PfCacheOnLoadSuite1& cache_on_load_suite() noexcept;

}  // namespace aexcompat::suites
