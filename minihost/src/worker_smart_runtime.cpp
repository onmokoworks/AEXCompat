#include "worker_smart_runtime.hpp"

#include <algorithm>
#include <atomic>
#include <cstring>
#include <thread>

namespace aexcompat::worker_runtime::smart {
namespace {

constexpr std::size_t kCheckoutResultBytes = 76;
thread_local State g_default_state;
thread_local State* g_active_state{};

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
  const auto& runtime = state();
  const int32_t par[2] = {runtime.pixel_aspect_numerator,
                          static_cast<int32_t>(runtime.pixel_aspect_denominator)};
  std::memcpy(bytes + 32, par, sizeof(par));
  const int32_t reference_size[2] = {reference_width, reference_height};
  std::memcpy(bytes + 44, reference_size, sizeof(reference_size));
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
  g_default_state.wide_time_checkout_allowed = state_.wide_time_checkout_allowed;
  g_default_state.rejected_temporal_checkouts = state_.rejected_temporal_checkouts;
  state_.clear_transient();
  g_active_state = previous_;
}

int32_t __cdecl pre_checkout_layer(void*, int32_t index, int32_t checkout_id,
                                   const void* request, int32_t what_time,
                                   int32_t time_step, uint32_t time_scale,
                                   void* result) {
  // PF does not provide a host refcon for these callback tables. A callback
  // made on a thread other than the selector thread cannot be bound safely.
  if (!g_active_state || time_step <= 0 || time_scale == 0) return 4;
  auto& runtime = *g_active_state;
  const bool current_time =
      static_cast<int64_t>(what_time) * runtime.current_time_scale ==
      static_cast<int64_t>(runtime.current_time) * time_scale;
  if (!current_time && !runtime.wide_time_checkout_allowed) {
    ++runtime.rejected_temporal_checkouts;
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
  if (request && index == runtime.secondary_layer_slot) {
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
    const int32_t reference_width = runtime.full_resolution_width > 0
        ? runtime.full_resolution_width : runtime.width;
    const int32_t reference_height = runtime.full_resolution_height > 0
        ? runtime.full_resolution_height : runtime.height;
    write_checkout_result(result, runtime.width, runtime.height,
                          reference_width, reference_height);
    return 0;
  }
  if (index == runtime.secondary_layer_slot && runtime.map_world) {
    runtime.secondary_checkout_id = checkout_id;
    write_checkout_result(result, runtime.map_width, runtime.map_height,
                          runtime.map_width, runtime.map_height);
    return 0;
  }
  return 4;
}

int32_t __cdecl checkout_pixels(void*, int32_t checkout_id, void** world) {
  if (!world || !g_active_state) return 4;
  auto& runtime = *g_active_state;
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

int32_t __cdecl checkin_pixels(void*, int32_t) { return g_active_state ? 0 : 4; }

int32_t __cdecl checkout_output(void*, void** world) {
  if (!world || !g_active_state) return 4;
  auto& runtime = *g_active_state;
  if (!runtime.output_world) return 4;
  *world = runtime.output_world;
  return 0;
}

bool concurrency_self_test() {
  std::atomic<int> ready{};
  std::atomic<bool> release{};
  std::array<bool, 2> passed{};
  std::array<std::thread, 2> workers;
  for (int index = 0; index < 2; ++index) {
    workers[index] = std::thread([&, index] {
      Session session;
      auto& runtime = state();
      runtime.width = 320 + index;
      runtime.height = 180 + index;
      runtime.current_time = 10 + index;
      runtime.current_time_scale = 24;
      runtime.pixel_aspect_numerator = 1 + index;
      runtime.pixel_aspect_denominator = 2 + index;
      runtime.output_world = reinterpret_cast<void*>(static_cast<uintptr_t>(index + 1));
      ++ready;
      while (!release.load(std::memory_order_acquire)) std::this_thread::yield();
      std::array<unsigned char, kCheckoutResultBytes> result{};
      void* output{};
      const int32_t checkout_status = pre_checkout_layer(
          nullptr, 0, 0, nullptr, 10 + index, 1, 24, result.data());
      int32_t result_width{}, result_height{}, par_numerator{};
      std::memcpy(&result_width, result.data() + 8, sizeof(result_width));
      std::memcpy(&result_height, result.data() + 12, sizeof(result_height));
      std::memcpy(&par_numerator, result.data() + 32, sizeof(par_numerator));
      passed[index] = checkout_status == 0 && checkout_output(nullptr, &output) == 0 &&
          output == runtime.output_world &&
          result_width == 320 + index && result_height == 180 + index &&
          par_numerator == 1 + index;
    });
  }
  while (ready.load(std::memory_order_acquire) != 2) std::this_thread::yield();
  release.store(true, std::memory_order_release);
  for (auto& worker : workers) worker.join();
  void* cross_thread_output{};
  return passed[0] && passed[1] &&
      checkout_output(nullptr, &cross_thread_output) == 4;
}

}  // namespace aexcompat::worker_runtime::smart
