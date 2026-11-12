#pragma once

#include <cstddef>

namespace aexcompat::worker_runtime::parameters {

inline constexpr std::size_t kMaxParameterCount = 4096;
// Mirrored by broker parameter_limits.rs; aggregate budget, not a per-value limit.
inline constexpr std::size_t kMaxParameterPayloadBytes = kMaxParameterCount * 256;
inline constexpr std::size_t kMaxParameterMessageBytes = kMaxParameterPayloadBytes + 64 * 1024;

}  // namespace aexcompat::worker_runtime::parameters
