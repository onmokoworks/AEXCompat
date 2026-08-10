#include "worker_param_checkout_runtime.hpp"

#include "worker_classic_runtime.hpp"
#include "worker_callback_diagnostics.hpp"
#include "worker_extended_diag.hpp"
#include "worker_parameter_runtime.hpp"

#include <cstddef>
#include <cstring>
#include <mutex>

namespace aexcompat::l2_detail {

// Checkout ledger state read and written through its owner,
// aexcompat::worker_runtime::parameters::state() (issue #126 Phase D); these
// references keep the g_* spellings.
namespace {
constexpr std::size_t kParamSize =
    aexcompat::worker_runtime::parameters::kDefinitionSize;
auto& g_parameter_runtime = aexcompat::worker_runtime::parameters::state();
auto& g_checkout_layer_definitions = g_parameter_runtime.checkout.definitions;
auto& g_param_checkout_mutex = g_parameter_runtime.checkout.mutex;
auto& g_live_param_checkouts = g_parameter_runtime.checkout.live;
auto& g_param_checkout_calls = g_parameter_runtime.checkout.checkout_calls;
auto& g_param_checkin_calls = g_parameter_runtime.checkout.checkin_calls;
auto& g_automatic_param_checkins = g_parameter_runtime.checkout.automatic_checkins;
auto& g_invalid_param_checkins = g_parameter_runtime.checkout.invalid_checkins;
auto& g_rejected_temporal_param_checkouts = g_parameter_runtime.checkout.rejected_temporal;
auto& g_wide_time_checkout_allowed = g_parameter_runtime.checkout.wide_time_allowed;
auto& g_checkout_current_time = g_parameter_runtime.checkout.current_time;
auto& g_checkout_current_time_scale = g_parameter_runtime.checkout.current_time_scale;
auto& g_last_param_checkout_index = g_parameter_runtime.checkout.last_index;
auto& g_last_param_checkout_time = g_parameter_runtime.checkout.last_time;
auto& g_last_param_checkout_time_step = g_parameter_runtime.checkout.last_time_step;
auto& g_last_param_checkout_time_scale = g_parameter_runtime.checkout.last_time_scale;

int32_t finish_param_callback(aexcompat::callback_diagnostics::Callback callback,
                              int32_t result,
                              aexcompat::callback_diagnostics::Reason reason =
                                  aexcompat::callback_diagnostics::Reason::None) {
  aexcompat::callback_diagnostics::record(callback, result, reason);
  if (extended_diag_enabled()) {
    std::cerr << "extended_diag:"
              << aexcompat::callback_diagnostics::CALLBACK_NAMES[
                     static_cast<std::size_t>(callback)]
              << " -> " << result;
    if (result != 0)
      std::cerr << " ("
                << aexcompat::callback_diagnostics::REASON_NAMES[
                       static_cast<std::size_t>(reason)]
                << ')';
    std::cerr << '\n' << std::flush;
  }
  return result;
}
}  // namespace

int32_t __cdecl checkout_param(void*, int32_t index, int32_t what_time, int32_t time_step,
                               uint32_t time_scale, void* definition) {
  using aexcompat::callback_diagnostics::Callback;
  using aexcompat::callback_diagnostics::Reason;
  if (extended_diag_enabled())
    std::cerr << "extended_diag:checkout_param index=" << index
              << " time=" << what_time << "/" << time_scale << "\n"
              << std::flush;
  if (!definition || time_step <= 0 || time_scale == 0) {
    return finish_param_callback(Callback::CheckoutParam, 4, Reason::InvalidArguments);
  }
  auto* classic_context = aexcompat::worker_runtime::classic::active_context();
  if (!classic_context && aexcompat::worker_runtime::classic::dispatch_active())
    return finish_param_callback(Callback::CheckoutParam, 4, Reason::NoActiveState);
  if (classic_context &&
      !classic_context->checkout_time_allowed(what_time, time_scale))
    return finish_param_callback(Callback::CheckoutParam, 4,
                                 Reason::InvalidArguments);
  const auto record_checkout = [&] {
    if (classic_context) {
      classic_context->record_checkout(definition, index, what_time, time_step, time_scale);
      return;
    }
    std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
    ++g_live_param_checkouts[definition];
    ++g_param_checkout_calls;
    g_last_param_checkout_index = index;
    g_last_param_checkout_time = what_time;
    g_last_param_checkout_time_step = time_step;
    g_last_param_checkout_time_scale = time_scale;
  };
  if (classic_context && classic_context->copy_timed_layer(
          index, what_time, time_scale, definition, kParamSize)) {
    record_checkout();
    return finish_param_callback(Callback::CheckoutParam, 0);
  }
  if (classic_context && classic_context->has_timed_slot(index))
    return finish_param_callback(Callback::CheckoutParam, 4, Reason::UnknownLayer);
  if (classic_context) {
    if (classic_context->copy_definition(index, definition, kParamSize) ||
        classic_context->copy_fallback_definition(index, definition, kParamSize)) {
      record_checkout();
      return finish_param_callback(Callback::CheckoutParam, 0);
    }
    return finish_param_callback(Callback::CheckoutParam, 4, Reason::UnknownLayer);
  }
  const auto hosted = g_checkout_layer_definitions.find(index);
  if (hosted != g_checkout_layer_definitions.end()) {
    aexcompat::worker_runtime::parameters::Definition evaluated{};
    if (!aexcompat::worker_runtime::parameters::copy_definition_at_time(
            index, what_time, time_scale, hosted->second, evaluated)) {
      ++g_rejected_temporal_param_checkouts;
      return finish_param_callback(
          Callback::CheckoutParam, 4, Reason::TemporalCheckoutDenied);
    }
    std::memcpy(definition, evaluated.data(), evaluated.size());
    record_checkout();
    return finish_param_callback(Callback::CheckoutParam, 0);
  }
  return finish_param_callback(Callback::CheckoutParam, 4, Reason::UnknownLayer);
}

void configure_hosted_checkout_time(int32_t current_time, uint32_t time_scale,
                                    bool wide_time_allowed) noexcept {
  // Retain the frame and WIDE_TIME_INPUT declaration for diagnostics and cache
  // policy. The flag describes temporal dependencies; it is not permission to
  // call checkout_param at another time.
  g_checkout_current_time = current_time;
  g_checkout_current_time_scale = time_scale;
  g_wide_time_checkout_allowed = wide_time_allowed;
}

int32_t __cdecl checkin_param(void*, void* definition) {
  using aexcompat::callback_diagnostics::Callback;
  using aexcompat::callback_diagnostics::Reason;
  if (auto* context = aexcompat::worker_runtime::classic::active_context()) {
    const int32_t result = context->checkin(definition);
    return finish_param_callback(Callback::CheckinParam, result,
        result == 0 ? Reason::None : Reason::NotCheckedOut);
  }
  if (aexcompat::worker_runtime::classic::dispatch_active())
    return finish_param_callback(Callback::CheckinParam, 4, Reason::NoActiveState);
  std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
  if (!definition || g_live_param_checkouts.empty()) {
    ++g_invalid_param_checkins;
    return finish_param_callback(Callback::CheckinParam, 4, Reason::NotCheckedOut);
  }
  auto found = g_live_param_checkouts.find(definition);
  // PF_ParamDef is a value type. Wrappers may move the checked-out value before
  // checkin, so its address is not a stable checkout identity.
  if (found == g_live_param_checkouts.end()) found = g_live_param_checkouts.begin();
  if (--found->second == 0) g_live_param_checkouts.erase(found);
  ++g_param_checkin_calls;
  return finish_param_callback(Callback::CheckinParam, 0);
}

bool param_checkouts_balanced() {
  if (auto* context = aexcompat::worker_runtime::classic::active_context())
    return context->checkouts_balanced();
  std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
  return g_live_param_checkouts.empty() && g_param_checkout_calls == g_param_checkin_calls &&
      g_invalid_param_checkins == 0;
}

void automatic_checkin_pre_render_params() {
  if (auto* context = aexcompat::worker_runtime::classic::active_context()) {
    context->automatic_checkin();
    return;
  }
  std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
  uint32_t checkout_count = 0;
  for (const auto& checkout : g_live_param_checkouts) checkout_count += checkout.second;
  g_automatic_param_checkins += checkout_count;
  g_param_checkin_calls += checkout_count;
  g_live_param_checkouts.clear();
}

}  // namespace aexcompat::l2_detail
