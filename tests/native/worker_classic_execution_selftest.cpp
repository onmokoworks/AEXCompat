// Behavioral self-test for classic_execution's lifecycle/dispatch contract.
//
// These are pure functions over a hook table, so the whole contract can be
// exercised with counting fakes and no plug-in, no worker process, and no AEX.
//
// The property under test is which steps reach the plug-in once its own setup
// has been refused (issue #725). dispatch_render enters the plug-in through
// three hooks, and the split is deliberate:
//
//   draw, dispatch_selector  - skipped, both guarded by `error == 0`
//   close_ui                 - NOT skipped, called unconditionally so a custom
//                              UI context opened earlier still gets closed.
//                              That it is also unbracketed by any stage marker
//                              is issue #735.
//
// prepare_output sits between them and is host-side only.

#include "worker_classic_execution.hpp"

#include <cstdio>

using namespace aexcompat::worker_runtime::classic_execution;

namespace {

// Stands in for RenderLifecycle: `begin` hands it back as an opaque void*, and
// the setup_error accessor is the only way the outcome gets out. `disposed`
// stands in for the heap RenderLifecycle the real begin hook `new`s, which only
// dispose_lifecycle frees - the leak an early return could introduce.
struct FakeLifecycle {
  int32_t setup_error{};
  bool disposed{};
};

struct Host {
  FakeLifecycle lifecycle{};
  bool begin_returns_null{};

  int click_calls{};
  int interpolate_calls{};
  int roundtrip_calls{};
  int conditional_ui_calls{};
  int draw_calls{};
  int prepare_output_calls{};
  int selector_calls{};
  int close_ui_calls{};
  int end_calls{};
  int32_t end_saw_error{-999};

