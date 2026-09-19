// Behavioral self-test for classic_execution's lifecycle, dispatch, and
// output-finalization contracts.
//
// These are pure functions over a hook table, so the whole contract can be
// exercised with counting fakes and no plug-in, no worker process, and no AEX.
//
// The property under test is which steps reach the plug-in once its own setup
// has been refused (issue #725). dispatch_render enters the plug-in through
// three hooks, and the split is deliberate:
//
//   draw, dispatch_selector  - skipped, both guarded by `error == 0`
//   close_ui                 - NOT skipped while a custom UI context is active,
//                              so a context opened earlier still gets closed.
//                              Its stage reports failure only when that failure
//                              becomes the frame error (issue #735).
//
// prepare_output sits between them and is host-side only.

#include "worker_classic_execution.hpp"

#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

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
  std::vector<std::string> stage_events;

  bool click_result{true};
  bool draw_enabled{true};
  bool draw_result{true};
  bool ui_context_active{true};
  bool close_ui_result{true};
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
      +[](void* opaque, const char* stage) {
        host_of(opaque).stage_events.push_back(std::string(stage) + ":begin"); },
      +[](void* opaque, const char* stage, int32_t error) {
        host_of(opaque).stage_events.push_back(
            std::string(stage) + ":end:" + std::to_string(error)); },
      +[](void* opaque) { return host_of(opaque).draw_enabled; },
      +[](void* opaque) { auto& h = host_of(opaque);
        h.stage_events.push_back("draw:call");
        ++h.draw_calls; return h.draw_result; },
      +[](void* opaque) { auto& h = host_of(opaque);
        ++h.prepare_output_calls; return h.prepare_output_result; },
      +[](void* opaque) { auto& h = host_of(opaque);
        ++h.selector_calls; return h.selector_result; },
      +[](void* opaque) { return host_of(opaque).ui_context_active; },
      +[](void* opaque) { auto& h = host_of(opaque);
        h.stage_events.push_back("close_ui:call");
        ++h.close_ui_calls; return h.close_ui_result; }};
  return value;
}

int failures = 0;

void check(bool condition, const char* what) {
  if (condition) return;
  std::fprintf(stderr, "FAIL: %s\n", what);
  ++failures;
}

struct FinalizeProbe {
  int publish_calls{};
  int dump_calls{};
  std::vector<unsigned char> published;
};

FinalizeProbe* finalize_probe{};

const Hooks& finalize_hooks() {
  static const Hooks value{
      +[](const unsigned char* source, int32_t rowbytes, int32_t width,
          int32_t height, int32_t pixel_bytes,
          std::vector<unsigned char>& output) {
        if (!source || rowbytes < width * pixel_bytes || width <= 0 ||
            height <= 0 || pixel_bytes <= 0) return false;
        output.resize(static_cast<std::size_t>(width) * height * pixel_bytes);
        for (int32_t y = 0; y < height; ++y)
          std::memcpy(output.data() + static_cast<std::size_t>(y) * width * pixel_bytes,
                      source + static_cast<std::size_t>(y) * rowbytes,
                      static_cast<std::size_t>(width) * pixel_bytes);
        return true;
      },
      +[](const unsigned char* data, std::size_t size) {
        return std::string(reinterpret_cast<const char*>(data), size);
      },
      +[](aexcompat::suite_abi::AegpTime, aexcompat::suite_abi::AegpTime,
          int8_t, int32_t,
          int32_t width, int32_t height, const void* pixels) {
        ++finalize_probe->publish_calls;
        const auto* bytes = static_cast<const unsigned char*>(pixels);
        finalize_probe->published.assign(bytes, bytes + width * height * 4);
        return true;
      },
      +[](const void*, int32_t, int32_t, int32_t) {
        ++finalize_probe->dump_calls;
      },
      +[](const char*) {}};
  return value;
}

Context finalize_context(std::vector<unsigned char>& destination,
                         int32_t rowbytes, int32_t width, int32_t height,
                         std::string& hash, bool& guards,
                         std::vector<unsigned char>& captured,
                         const std::vector<unsigned char>& initial_payload,
                         bool host_wrote_output = false) {
  return Context{destination.data(), rowbytes, width, height, 4, 0,
                 0, 1, 1, 0, 0, &hash, &guards, &captured, true,
                 host_wrote_output, &initial_payload};
}

