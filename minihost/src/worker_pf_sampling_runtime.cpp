#include "worker_pf_sampling_runtime.hpp"
#include "worker_pf_suites_internal.hpp"
#include "worker_suite_registry.hpp"
#include <windows.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <mutex>
#include <thread>
#include <unordered_map>

namespace {
constexpr int32_t kPfBadCallbackParam = 4;
PfSamplingHostHooks g_hooks{};
bool resolve_world(void* world, int32_t pixel_bytes, unsigned char*& pixels,
                   int32_t& rowbytes, int32_t& width, int32_t& height) {
  return g_hooks.resolve_world &&
      g_hooks.resolve_world(world, pixel_bytes, pixels, rowbytes, width, height);
}
int32_t acquire_host_suite(const char* name, int32_t version, const void** suite) {
  if (!suite) return kPfBadCallbackParam;
  *suite = nullptr;
  return g_hooks.acquire_suite ? g_hooks.acquire_suite(name, version, suite)
                               : kPfBadCallbackParam;
}
int32_t release_host_suite(const char* name, int32_t version) {
  return g_hooks.release_suite ? g_hooks.release_suite(name, version)
                               : kPfBadCallbackParam;
}
}

std::array<void*, 3> g_sampling8_suite1{
    reinterpret_cast<void*>(&nearest_sample8),
    reinterpret_cast<void*>(&subpixel_sample8),
    reinterpret_cast<void*>(&area_sample8)};
std::array<void*, 3> g_sampling16_suite1{
    reinterpret_cast<void*>(&nearest_sample16),
    reinterpret_cast<void*>(&subpixel_sample16),
    reinterpret_cast<void*>(&area_sample16)};
std::array<void*, 3> g_sampling_float_suite1{
    reinterpret_cast<void*>(&nearest_sample_float),
    reinterpret_cast<void*>(&subpixel_sample_float),
    reinterpret_cast<void*>(&area_sample_float)};
PfBatchSamplingSuite1 g_batch_sampling_suite1{
    &begin_sampling8, &end_sampling8, &unsupported_batch_sample_func,
    &unsupported_batch_sample_func16};
static_assert(sizeof(PfBatchSamplingSuite1) == 4 * sizeof(void*));
static_assert(offsetof(PfBatchSamplingSuite1, get_batch_func) == 2 * sizeof(void*));

void configure_pf_sampling_runtime(const PfSamplingHostHooks& hooks) noexcept {
  g_hooks = hooks;
}