  bool click_result{true};
  int32_t prepare_output_result{};
  int32_t selector_result{};
};

Host& host_of(void* opaque) { return *static_cast<Host*>(opaque); }

const LifecycleHooks& lifecycle_hooks() {
  static const LifecycleHooks value{
      +[](void* opaque) -> void* {
        auto& h = host_of(opaque);
        return h.begin_returns_null ? nullptr : static_cast<void*>(&h.lifecycle);
      },
      +[](void* lifecycle) { return static_cast<FakeLifecycle*>(lifecycle)->setup_error; },
      +[](void* opaque) { auto& h = host_of(opaque); ++h.click_calls; return h.click_result; },
      +[](void* opaque) { ++host_of(opaque).interpolate_calls; return true; },
      +[](void* opaque) { ++host_of(opaque).roundtrip_calls; return true; },
      +[](void* opaque) { ++host_of(opaque).conditional_ui_calls; return true; },
      +[](void* opaque) { ++host_of(opaque).draw_calls; return true; },
      +[](void* opaque, void*, int32_t error) {
        auto& h = host_of(opaque);
        ++h.end_calls;
        h.end_saw_error = error;
        return error; },
      +[](void* lifecycle) { static_cast<FakeLifecycle*>(lifecycle)->disposed = true; }};
  return value;
}

const RenderHooks& render_hooks() {
  static const RenderHooks value{
      +[](void* opaque) { ++host_of(opaque).draw_calls; return true; },
      +[](void* opaque) { auto& h = host_of(opaque);
        ++h.prepare_output_calls; return h.prepare_output_result; },
      +[](void* opaque) { auto& h = host_of(opaque);
        ++h.selector_calls; return h.selector_result; },
      +[](void* opaque) { ++host_of(opaque).close_ui_calls; return true; }};
  return value;
}

int failures = 0;

void check(bool condition, const char* what) {
  if (condition) return;
  std::fprintf(stderr, "FAIL: %s\n", what);
  ++failures;
}

// A failed FRAME_SETUP has to surface as LifecycleResult::error. Before #725 it
// stayed inside the opaque lifecycle and this came back 0, so the caller
// dispatched RENDER into a frame that was never set up.
void a_refused_setup_reaches_the_caller() {
  Host host;
  host.lifecycle.setup_error = 512;
  auto state = begin_lifecycle(&host, lifecycle_hooks());
  check(state.error == 512, "begin_lifecycle propagates the setup error");
  check(state.lifecycle != nullptr, "the lifecycle is still handed back for teardown");
  check(host.click_calls == 0 && host.interpolate_calls == 0 &&
            host.roundtrip_calls == 0 && host.conditional_ui_calls == 0,
        "no further selector runs after a failed setup");

  // The teardown still has to run, carrying the error, and the lifecycle the
  // early return skipped past still has to be disposed - the real begin hook
  // heap-allocates it and only dispose_lifecycle frees it.
  const int32_t finished = finish_lifecycle(&host, state, lifecycle_hooks(), false);
  check(finished == 512, "finish_lifecycle returns the setup error");
  check(host.end_calls == 1 && host.end_saw_error == 512, "teardown saw the error");
  check(host.lifecycle.disposed, "the lifecycle is disposed after an early return");
  check(state.lifecycle == nullptr, "the disposed lifecycle is not left dangling");
}

// A clean setup must not be disturbed by the new accessor.
void a_clean_setup_still_runs_every_step() {
  Host host;
  auto state = begin_lifecycle(&host, lifecycle_hooks());
  check(state.error == 0, "a clean setup reports no error");
  check(host.click_calls == 1 && host.interpolate_calls == 1 &&
            host.roundtrip_calls == 1 && host.conditional_ui_calls == 1,
        "a clean setup runs all four pre-render steps");
  finish_lifecycle(&host, state, lifecycle_hooks(), false);
  check(host.lifecycle.disposed, "a clean lifecycle is disposed too");
}

// The pre-render hooks are host-side probes, and their -5 is a different path
// from a setup error: setup succeeded, so the accessor must not mask it, and
// the steps after the failing one still have to be skipped.
void a_failing_pre_render_hook_is_still_minus_five() {
  Host host;
  host.click_result = false;
  const auto state = begin_lifecycle(&host, lifecycle_hooks());
  check(state.error == -5, "a failing click hook is -5");
  check(host.click_calls == 1, "the failing hook ran");
  check(host.interpolate_calls == 0 && host.roundtrip_calls == 0 &&
            host.conditional_ui_calls == 0,
        "the steps after the failing hook are skipped");
}

// The accessor is required, not optional: a hook table missing it cannot tell
// a failed setup from a clean one, so it fails closed rather than silently
// reverting to the #725 behavior.
void a_missing_accessor_fails_closed() {
  Host host;
  LifecycleHooks broken = lifecycle_hooks();
  broken.setup_error = nullptr;
  const auto state = begin_lifecycle(&host, broken);
  check(state.error == -5, "a hook table without setup_error is rejected");
  check(host.click_calls == 0, "nothing is dispatched through a rejected hook table");
}

void a_null_lifecycle_still_fails_closed() {
  Host host;
  host.begin_returns_null = true;
  const auto state = begin_lifecycle(&host, lifecycle_hooks());
  check(state.error == -5, "a null lifecycle is -5");
}

// dispatch_render enters the plug-in three times, and only two of them are
// guarded by the incoming error. close_ui runs regardless - issue #735 tracks
// that it is also unattributed - so pin the split rather than assuming it.
void a_nonzero_incoming_error_skips_everything_but_the_ui_close() {
  Host host;
  const int32_t error = dispatch_render(&host, 512, render_hooks());
  check(error == 512, "the incoming error is handed back unchanged");
  check(host.draw_calls == 0, "draw is skipped");
  check(host.prepare_output_calls == 0, "prepare_output is skipped");
  check(host.selector_calls == 0, "the selector is skipped");
  check(host.close_ui_calls == 1, "close_ui still runs (issue #735)");
}

// prepare_output is host-side and runs before the selector, so its refusal has
// to stop the dispatch rather than be handed to the plug-in.
void a_refused_output_stops_before_the_selector() {
  Host host;
  host.prepare_output_result = 4;
  const int32_t error = dispatch_render(&host, 0, render_hooks());
  check(error == 4, "the prepare_output refusal is the frame's error");
  check(host.prepare_output_calls == 1, "prepare_output ran");
  check(host.selector_calls == 0, "the selector did not run");
}

void a_clean_dispatch_reaches_the_selector() {
  Host host;
  const int32_t error = dispatch_render(&host, 0, render_hooks());
  check(error == 0, "a clean dispatch returns 0");
  check(host.draw_calls == 1 && host.prepare_output_calls == 1 &&
            host.selector_calls == 1 && host.close_ui_calls == 1,
        "a clean dispatch runs every step once");
}

// A selector that fails is the one case where the plug-in may have left a UI
// context open, so close_ui has to run and its error must not overwrite the
// selector's.
void a_failing_selector_still_closes_the_ui() {
  Host host;
  host.selector_result = 512;
  const int32_t error = dispatch_render(&host, 0, render_hooks());
  check(error == 512, "the selector's error is the frame's error");
  check(host.selector_calls == 1, "the selector ran");
  check(host.close_ui_calls == 1, "close_ui ran after a failing selector");
}

}  // namespace

int main() {
  a_refused_setup_reaches_the_caller();
  a_clean_setup_still_runs_every_step();
  a_failing_pre_render_hook_is_still_minus_five();
  a_missing_accessor_fails_closed();
  a_null_lifecycle_still_fails_closed();
  a_nonzero_incoming_error_skips_everything_but_the_ui_close();
  a_refused_output_stops_before_the_selector();
  a_clean_dispatch_reaches_the_selector();
  a_failing_selector_still_closes_the_ui();
  if (failures == 0) std::printf("{\"classic_execution_selftest\":\"passed\"}\n");
  return failures == 0 ? 0 : 1;
}