void an_untouched_classic_payload_is_not_a_rendered_frame() {
  constexpr int32_t width = 2;
  constexpr int32_t height = 2;
  constexpr int32_t rowbytes = width * 4 + 3;
  std::vector<unsigned char> destination(rowbytes * height, 0xCC);
  std::vector<unsigned char> initial(width * height * 4);
  for (std::size_t index = 0; index < initial.size(); ++index) {
    initial[index] = static_cast<unsigned char>((index * 131u + 0x5Du) & 0xFFu);
    destination[(index / (width * 4)) * rowbytes + index % (width * 4)] = initial[index];
  }
  std::string hash;
  bool guards = false;
  std::vector<unsigned char> captured;
  FinalizeProbe probe;
  finalize_probe = &probe;
  auto context = finalize_context(destination, rowbytes, width, height,
                                  hash, guards, captured, initial);

  const int32_t error = finalize(context, finalize_hooks());

  check(error == -6, "an unchanged classic canary is rejected as untouched");
  check(probe.publish_calls == 0,
        "an untouched classic payload is not published as a staged world");
  check(captured == initial,
        "the untouched payload remains available for diagnostics");
  check(probe.dump_calls == 1, "the untouched payload can still be dumped");
  check(guards, "untouched pixels do not imply damaged row or allocation guards");
}

void a_written_classic_payload_still_succeeds() {
  constexpr int32_t width = 2;
  constexpr int32_t height = 2;
  constexpr int32_t rowbytes = width * 4 + 3;
  std::vector<unsigned char> destination(rowbytes * height, 0xCC);
  std::vector<unsigned char> initial(width * height * 4);
  for (std::size_t index = 0; index < initial.size(); ++index)
    initial[index] = static_cast<unsigned char>((index * 131u + 0x5Du) & 0xFFu);
  std::string hash;
  bool guards = false;
  std::vector<unsigned char> captured;
  FinalizeProbe probe;
  finalize_probe = &probe;
  auto context = finalize_context(destination, rowbytes, width, height,
                                  hash, guards, captured, initial);

  const int32_t error = finalize(context, finalize_hooks());

  check(error == 0, "a plugin-written all-0xCC classic payload remains valid");
  check(probe.publish_calls == 1,
        "a written classic payload is published exactly once");
  check(probe.published == captured,
        "the staged and captured written payloads match");
  check(guards, "a written payload preserves untouched row and allocation guards");
}

void a_host_copied_payload_equal_to_the_canary_still_succeeds() {
  constexpr int32_t width = 2;
  constexpr int32_t height = 2;
  constexpr int32_t rowbytes = width * 4 + 3;
  // NOP_RENDER asks the host to copy the input. Make that valid input exactly
  // equal to the canary so this succeeds only because the explicit host write
  // is authoritative, not because ordinary byte comparison sees a change.
  std::vector<unsigned char> destination(rowbytes * height, 0xCC);
  std::vector<unsigned char> initial(width * height * 4);
  for (std::size_t index = 0; index < initial.size(); ++index) {
    initial[index] = static_cast<unsigned char>((index * 131u + 0x5Du) & 0xFFu);
    destination[(index / (width * 4)) * rowbytes + index % (width * 4)] = initial[index];
  }
  std::string hash;
  bool guards = false;
  std::vector<unsigned char> captured;
  FinalizeProbe probe;
  finalize_probe = &probe;
  auto context = finalize_context(destination, rowbytes, width, height,
                                  hash, guards, captured, initial, true);

  const int32_t error = finalize(context, finalize_hooks());

  check(error == 0, "a host-copied frame equal to the canary remains valid");
  check(probe.publish_calls == 1,
        "a host-copied canary-equal frame is published exactly once");
  check(probe.published == captured,
        "the staged and captured host-copied payloads match");
  check(guards, "the host-copied payload preserves row and allocation guards");
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

// The pre-render hooks dispatch into the plug-in too, but they report failure
// as a bool, so their -5 is synthesized by begin_lifecycle rather than coming
// from the plug-in. That is a different path from a setup error: setup itself
// succeeded, so the accessor must not mask it, and the steps after the failing
// one still have to be skipped.
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
// guarded by the incoming error. close_ui still runs for an active context, so
// pin the split rather than assuming it.
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
  check(host.stage_events == std::vector<std::string>{
            "classic_ui_draw:begin", "draw:call", "classic_ui_draw:end:0",
            "classic_ui_teardown:begin", "close_ui:call",
            "classic_ui_teardown:end:0"},
        "a clean dispatch brackets both UI steps");
}