// `effect_ref` is accepted and ignored. It used to be rejected when null, which
// cost AE's own Displacement every frame: its SMART_RENDER pixel function calls
// PF_SUBPIXEL_SAMPLE with a null ref, took the 4 back, and returned it as its
// own PF_Err_OUT_OF_MEMORY (issue #777). The host's `in_data->effect_ref` is
// populated - the plug-in simply does not pass it, and AE samples anyway.
//
// Nothing here needs it: the world being sampled comes from the sampling
// parameter block, and `resolve_world` below still refuses a world this worker
// does not own. Rejecting on the unused argument protected nothing and turned a
// valid sample into an allocation failure.
int32_t subpixel_sample_typed(int32_t pixel_bytes, void* effect_ref, int32_t fixed_x,
                              int32_t fixed_y, const void* sampling_params,
                              void* destination_pixel) {
  (void)effect_ref;
  if (!sampling_params || !destination_pixel) return 4;
  void* source_world{};
  std::memcpy(&source_world, static_cast<const std::byte*>(sampling_params) + 16,
              sizeof(source_world));
  unsigned char* source{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(source_world, pixel_bytes, source, rowbytes, width, height)) return 4;
  const double x = fixed_x / 65536.0;
  const double y = fixed_y / 65536.0;
  const int32_t x0 = static_cast<int32_t>(std::floor(x));
  const int32_t y0 = static_cast<int32_t>(std::floor(y));
  const double fraction_x = x - x0;
  const double fraction_y = y - y0;
  const auto weight = [&](int dx, int dy) {
    return (dx ? fraction_x : 1.0 - fraction_x) *
        (dy ? fraction_y : 1.0 - fraction_y);
  };
  for (int channel = 0; channel < 4; ++channel) {
    double value = 0.0;
    for (int dy = 0; dy < 2; ++dy) for (int dx = 0; dx < 2; ++dx) {
      const int32_t sample_x = x0 + dx, sample_y = y0 + dy;
      if (sample_x < 0 || sample_x >= width || sample_y < 0 || sample_y >= height) continue;
      const auto* pixel = source + static_cast<std::size_t>(sample_y) * rowbytes +
          static_cast<std::size_t>(sample_x) * pixel_bytes;
      const double sample = pixel_bytes == 4 ? pixel[channel] :
          (pixel_bytes == 8 ? reinterpret_cast<const uint16_t*>(pixel)[channel] :
                              reinterpret_cast<const float*>(pixel)[channel]);
      value += sample * weight(dx, dy);
    }
    if (pixel_bytes == 4)
      static_cast<uint8_t*>(destination_pixel)[channel] =
          static_cast<uint8_t>(std::clamp(std::lround(value), 0l, 255l));
    else if (pixel_bytes == 8)
      static_cast<uint16_t*>(destination_pixel)[channel] =
          static_cast<uint16_t>(std::clamp(std::lround(value), 0l, 32768l));
    else
      static_cast<float*>(destination_pixel)[channel] = static_cast<float>(value);
  }
  return 0;
}

// `effect_ref` accepted and ignored, for the same reason as above (issue #777).
int32_t nearest_sample_typed(int32_t pixel_bytes, void* effect_ref, int32_t fixed_x,
                             int32_t fixed_y, const void* sampling_params,
                             void* destination_pixel) {
  (void)effect_ref;
  if (!sampling_params || !destination_pixel) return 4;
  void* source_world{};
  std::memcpy(&source_world, static_cast<const std::byte*>(sampling_params) + 16,
              sizeof(source_world));
  unsigned char* source{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(source_world, pixel_bytes, source, rowbytes, width, height)) return 4;
  const int32_t x = static_cast<int32_t>(std::floor(fixed_x / 65536.0 + 0.5));
  const int32_t y = static_cast<int32_t>(std::floor(fixed_y / 65536.0 + 0.5));
  if (x < 0 || x >= width || y < 0 || y >= height) {
    std::memset(destination_pixel, 0, pixel_bytes);
    return 0;
  }
  std::memcpy(destination_pixel,
              source + static_cast<std::size_t>(y) * rowbytes +
                  static_cast<std::size_t>(x) * pixel_bytes,
              pixel_bytes);
  return 0;
}

int32_t __cdecl nearest_sample8(void* effect_ref, int32_t x, int32_t y,
                                const void* params, void* pixel) {
  return nearest_sample_typed(4, effect_ref, x, y, params, pixel);
}
int32_t __cdecl nearest_sample16(void* effect_ref, int32_t x, int32_t y,
                                 const void* params, void* pixel) {
  return nearest_sample_typed(8, effect_ref, x, y, params, pixel);
}
int32_t __cdecl nearest_sample_float(void* effect_ref, int32_t x, int32_t y,
                                     const void* params, void* pixel) {
  return nearest_sample_typed(16, effect_ref, x, y, params, pixel);
}

