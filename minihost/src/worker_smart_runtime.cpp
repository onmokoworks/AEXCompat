#include "worker_smart_runtime.hpp"

#include <algorithm>
#include <cstring>

namespace aexcompat::worker_runtime::smart {
namespace {

constexpr std::size_t kCheckoutResultBytes = 76;
thread_local State g_default_state;
thread_local State* g_active_state{};
HostHooks g_hooks{};
bool g_hooks_configured{};

bool same_rational_time(int32_t left, uint32_t left_scale,
                        int32_t right, uint32_t right_scale) {
  return static_cast<int64_t>(left) * right_scale ==
      static_cast<int64_t>(right) * left_scale;
}

void write_rect(void* destination, int32_t width, int32_t height) {
  const int32_t values[4] = {0, 0, width, height};
  std::memcpy(destination, values, sizeof(values));
}

void write_checkout_result(void* destination, int32_t width, int32_t height,
                           int32_t reference_width,
                           int32_t reference_height) {
  auto* bytes = static_cast<unsigned char*>(destination);
  std::memset(bytes, 0, kCheckoutResultBytes);
  write_rect(bytes, width, height);
  write_rect(bytes + 16, width, height);
  const int32_t par[2] = {*g_hooks.pixel_aspect_numerator,
                          static_cast<int32_t>(*g_hooks.pixel_aspect_denominator)};
  std::memcpy(bytes + 32, par, sizeof(par));
  const int32_t reference_size[2] = {reference_width, reference_height};
  std::memcpy(bytes + 44, reference_size, sizeof(reference_size));
}

bool hooks_valid(const HostHooks& hooks) {
  return hooks.wide_time_checkout_allowed && hooks.checkout_current_time &&
      hooks.checkout_current_time_scale && hooks.rejected_temporal_checkouts &&
      hooks.secondary_layer_slot && hooks.full_resolution_width &&
      hooks.full_resolution_height && hooks.pixel_aspect_numerator &&
      hooks.pixel_aspect_denominator;
}

}  // namespace

void State::clear_transient() {
  input_world = nullptr;
  output_world = nullptr;
  map_world = nullptr;
  hosted_layers.clear();
  map_width = 0;
  map_height = 0;
  secondary_checkout_id = -1;
  checkout_time = 0;
  checkout_time_step = 0;
  checkout_time_scale = 0;
  input_checkout_request.fill(-1);
  map_checkout_request.fill(-1);
}

bool configure_host_hooks(const HostHooks& hooks) {
  if (!hooks_valid(hooks)) return false;
  g_hooks = hooks;
  g_hooks_configured = true;
  return true;
}

State& state() { return g_active_state ? *g_active_state : g_default_state; }
int32_t __cdecl width() { return state().width; }
int32_t __cdecl height() { return state().height; }

Session::Session() : previous_(g_active_state) { g_active_state = &state_; }

Session::~Session() {
  g_default_state.width = state_.width;
  g_default_state.height = state_.height;
  g_default_state.rowbytes = state_.rowbytes;
  g_default_state.pixel_format = state_.pixel_format;
  g_default_state.checkout_time = state_.checkout_time;
  g_default_state.checkout_time_step = state_.checkout_time_step;
  g_default_state.checkout_time_scale = state_.checkout_time_scale;
  g_default_state.input_checkout_request = state_.input_checkout_request;
  g_default_state.map_checkout_request = state_.map_checkout_request;
  state_.clear_transient();
  g_active_state = previous_;
}

int32_t __cdecl pre_checkout_layer(void*, int32_t index, int32_t checkout_id,
                                   const void* request, int32_t what_time,
                                   int32_t time_step, uint32_t time_scale,
                                   void* result) {
  if (!g_hooks_configured || time_step <= 0 || time_scale == 0) return 4;
  auto& runtime = state();
  const bool current_time =
      static_cast<int64_t>(what_time) * *g_hooks.checkout_current_time_scale ==
      static_cast<int64_t>(*g_hooks.checkout_current_time) * time_scale;
  if (!current_time && !*g_hooks.wide_time_checkout_allowed) {
    ++*g_hooks.rejected_temporal_checkouts;
    return 4;
  }
  auto hosted = std::find_if(runtime.hosted_layers.begin(),
      runtime.hosted_layers.end(), [index, what_time, time_scale](const auto& layer) {
        return layer.slot == index && layer.timed &&
            same_rational_time(layer.time, layer.time_scale, what_time, time_scale);
      });
  if (hosted == runtime.hosted_layers.end())
    hosted = std::find_if(runtime.hosted_layers.begin(), runtime.hosted_layers.end(),
        [index](const auto& layer) { return layer.slot == index && !layer.timed; });
  const bool timed_slot = std::any_of(runtime.hosted_layers.begin(),
      runtime.hosted_layers.end(),
      [index](const auto& layer) { return layer.slot == index && layer.timed; });
  if (hosted != runtime.hosted_layers.end()) {
    if (request)
      std::memcpy(runtime.map_checkout_request.data(), request,
                  sizeof(runtime.map_checkout_request));
    hosted->checkout_id = checkout_id;
    if (!result) return 4;
    write_checkout_result(result, hosted->width, hosted->height,
                          hosted->width, hosted->height);
    return 0;
  }
  if (timed_slot) return 4;
  if (request && index == 0 && checkout_id == 0)
    std::memcpy(runtime.input_checkout_request.data(), request,
                sizeof(runtime.input_checkout_request));
  if (request && index == *g_hooks.secondary_layer_slot) {
    std::memcpy(runtime.map_checkout_request.data(), request,
                sizeof(runtime.map_checkout_request));
    runtime.secondary_checkout_id = checkout_id;
  }
  if (index == 0 && checkout_id == 0) {
    runtime.checkout_time = what_time;
    runtime.checkout_time_step = time_step;
    runtime.checkout_time_scale = time_scale;
  }
  if (!result) return 4;
  if (index == 0 && checkout_id == 0) {
    const int32_t reference_width = *g_hooks.full_resolution_width > 0
        ? *g_hooks.full_resolution_width : runtime.width;
    const int32_t reference_height = *g_hooks.full_resolution_height > 0
        ? *g_hooks.full_resolution_height : runtime.height;
    write_checkout_result(result, runtime.width, runtime.height,
                          reference_width, reference_height);
    return 0;
  }
  if (index == *g_hooks.secondary_layer_slot && runtime.map_world) {
    runtime.secondary_checkout_id = checkout_id;
    write_checkout_result(result, runtime.map_width, runtime.map_height,
                          runtime.map_width, runtime.map_height);
    return 0;
  }
  return 4;
}

int32_t __cdecl checkout_pixels(void*, int32_t checkout_id, void** world) {
  if (!world) return 4;
  auto& runtime = state();
  const auto hosted = std::find_if(runtime.hosted_layers.begin(),
      runtime.hosted_layers.end(), [checkout_id](const auto& layer) {
        return layer.checkout_id == checkout_id;
      });
  if (hosted != runtime.hosted_layers.end() && hosted->world) {
    *world = hosted->world;
    return 0;
  }
  if (checkout_id == 0 && runtime.input_world) *world = runtime.input_world;
  else if (checkout_id == runtime.secondary_checkout_id && runtime.map_world)
    *world = runtime.map_world;
  else return 4;
  return 0;
}

int32_t __cdecl checkin_pixels(void*, int32_t) { return 0; }

int32_t __cdecl checkout_output(void*, void** world) {
  auto& runtime = state();
  if (!world || !runtime.output_world) return 4;
  *world = runtime.output_world;
  return 0;
}

}  // namespace aexcompat::worker_runtime::smart
