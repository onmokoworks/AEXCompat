#pragma once

#include "worker_handle_runtime.hpp"
#include "worker_parameter_execution.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_suite_registry.hpp"
#include "worker_world_registry.hpp"

namespace aexcompat::worker_runtime {

// PARAMS_SETUP declarations (including owned arbitrary DEFAULT handles) belong
// to the loaded effect. They stay alive until global shutdown. Render entry
// initializes fresh definitions and uses ARB_COPY for each instance value;
// this metadata snapshot is never used as an owning instance-value copy.
class FreshCase {
 public:
  using Defaults = std::vector<parameters::ParamRecord>;
  explicit FreshCase(const Defaults& defaults) : defaults_(defaults) {}

  bool begin() {
    auto& state = parameters::state();
    if (!state.checkout.live.empty() || !state.keyframe_checkout_ledger.empty())
      return false;
    state.records = defaults_;
    state.checkout.definitions.clear();
    state.user_changed_parameters.clear();
    state.timelines.clear();
    const bool update = state.ui.update_advertised;
    const bool dynamic = state.ui.dynamic_flags_advertised;
    state.ui = parameters::UiState{};
    state.ui.update_advertised = update;
    state.ui.dynamic_flags_advertised = dynamic;
    pf_state_runtime::reset_effect_lifetime(true);
    const auto handles = handles::statistics();
    handles_ = handles::live_identities();
    invalid_handles_ = handles.invalid_operations;
    invalid_arbitrary_ = state.arbitrary.invalid_operations;
    leases_ = suite_registry().snapshot();
    worlds_ = world_registry::statistics();
    active_ = true;
    return true;
  }

  bool finish() {
    if (!active_) return true;
    active_ = false;
    auto& state = parameters::state();
    const auto handles = handles::statistics();
    const auto worlds = world_registry::statistics();
    const bool clean = state.checkout.live.empty() && state.keyframe_checkout_ledger.empty() &&
        handles::live_identities() == handles_ && handles.invalid_operations == invalid_handles_ &&
        state.arbitrary.invalid_operations == invalid_arbitrary_ &&
        suite_registry().snapshot().live_leases == leases_.live_leases &&
        worlds.live_count == worlds_.live_count && worlds.live_bytes == worlds_.live_bytes &&
        worlds.invalid_operations == worlds_.invalid_operations;
    if (!clean) return false;
    state.records = defaults_;
    state.checkout.definitions.clear();
    state.user_changed_parameters.clear();
    pf_state_runtime::reset_effect_lifetime(true);
    return true;
  }

 private:
  const Defaults& defaults_;
  std::vector<std::uintptr_t> handles_;
  uint32_t invalid_handles_{};
  uint32_t invalid_arbitrary_{};
  suite_runtime::SuiteLeaseSnapshot leases_;
  world_registry::Statistics worlds_;
  bool active_{};
};

}  // namespace aexcompat::worker_runtime