// `effect_ref` accepted and ignored, for the same reason as the other two
// samplers (issue #777). PF_SUBPIXEL_SAMPLE and PF_AREA_SAMPLE take the same
// argument on the same terms, so they answer a null ref the same way.
int32_t area_sample_typed(int32_t pixel_bytes, void* effect_ref, int32_t fixed_x,
                          int32_t fixed_y, const void* sampling_params,
                          void* destination_pixel) {
  (void)effect_ref;
  if (!sampling_params || !destination_pixel) return 4;
  int32_t fixed_radius_x{}, fixed_radius_y{}, fixed_area{};
  uint32_t edge_behavior{};
  void* source_world{};
  const auto* params = static_cast<const std::byte*>(sampling_params);
  std::memcpy(&fixed_radius_x, params, sizeof(fixed_radius_x));
  std::memcpy(&fixed_radius_y, params + 4, sizeof(fixed_radius_y));
  std::memcpy(&fixed_area, params + 8, sizeof(fixed_area));
  std::memcpy(&source_world, params + 16, sizeof(source_world));
  std::memcpy(&edge_behavior, params + 24, sizeof(edge_behavior));
  const double radius_x = fixed_radius_x / 65536.0;
  const double radius_y = fixed_radius_y / 65536.0;
  if (fixed_area <= 0 || edge_behavior != 0 || radius_x <= 0.0 || radius_y <= 0.0 ||
      radius_x >= 128.0 || radius_y >= 128.0) return 4;
  unsigned char* source{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(source_world, pixel_bytes, source, rowbytes, width, height)) return 4;
  const double center_x = fixed_x / 65536.0;
  const double center_y = fixed_y / 65536.0;
  const double left = center_x - radius_x, right = center_x + radius_x;
  const double top = center_y - radius_y, bottom = center_y + radius_y;
  const double footprint = (right - left) * (bottom - top);
  double weighted_alpha = 0.0;
  std::array<double, 3> weighted_color{};
  const double maximum = pixel_bytes == 4 ? 255.0 : (pixel_bytes == 8 ? 32768.0 : 1.0);
  const int32_t first_x = std::max(0, static_cast<int32_t>(std::floor(left - 0.5)));
  const int32_t last_x = std::min(width - 1, static_cast<int32_t>(std::ceil(right + 0.5)));
  const int32_t first_y = std::max(0, static_cast<int32_t>(std::floor(top - 0.5)));
  const int32_t last_y = std::min(height - 1, static_cast<int32_t>(std::ceil(bottom + 0.5)));
  for (int32_t y = first_y; y <= last_y; ++y) {
    const double overlap_y = std::max(0.0, std::min(bottom, y + 0.5) - std::max(top, y - 0.5));
    for (int32_t x = first_x; x <= last_x; ++x) {
      const double overlap_x = std::max(0.0, std::min(right, x + 0.5) - std::max(left, x - 0.5));
      const double weight = overlap_x * overlap_y;
      if (weight == 0.0) continue;
      const auto* pixel = source + static_cast<std::size_t>(y) * rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      const auto read_channel = [&](int channel) {
        return pixel_bytes == 4 ? static_cast<double>(pixel[channel]) :
            (pixel_bytes == 8 ? static_cast<double>(reinterpret_cast<const uint16_t*>(pixel)[channel]) :
                                static_cast<double>(reinterpret_cast<const float*>(pixel)[channel]));
      };
      const double alpha = read_channel(0) / maximum;
      weighted_alpha += weight * alpha;
      for (int channel = 0; channel < 3; ++channel)
        weighted_color[channel] += weight * alpha * read_channel(channel + 1);
    }
  }
  std::array<double, 4> result{};
  result[0] = weighted_alpha / footprint * maximum;
  for (int channel = 0; channel < 3; ++channel)
    result[channel + 1] = weighted_alpha > 0.0 ? weighted_color[channel] / weighted_alpha : 0.0;
  for (int channel = 0; channel < 4; ++channel) {
    if (pixel_bytes == 4)
      static_cast<uint8_t*>(destination_pixel)[channel] = static_cast<uint8_t>(
          std::clamp(std::lround(result[channel]), 0l, 255l));
    else if (pixel_bytes == 8)
      static_cast<uint16_t*>(destination_pixel)[channel] = static_cast<uint16_t>(
          std::clamp(std::lround(result[channel]), 0l, 32768l));
    else
      static_cast<float*>(destination_pixel)[channel] = static_cast<float>(result[channel]);
  }
  return 0;
}

