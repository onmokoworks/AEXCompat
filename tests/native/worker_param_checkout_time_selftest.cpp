// Hosted temporal parameter checkout behavior. WIDE_TIME_INPUT describes cache
// dependencies; it is not permission to request a value at another time.

#include "render_subsystem.h"
#include "worker_classic_runtime.hpp"
#include "worker_param_checkout_runtime.hpp"
#include "worker_parameter_runtime.hpp"

#include <array>
#include <cmath>
#include <cstring>
#include <cstdio>

namespace params = aexcompat::worker_runtime::parameters;

// worker_classic_runtime.cpp is linked for active_context()/dispatch_active(),
// which checkout_param consults; its own dispatch entry point is never reached
// from here, so the render subsystem is stubbed instead of linked.
namespace aexcompat::render {
int dispatch(RenderContext&) { return -1; }
}  // namespace aexcompat::render

namespace {

int failures = 0;

void check(bool condition, const char* what) {
  if (condition) return;
  std::fprintf(stderr, "FAIL: %s\n", what);
  ++failures;
}

constexpr int32_t kSlot = 3;
constexpr uint32_t kScale = 30;

// One definition in the hosted ledger, so a checkout that clears the time gate
// has something to answer with. Without this every checkout fails for a second,
// unrelated reason and the test would pass for the wrong cause.
void seed_definition() {
  auto& state = params::state();
  auto& ledger = state.checkout;
  state.records.assign(kSlot, {});
  state.records[kSlot - 1].type = 1;
  state.timelines.clear();
  ledger.definitions.clear();
  ledger.definitions[kSlot] = {};
  ledger.live.clear();
  ledger.checkout_calls = 0;
  ledger.checkin_calls = 0;
  ledger.rejected_temporal = 0;
}

int32_t checkout_at(int32_t what_time, params::Definition* result = nullptr) {
  params::Definition definition{};
  const int32_t status =
      aexcompat::l2_detail::checkout_param(nullptr, kSlot, what_time, 1, kScale,
                                          definition.data());
  if (result) *result = definition;
  return status;
}

// The frame the host is actually rendering is the one that must be answerable.
void the_frames_own_time_is_answered() {
  for (const int32_t frame : {0, 1, 34, -7}) {
    seed_definition();
    aexcompat::l2_detail::configure_hosted_checkout_time(frame, kScale, false);
    char message[96];
    std::snprintf(message, sizeof(message), "a checkout at the frame's own time %d succeeds",
                  frame);
    check(checkout_at(frame) == 0, message);
  }
}

void another_time_is_answered_without_wide_time() {
  seed_definition();
  aexcompat::l2_detail::configure_hosted_checkout_time(34, kScale, false);
  params::Definition result{};
  check(checkout_at(35, &result) == 0, "a static checkout at another time succeeds");
  check(params::state().checkout.rejected_temporal == 0, "the checkout is not refused");
  check(aexcompat::l2_detail::checkin_param(nullptr, result.data()) == 0,
        "the static temporal checkout checks in");
  check(aexcompat::l2_detail::param_checkouts_balanced(),
        "the static temporal checkout ledger balances");
}

void wide_time_admits_another_time() {
  seed_definition();
  aexcompat::l2_detail::configure_hosted_checkout_time(34, kScale, true);
  check(checkout_at(35) == 0, "wide time admits another time");
  check(params::state().checkout.rejected_temporal == 0, "nothing is counted as refused");
}

void an_unconfigured_ledger_still_answers_static_values() {
  seed_definition();
  auto& ledger = params::state().checkout;
  ledger.current_time = 0;
  ledger.current_time_scale = 1;
  ledger.wide_time_allowed = false;
  check(checkout_at(0) == 0, "the default ledger answers t=0");
  check(checkout_at(1) == 0, "the default ledger answers a static value at t=1");
}

void timeline_is_evaluated_at_requested_time() {
  seed_definition();
  params::state().records[kSlot - 1].type = 10;
  aexcompat::parameter_animation::ParameterTimeline timeline{};
  timeline.slot = kSlot;
  timeline.keys.push_back({34, kScale, false,
      aexcompat::parameter_animation::AnimationValueKind::Scalar, 10.0});
  timeline.keys.push_back({36, kScale, false,
      aexcompat::parameter_animation::AnimationValueKind::Scalar, 20.0});
  params::state().timelines.push_back(timeline);
  params::Definition result{};
  check(checkout_at(35, &result) == 0, "animated checkout succeeds without wide time");
  double value{};
  std::memcpy(&value, result.data() + 56, sizeof(value));
  check(std::abs(value - 15.0) < 0.0001,
        "animated checkout returns the requested-time value");
  check(aexcompat::l2_detail::checkin_param(nullptr, result.data()) == 0,
        "the temporal checkout checks in");
  check(aexcompat::l2_detail::param_checkouts_balanced(),
        "the temporal checkout ledger balances");
}

void input_layer_slot_zero_remains_answerable() {
  seed_definition();
  auto& ledger = params::state().checkout;
  ledger.definitions[0] = {};
  params::Definition result{};
  check(aexcompat::l2_detail::checkout_param(nullptr, 0, 35, 1, kScale,
                                              result.data()) == 0,
        "input layer slot zero is answered at another time");
  check(aexcompat::l2_detail::checkin_param(nullptr, result.data()) == 0,
        "input layer slot zero checks in");
  check(aexcompat::l2_detail::param_checkouts_balanced(),
        "input layer slot zero balances");
}

void classic_static_value_is_answered_without_wide_time() {
  using aexcompat::worker_runtime::classic::Context;
  Context context;
  context.configure_checkout_time(34, kScale, false, false);
  aexcompat::worker_runtime::classic::ParameterDefinition hosted{};
  hosted[0] = std::byte{0x5a};
  context.set_definition(kSlot, hosted);
  params::Definition result{};
  check(aexcompat::l2_detail::checkout_param(nullptr, kSlot, 35, 1, kScale,
                                              result.data()) == 0,
        "classic static checkout at another time succeeds without wide time");
  check(result[0] == std::byte{0x5a},
        "classic temporal checkout copies the hosted definition");
  check(aexcompat::l2_detail::checkin_param(nullptr, result.data()) == 0,
        "classic temporal checkout checks in");
  check(aexcompat::l2_detail::param_checkouts_balanced(),
        "classic temporal checkout ledger balances");
}

// AE-shipped effects pass a zero step (CycoreFXHD RipplePulse through this
// callback; ForceMB/WideTime through pre_checkout_layer) and render in AE, so
// zero is a request shape the host answers (issue #1052). Negative remains
// nonsense and fails closed.
void zero_time_step_is_accepted_negative_is_refused() {
  seed_definition();
  aexcompat::l2_detail::configure_hosted_checkout_time(0, kScale, false);
  params::Definition result{};
  check(aexcompat::l2_detail::checkout_param(nullptr, kSlot, 0, 0, kScale,
                                              result.data()) == 0,
        "a checkout with time_step zero succeeds");
  check(aexcompat::l2_detail::checkin_param(nullptr, result.data()) == 0,
        "the zero-step checkout checks in");
  check(aexcompat::l2_detail::checkout_param(nullptr, kSlot, 0, -1, kScale,
                                              result.data()) == 4,
        "a checkout with a negative time_step fails closed");
  check(aexcompat::l2_detail::param_checkouts_balanced(),
        "the zero-step ledger balances");
}

void classic_zero_configured_scale_fails_closed() {
  using aexcompat::worker_runtime::classic::Context;
  Context context;
  context.configure_checkout_time(0, 0, false, false);
  aexcompat::worker_runtime::classic::ParameterDefinition hosted{};
  context.set_definition(kSlot, hosted);
  params::Definition result{};
  check(aexcompat::l2_detail::checkout_param(nullptr, kSlot, 35, 1, kScale,
                                              result.data()) == 4,
        "classic zero configured scale fails closed");
  check(context.checkouts_balanced(),
        "failed classic checkout leaves the ledger balanced");
}

}  // namespace

int main() {
  the_frames_own_time_is_answered();
  another_time_is_answered_without_wide_time();
  wide_time_admits_another_time();
  an_unconfigured_ledger_still_answers_static_values();
  timeline_is_evaluated_at_requested_time();
  input_layer_slot_zero_remains_answerable();
  zero_time_step_is_accepted_negative_is_refused();
  classic_static_value_is_answered_without_wide_time();
  classic_zero_configured_scale_fails_closed();
  if (failures == 0) std::printf("{\"param_checkout_time_selftest\":\"passed\"}\n");
  return failures == 0 ? 0 : 1;
}
