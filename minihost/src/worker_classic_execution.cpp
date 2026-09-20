#include "worker_classic_execution.hpp"
namespace aexcompat::worker_runtime::classic_execution {
LifecycleResult begin_lifecycle(void* host, const LifecycleHooks& h) {
  LifecycleResult result{};
  if (!host || !h.begin || !h.setup_error || !h.end || !h.dispose_lifecycle) {
    result.error = -5;
    return result;
  }
  result.lifecycle = h.begin(host);
  if (!result.lifecycle) { result.error = -5; return result; }
  // SEQUENCE_SETUP or FRAME_SETUP did not succeed. Everything past this point
  // dispatches further selectors into the plug-in, and dispatch_render's
  // short-circuit only holds if that reaches LifecycleResult::error. It did
  // not: `begin` returns the lifecycle opaquely, so the outcome stayed sealed
  // inside it and the caller saw success. RENDER then ran against frame-local
  // state that begin_frame returns before transferring, which is how eight AE
  // 2026 effects took an access violation in RENDER whose FRAME_SETUP had
  // already faulted - and because the SEH telemetry keeps only the last fault,
  // the second one hid the first (issue #725).
  //
  // Stop here rather than at the caller: finish_lifecycle still runs the
  // teardown, and end_frame/end_render already handle a frame that never
  // started.
  result.error = h.setup_error(result.lifecycle);
  if (result.error != 0) return result;
  auto apply = [&](bool (*operation)(void*)) {
    if (result.error == 0 && operation && !operation(host)) result.error = -5;
  };
  apply(h.click);
  apply(h.conditional_ui);
  return result;
}

int32_t finish_lifecycle(void* host, LifecycleResult& state,
                         const LifecycleHooks& h, bool draw) {
  if (!state.lifecycle || !h.end || !h.dispose_lifecycle) return state.error;
  if (draw && state.error == 0 && h.draw && !h.draw(host)) state.error = -5;
  state.error = h.end(host, state.lifecycle, state.error);
  h.dispose_lifecycle(state.lifecycle);
  state.lifecycle = nullptr;
  return state.error;
}

int32_t dispatch_render(void* host, int32_t error, const RenderHooks& h) {
  if (!host || !h.stage_begin || !h.stage_end || !h.draw_enabled ||
      !h.prepare_output || !h.dispatch_selector || !h.ui_context_active ||
      !h.close_ui) return -5;
  if (error == 0 && h.draw && h.draw_enabled(host)) {
    h.stage_begin(host, "classic_ui_draw");
    const bool succeeded = h.draw(host);
    h.stage_end(host, "classic_ui_draw", succeeded ? 0 : -5);
    if (!succeeded) error = -5;
  }
  if (error == 0) error = h.prepare_output(host);
  if (error == 0) error = h.dispatch_selector(host);
  if (h.ui_context_active(host)) {
    h.stage_begin(host, "classic_ui_teardown");
    const bool close_succeeded = h.close_ui(host);
    const int32_t close_error = !close_succeeded && error == 0 ? -5 : 0;
    h.stage_end(host, "classic_ui_teardown", close_error);
    if (close_error != 0) error = close_error;
  }
  return error;
}

int finalize(Context& c, const Hooks& h) {
  std::vector<unsigned char> logical;
  if (!h.copy_packed || !h.hash || !h.copy_packed(c.destination, c.rowbytes, c.width,
      c.height, c.pixel_bytes, logical)) return -3;
  if (c.output_hash) *c.output_hash = h.hash(logical.data(), logical.size());
  // The classic runtime places a non-uniform canary in the active pixels just
  // before RENDER. Exact equality means the plug-in wrote nothing. Comparing
  // that snapshot instead of a fixed fill color keeps legitimate uniform
  // frames (including all 0xCC) distinguishable from untouched storage.
  const bool untouched = c.initial_payload && !logical.empty() &&
      logical == *c.initial_payload;
  if (c.error == 0 && untouched && !c.host_wrote_output) c.error = -6;
  if (c.error == 0 && (!h.publish_stage || !h.publish_stage(
      {c.current_time, c.time_scale}, {c.time_step, c.time_scale},
      static_cast<int8_t>(c.quality == 0 ? 0 : 1), c.pixel_format,
      c.width, c.height, logical.data()))) c.error = 4;
  if (c.captured) *c.captured = logical;
  if (h.dump) h.dump(logical.data(), c.width, c.height, c.pixel_bytes);
  bool padding = true;
  for (int32_t y = 0; padding && y < c.height; ++y)
    for (int32_t x = c.width * c.pixel_bytes; x < c.rowbytes; ++x)
      if (c.destination[y * c.rowbytes + x] != 0xCC) { padding = false; break; }
  if (c.guards_intact) *c.guards_intact = padding && c.sentinels_intact;
  if (h.set_pixel_format) h.set_pixel_format(
      c.pixel_bytes == 16 ? "argb32f" : (c.pixel_bytes == 8 ? "argb16" : "argb8"));
  return c.error;
}
}