int32_t __cdecl area_sample8(void* effect_ref, int32_t x, int32_t y,
                             const void* params, void* pixel) {
  return area_sample_typed(4, effect_ref, x, y, params, pixel);
}
int32_t __cdecl area_sample16(void* effect_ref, int32_t x, int32_t y,
                              const void* params, void* pixel) {
  return area_sample_typed(8, effect_ref, x, y, params, pixel);
}
int32_t __cdecl area_sample_float(void* effect_ref, int32_t x, int32_t y,
                                   const void* params, void* pixel) {
  return area_sample_typed(16, effect_ref, x, y, params, pixel);
}

struct LegacySamplingSession {
  int32_t quality{};
  uint32_t mode_flags{};
  void* source_world{};
  DWORD thread_id{};
};
std::mutex g_legacy_sampling_mutex;
std::unordered_map<void*, LegacySamplingSession> g_legacy_sampling_sessions;

int32_t __cdecl begin_sampling8(void* effect_ref, int32_t quality, uint32_t mode_flags,
                                void* sampling_params) {
  if (effect_ref != g_hooks.effect_ref || !sampling_params || (quality != 0 && quality != 1))
    return kPfBadCallbackParam;
  void* source_world{};
  std::memcpy(&source_world, static_cast<std::byte*>(sampling_params) + 16,
              sizeof(source_world));
  unsigned char* source{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(source_world, 4, source, rowbytes, width, height))
    return kPfBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_legacy_sampling_mutex);
  const auto [_, inserted] = g_legacy_sampling_sessions.emplace(
      sampling_params, LegacySamplingSession{quality, mode_flags, source_world,
                                             GetCurrentThreadId()});
  return inserted ? 0 : kPfBadCallbackParam;
}

int32_t __cdecl end_sampling8(void* effect_ref, int32_t quality, uint32_t mode_flags,
                              void* sampling_params) {
  if (effect_ref != g_hooks.effect_ref || !sampling_params) return kPfBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_legacy_sampling_mutex);
  const auto found = g_legacy_sampling_sessions.find(sampling_params);
  if (found == g_legacy_sampling_sessions.end() || found->second.quality != quality ||
      found->second.mode_flags != mode_flags ||
      found->second.thread_id != GetCurrentThreadId()) return kPfBadCallbackParam;
  g_legacy_sampling_sessions.erase(found);
  return 0;
}

int32_t unsupported_batch_sample_func_for_slot(
    uint32_t slot, void* effect_ref, int32_t quality, uint32_t mode_flags,
    const void* sampling_params, void** batch) {
  if (!batch) return kPfBadCallbackParam;
  *batch = nullptr;
  if (effect_ref != g_hooks.effect_ref || !sampling_params || (quality != 0 && quality != 1))
    return kPfBadCallbackParam;
  (void)mode_flags;
  return aexcompat::worker_runtime::record_unsupported_suite_call(
      aexcompat::worker_runtime::UnsupportedSuiteId::pf_batch_sampling_1,
      slot);
}

int32_t __cdecl unsupported_batch_sample_func(void* effect_ref, int32_t quality,
                                               uint32_t mode_flags,
                                               const void* sampling_params,
                                               void** batch) {
  return unsupported_batch_sample_func_for_slot(
      2, effect_ref, quality, mode_flags, sampling_params, batch);
}

int32_t __cdecl unsupported_batch_sample_func16(void* effect_ref, int32_t quality,
                                                 uint32_t mode_flags,
                                                 const void* sampling_params,
                                                 void** batch) {
  return unsupported_batch_sample_func_for_slot(
      3, effect_ref, quality, mode_flags, sampling_params, batch);
}

