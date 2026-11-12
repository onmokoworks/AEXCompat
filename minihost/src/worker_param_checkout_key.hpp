#pragma once

#include <cstdint>
#include <utility>

namespace aexcompat::worker_runtime {
// Never dereference a checkin pointer. Retired keys describe completed callbacks,
// not owned resources, and must be discarded at the next effect/case boundary.
using ParamCheckoutKey = std::pair<std::uintptr_t, std::uintptr_t>;
inline ParamCheckoutKey param_checkout_key(void* effect, void* definition) {
  return {reinterpret_cast<std::uintptr_t>(effect),
          reinterpret_cast<std::uintptr_t>(definition)};
}
}
