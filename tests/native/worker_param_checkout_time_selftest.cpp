// The hosted parameter ledger's time gate (issue #828).
//
// `checkout_param` refuses a checkout whose time is not the frame's, unless the
// plug-in advertised wide time input. The smart path serves its checkouts from
// this hosted ledger rather than from a classic dispatch context, and nothing
// ever set the ledger's frame time: it kept `current_time = 0`,
// `current_time_scale = 1`, so the comparison
//
//     what_time * current_time_scale == current_time * time_scale
//
// admitted t=0 (0 == 0) and refused every other time (t*1 != 0*scale). Every
// SmartFX frame past t=0 therefore had its first parameter checkout answered
// with 4, and the plug-in returned that as PF_Err_OUT_OF_MEMORY. AviUtl2 renders
// at the timeline cursor, so no smart effect worked anywhere but frame 0.
//
// These cases pin the gate itself, in both directions: what the ledger answers
// once it is configured, and what a ledger left at its defaults answers. They do
// not reach smart_render_runtime, so they cannot tell whether anything calls
// configure_hosted_checkout_time - that is what
// tests/test_smart_param_checkout_time_worker.py renders a real plug-in for.

#include "render_subsystem.h"
#include "worker_param_checkout_runtime.hpp"
#include "worker_parameter_runtime.hpp"

#include <array>
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
  auto& ledger = params::state().checkout;
  ledger.definitions.clear();
  ledger.definitions[kSlot] = {};
  ledger.live.clear();
  ledger.checkout_calls = 0;
  ledger.checkin_calls = 0;
  ledger.rejected_temporal = 0;
}

int32_t checkout_at(int32_t what_time) {
  std::array<std::byte, params::kDefinitionSize> definition{};
  return aexcompat::l2_detail::checkout_param(nullptr, kSlot, what_time, 1, kScale,
                                              definition.data());
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

// The gate still refuses another time - dropping it would let a plug-in read
// parameters the host never evaluated for this frame.
void another_time_is_refused_without_wide_time() {
  seed_definition();
  aexcompat::l2_detail::configure_hosted_checkout_time(34, kScale, false);
  check(checkout_at(35) == 4, "a checkout at another time is refused");
  check(params::state().checkout.rejected_temporal == 1, "the refusal is counted");
  check(checkout_at(34) == 0, "the frame's own time still succeeds after a refusal");
}

// ...and admits it when the plug-in advertised wide time input.
void wide_time_admits_another_time() {
  seed_definition();
  aexcompat::l2_detail::configure_hosted_checkout_time(34, kScale, true);
  check(checkout_at(35) == 0, "wide time admits another time");
  check(params::state().checkout.rejected_temporal == 0, "nothing is counted as refused");
}

// The regression itself: a ledger left at its defaults answers only t=0. This is
// what the smart path saw before #828, and what it must never see again.
void an_unconfigured_ledger_answers_only_time_zero() {
  seed_definition();
  auto& ledger = params::state().checkout;
  ledger.current_time = 0;
  ledger.current_time_scale = 1;
  ledger.wide_time_allowed = false;
  check(checkout_at(0) == 0, "the default ledger answers t=0");
  check(checkout_at(1) == 4, "the default ledger refuses t=1 - the #828 symptom");
  // Configuring it is what makes the same frame answerable.
  aexcompat::l2_detail::configure_hosted_checkout_time(1, kScale, false);
  check(checkout_at(1) == 0, "configuring the ledger makes t=1 answerable");
}

// A zero ledger scale reduces the comparison to `0 == current_time * time_scale`,
// which is true for every requested time once the frame sits at 0 - so left alone
// it turns the gate off exactly where it looks harmless. It must fail closed
// instead, including for the frame's own time.
void a_zero_scale_admits_nothing() {
  seed_definition();
  aexcompat::l2_detail::configure_hosted_checkout_time(0, 0, false);
  check(checkout_at(5) == 4, "a zero scale at t=0 does not admit another time");
  check(checkout_at(0) == 4, "a zero scale admits nothing, not even t=0");
  seed_definition();
  aexcompat::l2_detail::configure_hosted_checkout_time(34, 0, false);
  check(checkout_at(35) == 4, "a zero scale away from t=0 refuses another time");
  check(checkout_at(34) == 4, "a zero scale away from t=0 refuses its own time");
  // Wide time still outranks the gate, as it does for any other refusal.
  seed_definition();
  aexcompat::l2_detail::configure_hosted_checkout_time(0, 0, true);
  check(checkout_at(5) == 0, "wide time still admits a checkout under a zero scale");
}

}  // namespace

int main() {
  the_frames_own_time_is_answered();
  another_time_is_refused_without_wide_time();
  wide_time_admits_another_time();
  an_unconfigured_ledger_answers_only_time_zero();
  a_zero_scale_admits_nothing();
  if (failures == 0) std::printf("{\"param_checkout_time_selftest\":\"passed\"}\n");
  return failures == 0 ? 0 : 1;
}