bool verify_pf_batch_sampling_suite() {
  const void* acquired{};
  if (acquire_host_suite("PF Batch Sampling Suite", 1, &acquired) != 0 ||
      acquired != g_hooks.batch_sampling_suite)
    return false;

  alignas(void*) std::array<std::byte, 64> world{};
  alignas(void*) std::array<std::byte, 64> params{};
  std::array<std::byte, 16> pixels{};
  void* pixel_data = pixels.data();
  int32_t rowbytes = 8, width = 2, height = 2;
  std::memcpy(world.data() + 24, &pixel_data, sizeof(pixel_data));
  std::memcpy(world.data() + 32, &rowbytes, sizeof(rowbytes));
  std::memcpy(world.data() + 36, &width, sizeof(width));
  std::memcpy(world.data() + 40, &height, sizeof(height));
  void* source_world = world.data();
  std::memcpy(params.data() + 16, &source_world, sizeof(source_world));

  auto& suite = g_batch_sampling_suite1;
  bool passed = suite.begin_sampling(g_hooks.effect_ref, 1, 0x12, params.data()) == 0 &&
      suite.begin_sampling(g_hooks.effect_ref, 1, 0x12, params.data()) == kPfBadCallbackParam &&
      suite.end_sampling(g_hooks.effect_ref, 0, 0x12, params.data()) == kPfBadCallbackParam &&
      suite.end_sampling(g_hooks.effect_ref, 1, 0x13, params.data()) == kPfBadCallbackParam;

  int32_t cross_thread_result{};
  std::thread foreign_thread([&] {
    cross_thread_result = suite.end_sampling(g_hooks.effect_ref, 1, 0x12, params.data());
  });
  foreign_thread.join();
  passed = passed && cross_thread_result == kPfBadCallbackParam &&
      suite.end_sampling(g_hooks.effect_ref, 1, 0x12, params.data()) == 0 &&
      suite.end_sampling(g_hooks.effect_ref, 1, 0x12, params.data()) == kPfBadCallbackParam &&
      suite.begin_sampling(nullptr, 1, 0, params.data()) == kPfBadCallbackParam &&
      suite.begin_sampling(g_hooks.effect_ref, 2, 0, params.data()) == kPfBadCallbackParam &&
      suite.begin_sampling(g_hooks.effect_ref, 1, 0, nullptr) == kPfBadCallbackParam;

  void* batch = reinterpret_cast<void*>(0x1234);
  passed = passed && suite.get_batch_func(g_hooks.effect_ref, 1, 0, params.data(), &batch) == 4 &&
      batch == nullptr;
  batch = reinterpret_cast<void*>(0x5678);
  passed = passed && suite.get_batch_func16(g_hooks.effect_ref, 0, 0, params.data(), &batch) == 4 &&
      batch == nullptr;
  batch = reinterpret_cast<void*>(0x9abc);
  passed = passed &&
      suite.get_batch_func(nullptr, 1, 0, params.data(), &batch) == kPfBadCallbackParam &&
      batch == nullptr &&
      suite.get_batch_func(g_hooks.effect_ref, 1, 0, params.data(), nullptr) == kPfBadCallbackParam;

  passed = release_host_suite("PF Batch Sampling Suite", 1) == 0 && passed;
  std::lock_guard<std::mutex> lock(g_legacy_sampling_mutex);
  return passed && g_legacy_sampling_sessions.empty();
}

int32_t __cdecl subpixel_sample8(void* effect_ref, int32_t x, int32_t y,
                                 const void* params, void* pixel) {
  return subpixel_sample_typed(4, effect_ref, x, y, params, pixel);
}
int32_t __cdecl subpixel_sample16(void* effect_ref, int32_t x, int32_t y,
                                  const void* params, void* pixel) {
  return subpixel_sample_typed(8, effect_ref, x, y, params, pixel);
}
int32_t __cdecl subpixel_sample_float(void* effect_ref, int32_t x, int32_t y,
                                      const void* params, void* pixel) {
  return subpixel_sample_typed(16, effect_ref, x, y, params, pixel);
}

