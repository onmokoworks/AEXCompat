#pragma once

#include <mutex>
#include <utility>

namespace aexcompat::worker_runtime::pica_component_lock {

enum class LoadDecision { UseMapped, StartLoad, InFlight, Absent };

struct LoadState {
  bool attempted{};
  bool in_flight{};
  unsigned attempts{};
};

inline LoadDecision decide(const LoadState& state, bool mapped) noexcept {
  if (state.in_flight) return LoadDecision::InFlight;
  if (mapped) return LoadDecision::UseMapped;
  if (!state.attempted) return LoadDecision::StartLoad;
  return LoadDecision::Absent;
}

inline void begin_load(LoadState& state) noexcept {
  state.attempted = true;
  state.in_flight = true;
  ++state.attempts;
}

inline void finish_load(LoadState& state) noexcept { state.in_flight = false; }

// The caller records its in-flight latch before entering this helper.  Only
// the loader call is outside the component mutex; state inspection and the
// foreign initialization calls on either side remain serialized.
template <class Load, class Observe, class Finish>
auto load_outside_component_lock(
    std::unique_lock<std::recursive_mutex>& component_lock, Load&& load,
    Observe&& observe, Finish&& finish)
    -> decltype(std::forward<Load>(load)()) {
  struct RelockOnExit {
    std::unique_lock<std::recursive_mutex>& lock;
    ~RelockOnExit() {
      if (!lock.owns_lock()) lock.lock();
    }
  } relock{component_lock};
  component_lock.unlock();
  auto loaded = std::forward<Load>(load)();
  component_lock.lock();
  auto observed = std::forward<Observe>(observe)();
  auto result = observed ? observed : loaded;
  std::forward<Finish>(finish)(result);
  return result;
}

}  // namespace aexcompat::worker_runtime::pica_component_lock
