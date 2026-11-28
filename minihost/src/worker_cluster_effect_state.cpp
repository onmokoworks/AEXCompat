#include "worker_cluster_effect_state.hpp"

#include "worker_compute_cache_suite.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_ui_event_execution.hpp"

#include <iostream>
#include <mutex>
#include <string>

namespace aexcompat::l2_detail {
bool configure_mask_scene(const std::string&);
}

namespace aexcompat::worker_runtime::cluster_effect_state {

void reset() {
  auto& state = parameters::state();
  state.records.clear();
  aexcompat::compute_cache::purge_registry();
  state.timelines.clear();
  state.animation_layer_width = 0;
  state.animation_layer_height = 0;
  {
    std::lock_guard<std::mutex> lock(state.keyframe_checkout_mutex);
    state.keyframe_checkout_ledger.clear();
  }
  state.arbitrary = parameters::ArbitraryTelemetry{};
  state.ui = parameters::UiState{};
  state.user_changed_parameters.clear();
  {
    auto& checkout = state.checkout;
    std::lock_guard<std::mutex> lock(checkout.mutex);
    const parameters::CheckoutState fresh;
    checkout.definitions.clear();
    checkout.live.clear();
    checkout.completed.clear();
    checkout.checkout_calls = fresh.checkout_calls;
    checkout.checkin_calls = fresh.checkin_calls;
    checkout.automatic_checkins = fresh.automatic_checkins;
    checkout.invalid_checkins = fresh.invalid_checkins;
    checkout.redundant_checkins = fresh.redundant_checkins;
    checkout.rejected_temporal = fresh.rejected_temporal;
    checkout.wide_time_allowed = fresh.wide_time_allowed;
    checkout.current_time = fresh.current_time;
    checkout.current_time_scale = fresh.current_time_scale;
    checkout.last_index = fresh.last_index;
    checkout.last_time = fresh.last_time;
    checkout.last_time_step = fresh.last_time_step;
    checkout.last_time_scale = fresh.last_time_scale;
  }
  ui_event_execution::custom_ui_telemetry() =
      ui_event_execution::CustomUiTelemetry{};
  ui_event_execution::reset_info_text_telemetry();
  aexcompat::pf_state_runtime::reset_pf_state_statistics();
}

bool reset_member_mask_scene() {
  if (aexcompat::l2_detail::configure_mask_scene("empty")) return true;
  std::cerr << "stage:cluster_swap step=mask_scene_reset\n" << std::flush;
  return false;
}

}  // namespace aexcompat::worker_runtime::cluster_effect_state