void inactive_ui_does_not_emit_stages_or_call_ui_hooks() {
  Host host;
  host.draw_enabled = false;
  host.ui_context_active = false;
  const int32_t error = dispatch_render(&host, 0, render_hooks());
  check(error == 0, "an inactive UI does not change the frame result");
  check(host.draw_calls == 0 && host.close_ui_calls == 0,
        "inactive UI hooks do not run");
  check(host.stage_events.empty(), "inactive UI emits no stage noise");
  check(host.prepare_output_calls == 1 && host.selector_calls == 1,
        "an inactive UI still reaches RENDER");
}

void a_failing_draw_is_attributed_before_the_dispatch_short_circuits() {
  Host host;
  host.draw_result = false;
  const int32_t error = dispatch_render(&host, 0, render_hooks());
  check(error == -5, "a failing draw is the frame error");
  check(host.prepare_output_calls == 0 && host.selector_calls == 0,
        "a failing draw short-circuits output and selector dispatch");
  check(host.close_ui_calls == 1, "close_ui still runs after a failing draw");
  check(host.stage_events == std::vector<std::string>{
            "classic_ui_draw:begin", "draw:call", "classic_ui_draw:end:-5",
            "classic_ui_teardown:begin", "close_ui:call",
            "classic_ui_teardown:end:0"},
        "only the draw stage carries the frame error");
}

// close_ui runs after a failing selector too, and it only becomes the frame's
// error when there is not one already: `if (!close_ui(host) && error == 0)`.
// So a teardown failure is silently dropped whenever the frame already failed -
// which is why #735 cannot just emit a stage marker for it unconditionally.
void a_failing_close_ui_never_overwrites_an_existing_error() {
  Host selector_failed;
  selector_failed.selector_result = 512;
  selector_failed.close_ui_result = false;
  const int32_t kept = dispatch_render(&selector_failed, 0, render_hooks());
  check(kept == 512, "the selector's error survives a failing close_ui");
  check(selector_failed.selector_calls == 1, "the selector ran");
  check(selector_failed.close_ui_calls == 1, "close_ui ran after a failing selector");
  check(selector_failed.stage_events.back() == "classic_ui_teardown:end:0",
        "a suppressed close failure is not reported as a stage failure");

  // With nothing else wrong, the same teardown failure does become the error.
  Host only_close_failed;
  only_close_failed.close_ui_result = false;
  const int32_t surfaced = dispatch_render(&only_close_failed, 0, render_hooks());
  check(surfaced == -5, "a failing close_ui is -5 on an otherwise clean frame");
  check(only_close_failed.stage_events.back() == "classic_ui_teardown:end:-5",
        "a close failure that becomes the frame error is attributed");

  // And on a frame short-circuited before the selector, the drop applies too.
  Host short_circuited;
  short_circuited.close_ui_result = false;
  const int32_t incoming = dispatch_render(&short_circuited, 4, render_hooks());
  check(incoming == 4, "the incoming error survives a failing close_ui");
  check(short_circuited.stage_events == std::vector<std::string>{
            "classic_ui_teardown:begin", "close_ui:call",
            "classic_ui_teardown:end:0"},
        "an incoming error stays the only failure attribution");
}

}  // namespace

int main() {
  an_untouched_classic_payload_is_not_a_rendered_frame();
  a_written_classic_payload_still_succeeds();
  a_host_copied_payload_equal_to_the_canary_still_succeeds();
  a_refused_setup_reaches_the_caller();
  a_clean_setup_still_runs_every_step();
  a_failing_pre_render_hook_is_still_minus_five();
  a_missing_accessor_fails_closed();
  a_null_lifecycle_still_fails_closed();
  a_nonzero_incoming_error_skips_everything_but_the_ui_close();
  a_refused_output_stops_before_the_selector();
  a_clean_dispatch_reaches_the_selector();
  inactive_ui_does_not_emit_stages_or_call_ui_hooks();
  a_failing_draw_is_attributed_before_the_dispatch_short_circuits();
  a_failing_close_ui_never_overwrites_an_existing_error();
  if (failures == 0) std::printf("{\"classic_execution_selftest\":\"passed\"}\n");
  return failures == 0 ? 0 : 1;
}
