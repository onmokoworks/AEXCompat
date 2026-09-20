#include "worker_smart_finalize.hpp"
#include "render_subsystem.h"
#include "worker_handle_runtime.hpp"
#include "worker_param_checkout_runtime.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_world_registry.hpp"
#include <algorithm>
#include <cstring>

namespace aexcompat::worker_runtime::smart_finalize {
namespace {
template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{}; std::memcpy(&value, bytes.data() + offset, sizeof(value)); return value;
}

void record_output_extent_hint(
    const aexcompat::world_safety::EffectWorldStorage& output_world,
    smart_execution::Result& result) {
  std::memcpy(result.output_extent_hint.data(), output_world.data() + 44,
              sizeof(result.output_extent_hint));
  if (result.output_allocation_failed ||
      (result.empty_result_rect && !result.empty_result_passthrough))
    result.output_extent_hint = {0, 0, 0, 0};
}
}

bool finalize(const Request& r, const Hooks& h, smart_execution::Result& result) {
  if (!r.entry || !r.input || !r.output || !r.parameters || !r.output_world ||
      !r.lifecycle || !r.source || !r.guarded || !r.pre_output || !h.close_ui ||
      !h.end_lifecycle || !h.dump_world || !h.sha256 ||
      !h.ui_active) return false;
  void* pre_render_data = read<void*>(*r.pre_output, 40);
  if (auto cleanup = read<void(__cdecl*)(void*)>(*r.pre_output, 48)) {
    result.pre_cleanup_error = invoke_smart_pre_render_cleanup_seh(cleanup, pre_render_data);
  } else if (pre_render_data && handles::host_handle_is_live(pre_render_data)) {
    handles::dispose_handle(reinterpret_cast<void**>(pre_render_data));
    handles::record_automatic_pre_render_disposal();
  }
  if (h.ui_active() &&
      !h.close_ui(r.entry, *r.input, *r.output, r.parameters->definitions) &&
      result.render_error == 0) result.render_error = -5;
  result.lifecycle_error = h.end_lifecycle(r.entry, *r.input, *r.output,
      r.parameters->params.data(), r.output_world->data(), *r.lifecycle,
      0);
  if (result.render_error == 0) result.render_error = result.lifecycle_error;
  if (result.render_error == 0) result.render_error = result.pre_cleanup_error;
  result.parameter_checkouts_balanced = aexcompat::l2_detail::param_checkouts_balanced();
  auto& state = smart::state();
  state.input_world = nullptr; state.output_world = nullptr;
  state.map_world = nullptr; state.hosted_layers.clear();
  // The empty-layer world is the host's own allocation, so the host returns it.
  // Leaving it live would read as an unbalanced world lifetime, which is the
  // accounting that catches a plug-in leaking one.
  if (state.empty_layer_world_live) {
    world_registry::dispose_world(nullptr, state.empty_layer_world.data());
    state.empty_layer_world_live = false;
  }
  std::vector<unsigned char> logical_input, logical_output;
  if (!render::copy_packed_world(r.source->data(), r.rowbytes, r.width, r.height,
                                  r.pixel_bytes, logical_input) ||
      !render::copy_packed_world(r.destination, result.output_rowbytes,
                                  result.output_width, result.output_height,
                                  r.pixel_bytes, logical_output)) return false;
  result.input_hash = h.sha256(logical_input.data(), logical_input.size());
  result.output_hash = h.sha256(logical_output.data(), logical_output.size());
  // Session frames transfer the packed ARGB output through the shared-memory
  // slot instead of a file; the session loop converts and validates it.
  if (r.session && r.session->captured_argb)
    *r.session->captured_argb = logical_output;
  h.dump_world("smart-output", logical_output.data(), result.output_width,
               result.output_height, r.pixel_bytes);
  const bool untouched = !logical_output.empty() &&
      std::all_of(logical_output.begin(), logical_output.end(),
                  [](unsigned char value) { return value == 0xCC; });
  result.output_untouched = untouched;
  // An 8/16bpc world cannot hold a non-finite value, so the check is on the
  // float32 world; the Premiere GPU-filter route reports the same condition
  // for the float32 frame it narrowed into an 8/16bpc world (issue #1271).
  const bool finite =
      (r.pixel_bytes != 16 || render::finite_float_world(logical_output)) &&
      !result.output_non_finite;
  // A legally empty result promised no pixels; zero output bytes are the
  // correct fulfillment of that contract, not a validation failure.
  result.output_pixels_valid = (result.empty_result_rect &&
                                !result.empty_result_passthrough)
      ? true
      : !logical_output.empty() && !untouched && finite;
  if (result.render_error == 0 && !result.output_pixels_valid) result.render_error = -6;
  // The output world extent_hint is read back from the world the plug-in saw.
  // The empty answer never resized or dispatched the output world; reporting
  // the stale full-frame extent would claim pixels that were never promised.
  // The passthrough did size and fill an output world, so its extent is the
  // one the world carries. A promised-nothing frame and an allocation failure
  // both report empty; the latter must not republish the previous world's
  // stale extent.
  record_output_extent_hint(*r.output_world, result);
  result.guards_intact = r.guarded->sentinels_intact();
  return true;
}
}  // namespace aexcompat::worker_runtime::smart_finalize
