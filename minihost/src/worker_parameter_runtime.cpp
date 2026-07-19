#include "worker_parameter_runtime.hpp"

#include <algorithm>

namespace aexcompat::worker_runtime::parameters {
namespace {
State g_state;
}

State& state() noexcept { return g_state; }

const parameter_animation::ParameterTimeline* timeline(int32_t slot) noexcept {
  const auto& timelines = g_state.timelines;
  const auto found = std::find_if(timelines.begin(), timelines.end(),
      [slot](const auto& value) { return value.slot == slot; });
  return found == timelines.end() ? nullptr : &*found;
}

}  // namespace aexcompat::worker_runtime::parameters
