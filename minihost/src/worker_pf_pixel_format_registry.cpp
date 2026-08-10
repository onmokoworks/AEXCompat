#include "worker_pf_pixel_format_registry.hpp"

#include "worker_mask_runtime_internal.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>
#include <limits>
#include <mutex>

namespace aexcompat::l2_detail {

using world_registry::kPixelFormatArgb32;
using world_registry::kPixelFormatArgb64;
using world_registry::kPixelFormatArgb128;

extern OpaqueHostObject g_effect;

namespace {
std::mutex g_pixel_format_mutex;
constexpr std::size_t kMaxSupportedPixelFormats = 256;
constexpr int32_t make_public_fourcc(char a, char b, char c, char d) {
  return static_cast<int32_t>(static_cast<uint8_t>(a)) |
      (static_cast<int32_t>(static_cast<uint8_t>(b)) << 8) |
      (static_cast<int32_t>(static_cast<uint8_t>(c)) << 16) |
      (static_cast<int32_t>(static_cast<uint8_t>(d)) << 24);
}
constexpr int32_t kPublicPixelFormatArgb32 =
    make_public_fourcc('a', 'r', 'g', 'b');
constexpr int32_t kPublicPixelFormatArgb64 =
    make_public_fourcc('A', 'r', 'g', 'b');
constexpr int32_t kPublicPixelFormatArgb128 =
    make_public_fourcc('A', 'R', 'g', 'b');

int32_t public_to_internal_pixel_format(int32_t pixel_format) {
  if (pixel_format == kPublicPixelFormatArgb32) return kPixelFormatArgb32;
  if (pixel_format == kPublicPixelFormatArgb64) return kPixelFormatArgb64;
  if (pixel_format == kPublicPixelFormatArgb128) return kPixelFormatArgb128;
  return 0;
}

int32_t internal_to_public_pixel_format(int32_t pixel_format) {
  if (pixel_format == kPixelFormatArgb32) return kPublicPixelFormatArgb32;
  if (pixel_format == kPixelFormatArgb64) return kPublicPixelFormatArgb64;
  if (pixel_format == kPixelFormatArgb128) return kPublicPixelFormatArgb128;
  return 0;
}
}

std::vector<int32_t> g_supported_pixel_formats;
uint32_t g_pixel_format_add_calls{};
uint32_t g_pixel_format_clear_calls{};
std::atomic_uint32_t g_invalid_pixel_format_operations{};
std::atomic_bool g_global_setup_active{false};

bool supported_cpu_pixel_format(int32_t pixel_format) {
  return public_to_internal_pixel_format(pixel_format) != 0;
}

int32_t __cdecl add_supported_pixel_format(void*, int32_t pixel_format) {
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  // This callback records the plug-in's advertised PrPixelFormat values; it
  // does not select a world layout. Keep allocation/conversion restricted by
  // supported_cpu_pixel_format(), while accepting bounded nonzero public enum
  // values here so the host can fall back to AE's default ARGB8 format.
  if (!g_global_setup_active || pixel_format == 0) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  ++g_pixel_format_add_calls;
  const auto found = std::find(g_supported_pixel_formats.begin(),
                               g_supported_pixel_formats.end(), pixel_format);
  if (found == g_supported_pixel_formats.end()) {
    if (g_supported_pixel_formats.size() >= kMaxSupportedPixelFormats) {
      ++g_invalid_pixel_format_operations;
      return 4;
    }
    g_supported_pixel_formats.push_back(pixel_format);
  }
  return 0;
}

int32_t __cdecl clear_supported_pixel_formats(void*) {
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  if (!g_global_setup_active) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  g_supported_pixel_formats.clear();
  ++g_pixel_format_clear_calls;
  return 0;
}

int32_t __cdecl new_world_of_pixel_format(void* effect_ref, uint32_t width,
                                           uint32_t height, int32_t flags,
                                           int32_t pixel_format, void* world) {
  if (width > static_cast<uint32_t>((std::numeric_limits<int32_t>::max)()) ||
      height > static_cast<uint32_t>((std::numeric_limits<int32_t>::max)()) ||
      (flags & ~3) != 0) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  const int32_t internal_format = public_to_internal_pixel_format(pixel_format);
  if (!internal_format) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  return world_registry::new_world(
      effect_ref, static_cast<int32_t>(width), static_cast<int32_t>(height),
      flags & 1, internal_format, world);
}

int32_t __cdecl dispose_pixel_format_world(void* effect_ref, void* world) {
  return world_registry::dispose_world(effect_ref, world);
}

int32_t __cdecl get_pixel_format(const void* world, int32_t* pixel_format) {
  int32_t internal_format{};
  const int32_t result =
      world_registry::get_pixel_format(world, &internal_format);
  if (result != 0 || !pixel_format) return result != 0 ? result : 4;
  const int32_t public_format =
      internal_to_public_pixel_format(internal_format);
  if (!public_format) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  *pixel_format = public_format;
  return 0;
}

namespace {

template <typename Channel>
void write_argb(void* pixel, Channel alpha, Channel red, Channel green,
                Channel blue) {
  const std::array<Channel, 4> value{alpha, red, green, blue};
  std::memcpy(pixel, value.data(), sizeof(value));
}

float bounded_channel(float value) {
  return std::clamp(value, 0.0f, 1.0f);
}

}  // namespace

int32_t __cdecl convert_color_to_pixel_formatted_data(
    int32_t pixel_format, float alpha, float red, float green, float blue,
    void* pixel) {
  if (!pixel || !std::isfinite(alpha) || !std::isfinite(red) ||
      !std::isfinite(green) || !std::isfinite(blue)) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  alpha = bounded_channel(alpha);
  red = bounded_channel(red);
  green = bounded_channel(green);
  blue = bounded_channel(blue);
  const int32_t internal_format = public_to_internal_pixel_format(pixel_format);
  if (internal_format == kPixelFormatArgb32) {
    constexpr float kMaximum = 255.0f;
    write_argb<uint8_t>(pixel, static_cast<uint8_t>(std::lround(alpha * kMaximum)),
                        static_cast<uint8_t>(std::lround(red * kMaximum)),
                        static_cast<uint8_t>(std::lround(green * kMaximum)),
                        static_cast<uint8_t>(std::lround(blue * kMaximum)));
    return 0;
  }
  if (internal_format == kPixelFormatArgb64) {
    constexpr float kMaximum = 32768.0f;
    write_argb<uint16_t>(
        pixel, static_cast<uint16_t>(std::lround(alpha * kMaximum)),
        static_cast<uint16_t>(std::lround(red * kMaximum)),
        static_cast<uint16_t>(std::lround(green * kMaximum)),
        static_cast<uint16_t>(std::lround(blue * kMaximum)));
    return 0;
  }
  if (internal_format == kPixelFormatArgb128) {
    write_argb<float>(pixel, alpha, red, green, blue);
    return 0;
  }
  ++g_invalid_pixel_format_operations;
  return 4;
}

int32_t __cdecl get_black_for_pixel_format(int32_t pixel_format, void* pixel) {
  return convert_color_to_pixel_formatted_data(pixel_format, 1.0f, 0.0f,
                                                0.0f, 0.0f, pixel);
}

int32_t __cdecl get_white_for_pixel_format(int32_t pixel_format, void* pixel) {
  return convert_color_to_pixel_formatted_data(pixel_format, 1.0f, 1.0f,
                                                1.0f, 1.0f, pixel);
}

PixelFormatSuite1 g_pixel_format_suite1{
    &add_supported_pixel_format,
    &clear_supported_pixel_formats,
    &new_world_of_pixel_format,
    &dispose_pixel_format_world,
    &get_pixel_format,
    &get_black_for_pixel_format,
    &get_white_for_pixel_format,
    &convert_color_to_pixel_formatted_data};

PixelFormatSuite2 g_pixel_format_suite2{&add_supported_pixel_format,
                                        &clear_supported_pixel_formats};

PixelFormatTelemetry pixel_format_telemetry() {
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  return {g_pixel_format_add_calls, g_pixel_format_clear_calls,
          static_cast<uint32_t>(g_supported_pixel_formats.size()),
          g_invalid_pixel_format_operations.load()};
}

bool verify_pixel_format_registry_rejection() {
  const uint32_t invalid_before = g_invalid_pixel_format_operations;
  const uint32_t add_before = g_pixel_format_add_calls;
  const uint32_t clear_before = g_pixel_format_clear_calls;
  const bool phase_rejected = clear_supported_pixel_formats(&g_effect) != 0;
  g_global_setup_active = true;
  if (clear_supported_pixel_formats(&g_effect) != 0 ||
      add_supported_pixel_format(&g_effect, kPublicPixelFormatArgb128) != 0 ||
      add_supported_pixel_format(&g_effect, kPublicPixelFormatArgb64) != 0 ||
      add_supported_pixel_format(&g_effect, kPublicPixelFormatArgb128) != 0 ||
      add_supported_pixel_format(&g_effect, kPublicPixelFormatArgb32) != 0) {
    g_global_setup_active = false;
    return false;
  }
  bool order_valid = false;
  {
    std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
    order_valid = g_supported_pixel_formats ==
        std::vector<int32_t>{kPublicPixelFormatArgb128,
                             kPublicPixelFormatArgb64,
                             kPublicPixelFormatArgb32};
  }
  const bool rejected = add_supported_pixel_format(&g_effect, 0) != 0;
  const bool cleared = clear_supported_pixel_formats(&g_effect) == 0;
  g_global_setup_active = false;
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  return phase_rejected && order_valid && rejected && cleared &&
      g_supported_pixel_formats.empty() &&
      g_invalid_pixel_format_operations == invalid_before + 2 &&
      g_pixel_format_add_calls == add_before + 4 &&
      g_pixel_format_clear_calls == clear_before + 2;
}

}  // namespace aexcompat::l2_detail
