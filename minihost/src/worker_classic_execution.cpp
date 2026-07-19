#include "worker_classic_execution.hpp"
#include <fstream>
namespace aexcompat::worker_runtime::classic_execution {
LifecycleResult begin_lifecycle(void* host, const LifecycleHooks& h) {
  LifecycleResult result{};
  if (!host || !h.begin || !h.end || !h.dispose_lifecycle) {
    result.error = -5;
    return result;
  }
  result.lifecycle = h.begin(host);
  if (!result.lifecycle) { result.error = -5; return result; }
  auto apply = [&](bool (*operation)(void*)) {
    if (result.error == 0 && operation && !operation(host)) result.error = -5;
  };
  apply(h.click);
  apply(h.interpolate);
  apply(h.roundtrip);
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

int finalize(Context& c, const Hooks& h) {
  std::vector<unsigned char> logical;
  if (!h.copy_packed || !h.hash || !h.copy_packed(c.destination, c.rowbytes, c.width,
      c.height, c.pixel_bytes, logical)) return -3;
  if (c.output_hash) *c.output_hash = h.hash(logical.data(), logical.size());
  if (c.error == 0 && (!h.publish_stage || !h.publish_stage(
      {c.current_time, c.time_scale}, {c.time_step, c.time_scale},
      static_cast<int8_t>(c.quality == 0 ? 0 : 1), c.pixel_format,
      c.width, c.height, logical.data()))) c.error = 4;
  if (c.captured) *c.captured = logical;
  if (h.dump) h.dump(logical.data(), c.width, c.height, c.pixel_bytes);
  if (c.external_output && c.error == 0) {
    std::vector<unsigned char> rgba(static_cast<std::size_t>(c.width) * c.height * c.pixel_bytes);
    for (std::size_t p = 0; p < static_cast<std::size_t>(c.width) * c.height; ++p)
      h.argb_to_rgba(rgba.data() + p * c.pixel_bytes,
                     logical.data() + p * c.pixel_bytes, c.pixel_bytes);
    if (h.checksum) h.checksum(rgba.data(), c.width, c.height, c.pixel_bytes);
    std::ofstream file(*c.external_output, std::ios::binary | std::ios::out);
    if (!file || !file.write(reinterpret_cast<const char*>(rgba.data()), rgba.size())) return -4;
  }
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
