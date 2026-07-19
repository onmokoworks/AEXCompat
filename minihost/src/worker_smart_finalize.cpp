#include "worker_smart_finalize.hpp"
#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "worker_handle_runtime.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_smart_runtime.hpp"
#include <algorithm>
#include <cstring>
#include <fstream>

namespace aexcompat::worker_runtime::smart_finalize {
namespace {
template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{}; std::memcpy(&value, bytes.data() + offset, sizeof(value)); return value;
}
}
bool finalize(const Request& r, const Hooks& h, smart_execution::Result& result) {
  if (!r.entry || !r.input || !r.output || !r.parameters || !r.output_world ||
      !r.lifecycle || !r.source || !r.guarded || !r.pre_output || !h.close_ui ||
      !h.end_lifecycle || !h.dump_world || !h.record_checksum || !h.sha256 ||
      !h.ui_active) return false;
  void* pre_render_data = read<void*>(*r.pre_output, 40);
  if (auto cleanup = read<void(__cdecl*)(void*)>(*r.pre_output, 48)) {
    invoke_smart_pre_render_cleanup_seh(cleanup, pre_render_data);
  } else if (pre_render_data && handles::host_handle_is_live(pre_render_data)) {
    handles::dispose_handle(reinterpret_cast<void**>(pre_render_data));
    handles::record_automatic_pre_render_disposal();
  }
  if (h.ui_active() &&
      !h.close_ui(r.entry, *r.input, *r.output, r.parameters->definitions) &&
      result.render_error == 0) result.render_error = -5;
  result.render_error = h.end_lifecycle(r.entry, *r.input, *r.output,
      r.parameters->params.data(), r.output_world->data(), *r.lifecycle,
      result.render_error);
  auto& state = smart::state();
  state.input_world = nullptr; state.output_world = nullptr;
  state.map_world = nullptr; state.hosted_layers.clear();
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
  const bool finite = r.pixel_bytes != 16 || render::finite_float_world(logical_output);
  // A legally empty result promised no pixels; zero output bytes are the
  // correct fulfillment of that contract, not a validation failure.
  result.output_pixels_valid = result.empty_result_rect
      ? true
      : !logical_output.empty() && !untouched && finite;
  if (result.render_error == 0 && !result.output_pixels_valid) result.render_error = -6;
  // The output world extent_hint is read back from the world the plug-in saw.
  // The empty answer never resized or dispatched the output world; reporting
  // the stale full-frame extent would claim pixels that were never promised.
  std::memcpy(result.output_extent_hint.data(), r.output_world->data() + 44,
              sizeof(result.output_extent_hint));
  if (result.empty_result_rect) result.output_extent_hint = {0, 0, 0, 0};
  if (r.external_output && result.render_error == 0) {
    std::vector<unsigned char> rgba(static_cast<std::size_t>(result.output_width) *
                                    result.output_height * r.pixel_bytes);
    for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(result.output_width) *
                                      result.output_height; ++pixel)
      render_pixel_transport::argb_to_rgba_native(
          rgba.data() + pixel * r.pixel_bytes,
          logical_output.data() + pixel * r.pixel_bytes, r.pixel_bytes);
    h.record_checksum(rgba.data(), result.output_width, result.output_height,
                      r.pixel_bytes);
    std::ofstream file(*r.external_output, std::ios::binary | std::ios::out);
    if (!file || !file.write(reinterpret_cast<const char*>(rgba.data()), rgba.size()))
      result.render_error = -4;
  }
  result.guards_intact = r.guarded->sentinels_intact();
  return true;
}
}  // namespace aexcompat::worker_runtime::smart_finalize
