#include "worker_pf_world_transform_runtime.hpp"

#include "worker_pf_suites_internal.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <iostream>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace aexcompat::pf_world_transform {
namespace {

using world_safety::DispatchWorldFormat;
using world_safety::DispatchWorldFormatScope;
using world_safety::kEffectWorldSize;

constexpr int32_t kPfErrBadCallbackParam = 516;
constexpr int32_t kPixelFormatArgb32 = 1650946657;
using aexcompat::world_registry::kPixelFormatArgb64;
using aexcompat::world_registry::kPixelFormatArgb128;
constexpr uint64_t kMaxAsyncReceiptBytes = 64ULL * 1024 * 1024;
constexpr std::size_t kUtilsSize = 552;
constexpr std::size_t kUtilsFill = 72;
constexpr std::size_t kUtilsPremultiply = 96;
constexpr std::size_t kUtilsPremultiplyColor = 104;
constexpr std::size_t kUtilsFill16 = 488;
constexpr std::size_t kUtilsPremultiplyColor16 = 496;

Context g_context{};
bool g_configured{};
std::atomic<bool> g_fail_next_allocation_for_self_test{};

bool resolve_world(void* world, int32_t pixel_bytes, unsigned char*& pixels,
                   int32_t& rowbytes, int32_t& width, int32_t& height) {
  return g_configured && g_context.hooks.resolve_world &&
      g_context.hooks.resolve_world(world, pixel_bytes, pixels, rowbytes, width, height);
}

bool resolve_dispatch_world_format(const void* world, DispatchWorldFormat& result) {
  return g_configured && g_context.hooks.resolve_dispatch_world_format &&
      g_context.hooks.resolve_dispatch_world_format(world, result);
}

const char* pixel_format() {
  return g_configured && g_context.hooks.pixel_format ? g_context.hooks.pixel_format() : "";
}

bool set_pixel_format(const char* value) {
  return g_configured && g_context.hooks.set_pixel_format &&
      g_context.hooks.set_pixel_format(value);
}

bool bounded_argb8(void* world, unsigned char*& pixels, int32_t& rowbytes,
                   int32_t& width, int32_t& height) {
  return g_configured && g_context.hooks.bounded_argb8_world &&
      g_context.hooks.bounded_argb8_world(world, pixels, rowbytes, width, height);
}

// The requested rectangle intersected with the world. False only when there is
// no world to intersect with; anything else clips, including a rectangle that
// lands wholly outside, which clips to empty.
bool clip_legacy_rect(const LegacyRect* requested, int32_t width, int32_t height,
                      LegacyRect& result) {
  if (width <= 0 || height <= 0) return false;
  const LegacyRect wanted = requested ? *requested : LegacyRect{0, 0, width, height};
  result.left = std::clamp(wanted.left, 0, width);
  result.top = std::clamp(wanted.top, 0, height);
  result.right = std::clamp(wanted.right, result.left, width);
  result.bottom = std::clamp(wanted.bottom, result.top, height);
  return true;
}

bool normalize_legacy_rect(const LegacyRect* requested, int32_t width, int32_t height,
                           LegacyRect& result) {
  if (width <= 0 || height <= 0) return false;
  result = requested ? *requested : LegacyRect{0, 0, width, height};
  return result.left >= 0 && result.top >= 0 && result.right >= result.left &&
      result.bottom >= result.top && result.right <= width && result.bottom <= height;
}

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  if (offset > N || sizeof(value) > N - offset) return value;
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  if (offset > N || sizeof(value) > N - offset) return;
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

}  // namespace

void configure(const Context& context) {
  g_context = context;
  g_configured = context.hooks.resolve_world &&
      context.hooks.resolve_dispatch_world_format && context.hooks.pixel_format &&
      context.hooks.set_pixel_format && context.hooks.bounded_argb8_world &&
      context.telemetry.calls &&
      context.telemetry.last_x && context.telemetry.last_y &&
      context.telemetry.last_opacity;
}

bool configured() noexcept { return g_configured; }


int32_t fill_world_typed(int32_t pixel_bytes, const void* color,
                         const LegacyRect* requested, void* world) {
  unsigned char* pixels{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(world, pixel_bytes, pixels, rowbytes, width, height))
    return kPfErrBadCallbackParam;
  const std::array<unsigned char, 16> transparent_black{};
  if (!color) color = transparent_black.data();
  LegacyRect bounds{};
  if (!normalize_legacy_rect(requested, width, height, bounds))
    return kPfErrBadCallbackParam;
  for (int32_t y = bounds.top; y < bounds.bottom; ++y)
    for (int32_t x = bounds.left; x < bounds.right; ++x)
      std::memcpy(pixels + static_cast<std::size_t>(y) * rowbytes +
                      static_cast<std::size_t>(x) * pixel_bytes,
                  color, pixel_bytes);
  return 0;
}

int32_t __cdecl fill_world8(void*, const void* color, const LegacyRect* area, void* world) {
  if (std::strcmp(pixel_format(), "argb16") == 0) {
    std::array<uint16_t, 4> deep{};
    if (color) for (int channel = 0; channel < 4; ++channel)
      deep[channel] = static_cast<uint16_t>(
          (static_cast<uint32_t>(static_cast<const uint8_t*>(color)[channel]) * 32768u + 127u) /
          255u);
    return fill_world_typed(8, color ? deep.data() : nullptr, area, world);
  }
  if (std::strcmp(pixel_format(), "argb32f") == 0) {
    std::array<float, 4> floating{};
    if (color) for (int channel = 0; channel < 4; ++channel)
      floating[channel] = static_cast<const uint8_t*>(color)[channel] / 255.0f;
    return fill_world_typed(16, color ? floating.data() : nullptr, area, world);
  }
  return fill_world_typed(4, color, area, world);
}
int32_t __cdecl fill_world16(void*, const void* color, const LegacyRect* area, void* world) {
  return fill_world_typed(8, color, area, world);
}
int32_t __cdecl fill_world_float(void*, const void* color, const LegacyRect* area, void* world) {
  return fill_world_typed(16, color, area, world);
}

int32_t premultiply_color_typed(int32_t pixel_bytes, void* source_world, const void* matte,
                                int32_t forward, void* destination_world) {
  if (!source_world || !destination_world || !matte) return kPfErrBadCallbackParam;
  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if (!resolve_world(source_world, pixel_bytes, source, source_rowbytes,
                           source_width, source_height) ||
      !resolve_world(destination_world, pixel_bytes, destination, destination_rowbytes,
                           destination_width, destination_height) ||
      source_width != destination_width || source_height != destination_height)
    return kPfErrBadCallbackParam;
  const std::size_t packed_row = static_cast<std::size_t>(source_width) * pixel_bytes;
  if (packed_row > SIZE_MAX / static_cast<std::size_t>(source_height))
    return kPfErrBadCallbackParam;
  std::vector<unsigned char> snapshot;
  try {
    snapshot.resize(packed_row * static_cast<std::size_t>(source_height));
  } catch (const std::bad_alloc&) {
    return 4;
  } catch (...) {
    return kPfErrBadCallbackParam;
  }
  for (int32_t y = 0; y < source_height; ++y)
    std::memcpy(snapshot.data() + static_cast<std::size_t>(y) * packed_row,
                source + static_cast<std::size_t>(y) * source_rowbytes, packed_row);
  const double maximum = pixel_bytes == 4 ? 255.0 : (pixel_bytes == 8 ? 32768.0 : 1.0);
  const auto read_value = [&](const unsigned char* pixel, int channel) {
    return pixel_bytes == 4 ? static_cast<double>(pixel[channel]) :
        (pixel_bytes == 8 ? static_cast<double>(reinterpret_cast<const uint16_t*>(pixel)[channel]) :
                            static_cast<double>(reinterpret_cast<const float*>(pixel)[channel]));
  };
  const auto write_value = [&](unsigned char* pixel, int channel, double value) {
    if (pixel_bytes == 4)
      pixel[channel] = static_cast<uint8_t>(std::clamp(std::lround(value), 0l, 255l));
    else if (pixel_bytes == 8)
      reinterpret_cast<uint16_t*>(pixel)[channel] = static_cast<uint16_t>(
          std::clamp(std::lround(value), 0l, 32768l));
    else
      reinterpret_cast<float*>(pixel)[channel] = static_cast<float>(value);
  };
  const auto* matte_bytes = static_cast<const unsigned char*>(matte);
  for (int32_t y = 0; y < source_height; ++y) {
    for (int32_t x = 0; x < source_width; ++x) {
      const auto* input = snapshot.data() + static_cast<std::size_t>(y) * packed_row +
          static_cast<std::size_t>(x) * pixel_bytes;
      auto* output = destination + static_cast<std::size_t>(y) * destination_rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      const double alpha_value = read_value(input, 0);
      const double alpha = alpha_value / maximum;
      write_value(output, 0, alpha_value);
      for (int channel = 1; channel < 4; ++channel) {
        const double input_value = read_value(input, channel);
        const double matte_value = read_value(matte_bytes, channel);
        const double result = forward
            ? input_value * alpha + matte_value * (1.0 - alpha)
            : (alpha > 0.0
                ? (input_value - matte_value * (1.0 - alpha)) / alpha : 0.0);
        write_value(output, channel, result);
      }
    }
  }
  return 0;
}

int32_t __cdecl premultiply_world8(void*, int32_t forward, void* world) {
  if (std::strcmp(pixel_format(), "argb16") == 0) {
    const std::array<uint16_t, 4> black{};
    return premultiply_color_typed(8, world, black.data(), forward, world);
  }
  if (std::strcmp(pixel_format(), "argb32f") == 0) {
    const std::array<float, 4> black{};
    return premultiply_color_typed(16, world, black.data(), forward, world);
  }
  const std::array<uint8_t, 4> black{};
  return premultiply_color_typed(4, world, black.data(), forward, world);
}
int32_t __cdecl premultiply_color8(void*, void* source, const void* color,
                                   int32_t forward, void* destination) {
  return premultiply_color_typed(4, source, color, forward, destination);
}
int32_t __cdecl premultiply_color16(void*, void* source, const void* color,
                                    int32_t forward, void* destination) {
  return premultiply_color_typed(8, source, color, forward, destination);
}
int32_t __cdecl premultiply_color_float(void*, void* source, const void* color,
                                        int32_t forward, void* destination) {
  return premultiply_color_typed(16, source, color, forward, destination);
}

static_assert(kUtilsFill == 9 * sizeof(void*));
static_assert(kUtilsPremultiply == 12 * sizeof(void*));
static_assert(kUtilsPremultiplyColor == 13 * sizeof(void*));
static_assert(kUtilsFill16 == 61 * sizeof(void*));
static_assert(kUtilsPremultiplyColor16 == 62 * sizeof(void*));
static_assert(kUtilsSize == 69 * sizeof(void*));

void wire_legacy_fill_matte_callbacks(std::array<std::byte, kUtilsSize>& utils) {
  write(utils, kUtilsFill, &fill_world8);
  write(utils, kUtilsPremultiply, &premultiply_world8);
  write(utils, kUtilsPremultiplyColor, &premultiply_color8);
  write(utils, kUtilsFill16, &fill_world16);
  write(utils, kUtilsPremultiplyColor16, &premultiply_color16);
}

bool verify_legacy_fill_matte_callbacks() {
  std::array<std::byte, kUtilsSize> utils{};
  wire_legacy_fill_matte_callbacks(utils);
  using Fill8 = int32_t(__cdecl*)(void*, const void*, const LegacyRect*, void*);
  using Fill16 = int32_t(__cdecl*)(void*, const void*, const LegacyRect*, void*);
  using Premultiply = int32_t(__cdecl*)(void*, int32_t, void*);
  using PremultiplyColor = int32_t(__cdecl*)(void*, void*, const void*, int32_t, void*);
  const auto fill8 = read<Fill8>(utils, kUtilsFill);
  const auto fill16 = read<Fill16>(utils, kUtilsFill16);
  const auto premultiply = read<Premultiply>(utils, kUtilsPremultiply);
  const auto premultiply8 = read<PremultiplyColor>(utils, kUtilsPremultiplyColor);
  const auto premultiply16 = read<PremultiplyColor>(utils, kUtilsPremultiplyColor16);
  if (fill8 != &fill_world8 || fill16 != &fill_world16 ||
      premultiply != &premultiply_world8 || premultiply8 != &premultiply_color8 ||
      premultiply16 != &premultiply_color16) return false;

  const char* active_format = pixel_format();
  if (!active_format) return false;
  const std::string saved_format = active_format;
  if (!set_pixel_format("argb8")) return false;
  std::array<uint8_t, 24> guarded8{};
  guarded8.fill(0xa5);
  LocalEffectWorld world8{};
  world8.data = guarded8.data() + 4;
  world8.rowbytes = 8;
  world8.width = 2;
  world8.height = 2;
  const std::array<uint8_t, 4> color8{{255, 10, 20, 30}};
  const LegacyRect one_pixel{1, 0, 2, 1};
  bool ok = fill8(nullptr, color8.data(), &one_pixel, &world8) == 0 &&
      std::all_of(guarded8.begin(), guarded8.begin() + 4,
                  [](uint8_t value) { return value == 0xa5; }) &&
      std::all_of(guarded8.end() - 4, guarded8.end(),
                  [](uint8_t value) { return value == 0xa5; }) &&
      std::memcmp(guarded8.data() + 8, color8.data(), color8.size()) == 0;
  const auto before_error = guarded8;
  LegacyRect invalid_rect{0, 0, 3, 1};
  ok = ok && fill8(nullptr, color8.data(), &invalid_rect, &world8) ==
          kPfErrBadCallbackParam &&
      guarded8 == before_error &&
      fill8(nullptr, color8.data(), nullptr, nullptr) == kPfErrBadCallbackParam;

  std::array<uint8_t, 32> guarded16{};
  guarded16.fill(0x5a);
  LocalEffectWorld world16{};
  world16.world_flags = 1;
  world16.data = guarded16.data() + 8;
  world16.rowbytes = 16;
  world16.width = 2;
  world16.height = 1;
  const std::array<uint16_t, 4> color16{{32768, 1024, 2048, 4096}};
  ok = ok && fill16(nullptr, color16.data(), nullptr, &world16) == 0 &&
      std::memcmp(guarded16.data() + 8, color16.data(), sizeof(color16)) == 0 &&
      std::memcmp(guarded16.data() + 16, color16.data(), sizeof(color16)) == 0 &&
      std::all_of(guarded16.begin(), guarded16.begin() + 8,
                  [](uint8_t value) { return value == 0x5a; }) &&
      std::all_of(guarded16.end() - 8, guarded16.end(),
                  [](uint8_t value) { return value == 0x5a; });
  ok = ok && premultiply(nullptr, 1, &world8) == 0 &&
      premultiply8(nullptr, nullptr, color8.data(), 1, &world8) ==
          kPfErrBadCallbackParam &&
      premultiply16(nullptr, &world16, nullptr, 1, &world16) ==
          kPfErrBadCallbackParam;
  if (!set_pixel_format(saved_format.c_str())) return false;
  return ok;
}

int32_t __cdecl convolve_world(void*, void* source_world, const LegacyRect* requested,
                               uint32_t flags, int32_t kernel_size, void* alpha_kernel,
                               void* red_kernel, void* green_kernel, void* blue_kernel,
                               void* destination_world) {
  constexpr uint32_t kOneDimensional = 1u << 0;
  constexpr uint32_t kNormalized = 1u << 1;
  constexpr uint32_t kNoClamp = 1u << 2;
  constexpr uint32_t kUseChar = 1u << 3;
  constexpr uint32_t kUseFixed = 1u << 4;
  constexpr uint32_t kVertical = 1u << 5;
  constexpr uint32_t kReplicateBorders = 1u << 6;
  constexpr uint32_t kAlphaWeighted = 1u << 7;
  constexpr uint32_t kKnownFlags = (1u << 8) - 1;
  const char* active_format = pixel_format();
  if (!active_format) return kPfErrBadCallbackParam;
  const int32_t pixel_bytes = std::strcmp(active_format, "argb32f") == 0 ? 16 :
      (std::strcmp(active_format, "argb16") == 0 ? 8 : 4);
  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if ((flags & ~kKnownFlags) || (flags & kUseChar && flags & kUseFixed) ||
      kernel_size <= 0 || kernel_size > 15 ||
      (kernel_size & 1) == 0 || !alpha_kernel || !red_kernel || !green_kernel ||
      !blue_kernel ||
      !resolve_world(source_world, pixel_bytes, source, source_rowbytes,
                           source_width, source_height) ||
      !resolve_world(destination_world, pixel_bytes, destination,
                           destination_rowbytes, destination_width, destination_height) ||
      source_width != destination_width || source_height != destination_height)
    return kPfErrBadCallbackParam;
  LegacyRect bounds{};
  if (!normalize_legacy_rect(requested, source_width, source_height, bounds))
    return kPfErrBadCallbackParam;
  const auto kernels = std::array<const void*, 4>{
      alpha_kernel, red_kernel, green_kernel, blue_kernel};
  const bool one_dimensional = (flags & kOneDimensional) != 0;
  const int32_t tap_count = one_dimensional ? kernel_size : kernel_size * kernel_size;
  const double coefficient_scale = flags & kUseFixed ? 65536.0 : 255.0;
  const auto coefficient = [&](int channel, int index) -> double {
    if (flags & kUseChar)
      return static_cast<const uint8_t*>(kernels[channel])[index];
    if (flags & kUseFixed)
      return static_cast<const int32_t*>(kernels[channel])[index];
    return static_cast<const int32_t*>(kernels[channel])[index];
  };
  std::array<double, 4> divisors{};
  for (int channel = 0; channel < 4; ++channel) {
    double sum = 0.0;
    for (int index = 0; index < tap_count; ++index) sum += coefficient(channel, index);
    divisors[channel] = flags & kNormalized ? sum : coefficient_scale * tap_count;
    if (std::abs(divisors[channel]) < 1e-12) return kPfErrBadCallbackParam;
  }
  const uint64_t packed_rowbytes = static_cast<uint64_t>(source_width) * pixel_bytes;
  const uint64_t source_bytes = packed_rowbytes * source_height;
  if (!source_bytes || source_bytes > kMaxAsyncReceiptBytes)
    return kPfErrBadCallbackParam;
  std::vector<unsigned char> snapshot;
  try {
    snapshot.resize(static_cast<std::size_t>(source_bytes));
  } catch (const std::bad_alloc&) {
    return 4;
  } catch (...) {
    return kPfErrBadCallbackParam;
  }
  for (int32_t y = 0; y < source_height; ++y)
    std::memcpy(snapshot.data() + static_cast<std::size_t>(y) * packed_rowbytes,
                source + static_cast<std::size_t>(y) * source_rowbytes,
                static_cast<std::size_t>(packed_rowbytes));
  const int32_t radius = kernel_size / 2;
  const double maximum = pixel_bytes == 4 ? 255.0 : (pixel_bytes == 8 ? 32768.0 : 1.0);
  const auto sample = [&](int32_t x, int32_t y, int channel, bool& present) -> double {
    present = x >= 0 && x < source_width && y >= 0 && y < source_height;
    if (!present && !(flags & kReplicateBorders)) return 0.0;
    x = std::clamp(x, 0, source_width - 1);
    y = std::clamp(y, 0, source_height - 1);
    const auto* pixel = snapshot.data() + static_cast<std::size_t>(y) * packed_rowbytes +
        static_cast<std::size_t>(x) * pixel_bytes;
    if (pixel_bytes == 4) return pixel[channel];
    if (pixel_bytes == 8) return reinterpret_cast<const uint16_t*>(pixel)[channel];
    return reinterpret_cast<const float*>(pixel)[channel];
  };
  const auto store = [&](unsigned char* pixel, int channel, double value) {
    if (flags & kNoClamp) {
      if (pixel_bytes == 4) pixel[channel] = static_cast<uint8_t>(std::llround(value));
      else if (pixel_bytes == 8) reinterpret_cast<uint16_t*>(pixel)[channel] =
          static_cast<uint16_t>(std::llround(value));
      else reinterpret_cast<float*>(pixel)[channel] = static_cast<float>(value);
      return;
    }
    value = std::clamp(value, 0.0, maximum);
    if (pixel_bytes == 4) pixel[channel] = static_cast<uint8_t>(std::lround(value));
    else if (pixel_bytes == 8) reinterpret_cast<uint16_t*>(pixel)[channel] =
        static_cast<uint16_t>(std::lround(value));
    else reinterpret_cast<float*>(pixel)[channel] = static_cast<float>(value);
  };
  for (int32_t y = bounds.top; y < bounds.bottom; ++y) {
    for (int32_t x = bounds.left; x < bounds.right; ++x) {
      auto* output_pixel = destination + static_cast<std::size_t>(y) * destination_rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      double output_alpha = 0.0;
      for (int32_t channel = 0; channel < 4; ++channel) {
        double sum = 0.0;
        for (int32_t index = 0; index < tap_count; ++index) {
          const int32_t kernel_y = one_dimensional
              ? ((flags & kVertical) ? index : radius) : index / kernel_size;
          const int32_t kernel_x = one_dimensional
              ? ((flags & kVertical) ? radius : index) : index % kernel_size;
          bool present = false;
          double value = sample(x + kernel_x - radius, y + kernel_y - radius,
                                channel, present);
          if ((flags & kAlphaWeighted) && channel > 0) {
            bool alpha_present = false;
            const double alpha = sample(x + kernel_x - radius, y + kernel_y - radius,
                                        0, alpha_present) / maximum;
            value *= alpha;
          }
          sum += value * coefficient(channel, index);
        }
        double value = sum / divisors[channel];
        if (channel == 0) output_alpha = value;
        else if ((flags & kAlphaWeighted) && output_alpha > 0.0)
          value /= output_alpha / maximum;
        else if ((flags & kAlphaWeighted) && output_alpha <= 0.0) value = 0.0;
        store(output_pixel, channel, value);
      }
    }
  }
  return 0;
}

int32_t __cdecl blend_world(void*, const void* source_world1, const void* source_world2,
                            int32_t ratio, void* destination_world) {
  if (ratio < 0 || ratio > 65536) return kPfErrBadCallbackParam;
  DispatchWorldFormat first_info{}, second_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world1, first_info) ||
      !resolve_dispatch_world_format(source_world2, second_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      first_info.pixel_format != second_info.pixel_format ||
      first_info.pixel_format != destination_info.pixel_format ||
      first_info.width != second_info.width || first_info.width != destination_info.width ||
      first_info.height != second_info.height || first_info.height != destination_info.height ||
      first_info.width > 4096 || first_info.height > 4096)
    return kPfErrBadCallbackParam;
  const int32_t pixel_bytes = first_info.pixel_format == kPixelFormatArgb32 ? 4 :
      (first_info.pixel_format == kPixelFormatArgb64 ? 8 :
       (first_info.pixel_format == kPixelFormatArgb128 ? 16 : 0));
  if (!pixel_bytes || first_info.rowbytes < static_cast<int64_t>(first_info.width) * pixel_bytes ||
      second_info.rowbytes < static_cast<int64_t>(second_info.width) * pixel_bytes ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * pixel_bytes)
    return kPfErrBadCallbackParam;
  const std::size_t packed_row = static_cast<std::size_t>(first_info.width) * pixel_bytes;
  std::vector<unsigned char> first_copy, second_copy;
  try {
    first_copy.resize(packed_row * first_info.height);
    second_copy.resize(packed_row * first_info.height);
  } catch (const std::bad_alloc&) {
    return 4;
  } catch (...) {
    return kPfErrBadCallbackParam;
  }
  for (int32_t y = 0; y < first_info.height; ++y) {
    std::memcpy(first_copy.data() + static_cast<std::size_t>(y) * packed_row,
        static_cast<const unsigned char*>(first_info.data) +
            static_cast<std::size_t>(y) * first_info.rowbytes, packed_row);
    std::memcpy(second_copy.data() + static_cast<std::size_t>(y) * packed_row,
        static_cast<const unsigned char*>(second_info.data) +
            static_cast<std::size_t>(y) * second_info.rowbytes, packed_row);
  }
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  const double fraction = ratio / 65536.0;
  for (int32_t y = 0; y < first_info.height; ++y) {
    for (int32_t x = 0; x < first_info.width; ++x) {
      const auto* first = first_copy.data() + static_cast<std::size_t>(y) * packed_row +
          static_cast<std::size_t>(x) * pixel_bytes;
      const auto* second = second_copy.data() + static_cast<std::size_t>(y) * packed_row +
          static_cast<std::size_t>(x) * pixel_bytes;
      auto* output = destination + static_cast<std::size_t>(y) * destination_info.rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      if (pixel_bytes == 4) {
        for (int channel = 0; channel < 4; ++channel) {
          const int value = (static_cast<int>(first[channel]) * (65536 - ratio) +
                             static_cast<int>(second[channel]) * ratio + 32768) >> 16;
          output[channel] = static_cast<unsigned char>(std::clamp(value, 0, 255));
        }
      } else if (pixel_bytes == 8) {
        const auto* first16 = reinterpret_cast<const uint16_t*>(first);
        const auto* second16 = reinterpret_cast<const uint16_t*>(second);
        auto* output16 = reinterpret_cast<uint16_t*>(output);
        for (int channel = 0; channel < 4; ++channel) {
          const int64_t value = (static_cast<int64_t>(first16[channel]) * (65536 - ratio) +
                                 static_cast<int64_t>(second16[channel]) * ratio + 32768) >> 16;
          output16[channel] = static_cast<uint16_t>(std::clamp<int64_t>(value, 0, 32768));
        }
      } else {
        const auto* first32 = reinterpret_cast<const float*>(first);
        const auto* second32 = reinterpret_cast<const float*>(second);
        auto* output32 = reinterpret_cast<float*>(output);
        for (int channel = 0; channel < 4; ++channel)
          output32[channel] = static_cast<float>(first32[channel] * (1.0 - fraction) +
                                                 second32[channel] * fraction);
      }
    }
  }
  return 0;
}

bool verify_world_transform_blend() {
  DispatchWorldFormatScope formats;
  std::array<uint8_t, 8> first{{255,10,20,30, 128,40,50,60}};
  std::array<uint8_t, 8> second{{0,110,120,130, 64,140,150,160}};
  std::array<uint8_t, 8> destination{};
  LocalEffectWorld first_world{}, second_world{}, destination_world{};
  auto initialize = [](LocalEffectWorld& world, void* data) {
    world.data=data; world.rowbytes=8; world.width=2; world.height=1;
  };
  initialize(first_world, first.data()); initialize(second_world, second.data());
  initialize(destination_world, destination.data());
  if (!formats.register_world(&first_world, kPixelFormatArgb32) ||
      !formats.register_world(&second_world, kPixelFormatArgb32) ||
      !formats.register_world(&destination_world, kPixelFormatArgb32) ||
      blend_world(nullptr, &first_world, &second_world, 32768, &destination_world) != 0)
    return false;
  const std::array<uint8_t, 8> expected{{128,60,70,80, 96,90,100,110}};
  if (destination != expected) return false;
  first = {{255,10,20,30, 128,40,50,60}};
  if (blend_world(nullptr, &first_world, &second_world, 32768, &first_world) != 0 ||
      first != expected) return false;
  first = {{255,10,20,30, 128,40,50,60}};
  second = {{0,110,120,130, 64,140,150,160}};
  return blend_world(nullptr, &first_world, &second_world, 32768, &second_world) == 0 &&
      second == expected;
}

int32_t __cdecl copy_world8(void*, void* source_world, void* destination_world,
                            const LegacyRect* source_rect, const LegacyRect* destination_rect) {
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format)
    return kPfErrBadCallbackParam;
  const int32_t pixel_bytes = source_info.pixel_format == kPixelFormatArgb32 ? 4 :
      (source_info.pixel_format == kPixelFormatArgb64 ? 8 :
       (source_info.pixel_format == kPixelFormatArgb128 ? 16 : 0));
  if (!pixel_bytes || source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * pixel_bytes ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * pixel_bytes)
    return kPfErrBadCallbackParam;
  auto* source = static_cast<unsigned char*>(source_info.data);
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  // Clipped to each world rather than refused against it. AE's PF_COPY takes a
  // rectangle that may run past its world and copies the part that overlaps: an
  // effect splitting a stereo pair asks for the right half of a full-width
  // source into a half-width destination, and refusing that answered
  // PF_Err_BAD_CALLBACK_PARAM for the whole frame (3DGlasses, issue #962). The
  // copy below already takes the smaller of the two extents, so clipping is
  // what the rest of this function was written for; only the admission was not.
  // A rectangle that is inverted or lands wholly outside clips to nothing,
  // which the `copy_width <= 0` early-out answers as a no-op, and both memcpys
  // stay inside their worlds because both rects are inside them by construction.
  LegacyRect src{}, dst{};
  if (!clip_legacy_rect(source_rect, source_info.width, source_info.height, src) ||
      !clip_legacy_rect(destination_rect, destination_info.width, destination_info.height, dst))
    return kPfErrBadCallbackParam;
  const int32_t copy_width = std::min(src.right - src.left, dst.right - dst.left);
  const int32_t copy_height = std::min(src.bottom - src.top, dst.bottom - dst.top);
  if (copy_width <= 0 || copy_height <= 0) return 0;
  const std::size_t row_size = static_cast<std::size_t>(copy_width) * pixel_bytes;
  std::vector<unsigned char> temporary;
  try {
    if (g_fail_next_allocation_for_self_test.exchange(false)) throw std::bad_alloc();
    temporary.resize(row_size * copy_height);
  } catch (const std::bad_alloc&) {
    return 4;
  }
  for (int32_t row = 0; row < copy_height; ++row)
    std::memcpy(temporary.data() + static_cast<std::size_t>(row) * row_size,
                source + static_cast<std::size_t>(src.top + row) * source_info.rowbytes +
                    static_cast<std::size_t>(src.left) * pixel_bytes,
                row_size);
  for (int32_t row = 0; row < copy_height; ++row)
    std::memcpy(destination + static_cast<std::size_t>(dst.top + row) * destination_info.rowbytes +
                    static_cast<std::size_t>(dst.left) * pixel_bytes,
                temporary.data() + static_cast<std::size_t>(row) * row_size, row_size);
  return 0;
}

bool verify_bad_callback_param_contract() {
  if (fill_world8(nullptr, nullptr, nullptr, nullptr) != kPfErrBadCallbackParam ||
      blend_world(nullptr, nullptr, nullptr, -1, nullptr) != kPfErrBadCallbackParam ||
      transform_world(nullptr, 0, 0, 0, nullptr, nullptr, nullptr, nullptr, 1, 0,
                      nullptr, nullptr) != kPfErrBadCallbackParam ||
      copy_world8(nullptr, nullptr, nullptr, nullptr, nullptr) != kPfErrBadCallbackParam)
    return false;

  DispatchWorldFormatScope formats;
  std::array<uint8_t, 4> source_pixels{255, 1, 2, 3};
  std::array<uint8_t, 4> destination_pixels{4, 5, 6, 7};
  const auto destination_before = destination_pixels;
  LocalEffectWorld source{}, destination{};
  source.data = source_pixels.data();
  source.rowbytes = 4;
  source.width = 1;
  source.height = 1;
  destination.data = destination_pixels.data();
  destination.rowbytes = 4;
  destination.width = 1;
  destination.height = 1;
  if (!formats.register_world(&source, kPixelFormatArgb32) ||
      !formats.register_world(&destination, kPixelFormatArgb32))
    return false;
  g_fail_next_allocation_for_self_test.store(true);
  const int32_t allocation_result =
      copy_world8(nullptr, &source, &destination, nullptr, nullptr);
  g_fail_next_allocation_for_self_test.store(false);
  return allocation_result == 4 && destination_pixels == destination_before;
}

int32_t __cdecl transform_world(void* effect_ref, int32_t quality, uint32_t mode_flags,
                                int32_t field,
                                const void* source_world, const void* composite_mode,
                                const void* mask_world, const void* matrices,
                                int32_t matrix_count, uint8_t source_to_destination,
                                const LegacyRect* destination_rect, void* destination_world) {
  if (!effect_ref || !source_world || !composite_mode || !matrices ||
      matrix_count != 1 || source_to_destination > 1 || quality < 0 || quality > 1 ||
      mode_flags > 1 || field < 0 || field > 2) return kPfErrBadCallbackParam;
  int32_t transfer_mode{};
  uint8_t opacity{}, rgb_only{};
  uint16_t opacity16{};
  std::memcpy(&transfer_mode, composite_mode, sizeof(transfer_mode));
  std::memcpy(&opacity, static_cast<const std::byte*>(composite_mode) + 8, sizeof(opacity));
  std::memcpy(&rgb_only, static_cast<const std::byte*>(composite_mode) + 9, sizeof(rgb_only));
  std::memcpy(&opacity16, static_cast<const std::byte*>(composite_mode) + 10, sizeof(opacity16));
  if (transfer_mode != 0 || rgb_only > 1 || opacity16 > 32768)
    return kPfErrBadCallbackParam;
  std::array<double, 9> matrix{};
  std::memcpy(matrix.data(), matrices, sizeof(matrix));
  if (!std::all_of(matrix.begin(), matrix.end(),
                   [](double value) { return std::isfinite(value); }))
    return kPfErrBadCallbackParam;
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format)
    return kPfErrBadCallbackParam;
  const int32_t pixel_bytes = source_info.pixel_format == kPixelFormatArgb32 ? 4 :
      (source_info.pixel_format == kPixelFormatArgb64 ? 8 :
       (source_info.pixel_format == kPixelFormatArgb128 ? 16 : 0));
  if (!pixel_bytes || source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * pixel_bytes ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * pixel_bytes)
    return kPfErrBadCallbackParam;
  const double determinant = matrix[0] * (matrix[4] * matrix[8] - matrix[5] * matrix[7]) -
      matrix[1] * (matrix[3] * matrix[8] - matrix[5] * matrix[6]) +
      matrix[2] * (matrix[3] * matrix[7] - matrix[4] * matrix[6]);
  if (source_to_destination && std::abs(determinant) < 1e-12)
    return kPfErrBadCallbackParam;
  std::array<double, 9> sampling_matrix = matrix;
  if (source_to_destination) {
    sampling_matrix = {{
        (matrix[4]*matrix[8]-matrix[5]*matrix[7])/determinant,
        (matrix[2]*matrix[7]-matrix[1]*matrix[8])/determinant,
        (matrix[1]*matrix[5]-matrix[2]*matrix[4])/determinant,
        (matrix[5]*matrix[6]-matrix[3]*matrix[8])/determinant,
        (matrix[0]*matrix[8]-matrix[2]*matrix[6])/determinant,
        (matrix[2]*matrix[3]-matrix[0]*matrix[5])/determinant,
        (matrix[3]*matrix[7]-matrix[4]*matrix[6])/determinant,
        (matrix[1]*matrix[6]-matrix[0]*matrix[7])/determinant,
        (matrix[0]*matrix[4]-matrix[1]*matrix[3])/determinant}};
  }
  const auto destination_to_source = [&](double x, double y) {
    const double homogeneous = x * sampling_matrix[2] + y * sampling_matrix[5] +
        sampling_matrix[8];
    if (!std::isfinite(homogeneous) || std::abs(homogeneous) < 1e-12)
      return std::array<double, 2>{-1.0e9, -1.0e9};
    return std::array<double, 2>{
        (x * sampling_matrix[0] + y * sampling_matrix[3] + sampling_matrix[6]) /
            homogeneous,
        (x * sampling_matrix[1] + y * sampling_matrix[4] + sampling_matrix[7]) /
            homogeneous};
  };
  LegacyRect bounds{};
  if (!normalize_legacy_rect(destination_rect, destination_info.width,
                             destination_info.height, bounds))
    return kPfErrBadCallbackParam;
  const std::size_t packed_row = static_cast<std::size_t>(source_info.width) * pixel_bytes;
  std::vector<unsigned char> source_copy, mask_copy;
  int32_t mask_rowbytes{}, mask_width{}, mask_height{}, mask_offset_x{}, mask_offset_y{};
  uint32_t mask_flags{};
  try {
    source_copy.resize(packed_row * source_info.height);
    if (mask_world) {
      const auto* mask_bytes = static_cast<const std::byte*>(mask_world);
      void* mask_data{};
      std::memcpy(&mask_data, mask_bytes + 24, sizeof(mask_data));
      std::memcpy(&mask_rowbytes, mask_bytes + 32, sizeof(mask_rowbytes));
      std::memcpy(&mask_width, mask_bytes + 36, sizeof(mask_width));
      std::memcpy(&mask_height, mask_bytes + 40, sizeof(mask_height));
      std::memcpy(&mask_offset_x, mask_bytes + kEffectWorldSize, sizeof(mask_offset_x));
      std::memcpy(&mask_offset_y, mask_bytes + kEffectWorldSize + 4, sizeof(mask_offset_y));
      std::memcpy(&mask_flags, mask_bytes + kEffectWorldSize + 8, sizeof(mask_flags));
      if (!mask_data || mask_width <= 0 || mask_height <= 0 || mask_width > 4096 ||
          mask_height > 4096 || mask_rowbytes < static_cast<int64_t>(mask_width) * pixel_bytes ||
          (mask_flags & ~3u) != 0) return kPfErrBadCallbackParam;
      const std::size_t mask_packed_row = static_cast<std::size_t>(mask_width) * pixel_bytes;
      mask_copy.resize(mask_packed_row * mask_height);
      for (int32_t y = 0; y < mask_height; ++y)
        std::memcpy(mask_copy.data() + static_cast<std::size_t>(y) * mask_packed_row,
                    static_cast<const unsigned char*>(mask_data) +
                        static_cast<std::size_t>(y) * mask_rowbytes, mask_packed_row);
      mask_rowbytes = static_cast<int32_t>(mask_packed_row);
    }
  } catch (const std::bad_alloc&) { return 4; }
  const auto* source = static_cast<const unsigned char*>(source_info.data);
  for (int32_t y = 0; y < source_info.height; ++y)
    std::memcpy(source_copy.data() + static_cast<std::size_t>(y) * packed_row,
                source + static_cast<std::size_t>(y) * source_info.rowbytes, packed_row);
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  const double maximum = pixel_bytes == 4 ? 255.0 : (pixel_bytes == 8 ? 32768.0 : 1.0);
  const double opacity_fraction = pixel_bytes == 4 ? opacity / 255.0 : opacity16 / 32768.0;
  const auto read = [&](int x, int y, int channel) -> double {
    if (x < 0 || y < 0 || x >= source_info.width || y >= source_info.height) return 0.0;
    const auto* value = source_copy.data() + static_cast<std::size_t>(y) * packed_row +
        static_cast<std::size_t>(x) * pixel_bytes;
    return pixel_bytes == 4 ? value[channel] :
        (pixel_bytes == 8 ? reinterpret_cast<const uint16_t*>(value)[channel] :
                            reinterpret_cast<const float*>(value)[channel]);
  };
  const auto read_mask = [&](int x, int y, int channel) -> double {
    if (!mask_world || x < 0 || y < 0 || x >= mask_width || y >= mask_height) return 0.0;
    const auto* value = mask_copy.data() + static_cast<std::size_t>(y) * mask_rowbytes +
        static_cast<std::size_t>(x) * pixel_bytes;
    return pixel_bytes == 4 ? value[channel] :
        (pixel_bytes == 8 ? reinterpret_cast<const uint16_t*>(value)[channel] :
                            reinterpret_cast<const float*>(value)[channel]);
  };
  const auto write = [&](unsigned char* output, int channel, double value) {
    if (pixel_bytes == 4) output[channel] = static_cast<uint8_t>(
        std::clamp(std::lround(value), 0l, 255l));
    else if (pixel_bytes == 8) reinterpret_cast<uint16_t*>(output)[channel] =
        static_cast<uint16_t>(std::clamp(std::lround(value), 0l, 32768l));
    else reinterpret_cast<float*>(output)[channel] = static_cast<float>(value);
  };
  const auto floor_to_sample_coord = [](double value, int* result) -> bool {
    if (!result || !std::isfinite(value)) return false;
    const double floored = std::floor(value);
    constexpr double minimum = static_cast<double>(std::numeric_limits<int>::min()) + 1.0;
    constexpr double maximum = static_cast<double>(std::numeric_limits<int>::max()) - 1.0;
    if (floored < minimum || floored > maximum) return false;
    *result = static_cast<int>(floored);
    return true;
  };
  for (int32_t y = bounds.top; y < bounds.bottom; ++y) {
    if ((field == 1 && (y & 1)) || (field == 2 && !(y & 1))) continue;
    for (int32_t x = bounds.left; x < bounds.right; ++x) {
      const auto mapped = destination_to_source(x + 0.5, y + 0.5);
      const double source_x = mapped[0] - 0.5, source_y = mapped[1] - 0.5;
      std::array<double, 4> sampled{};
      if (quality == 0) {
        int sx = 0, sy = 0;
        if (floor_to_sample_coord(source_x + 0.5, &sx) &&
            floor_to_sample_coord(source_y + 0.5, &sy)) {
          for (int channel = 0; channel < 4; ++channel)
            sampled[channel] = read(sx, sy, channel);
        }
      } else {
        int x0 = 0, y0 = 0;
        if (floor_to_sample_coord(source_x, &x0) && floor_to_sample_coord(source_y, &y0)) {
          const double fx = source_x - x0, fy = source_y - y0;
          const std::array<double, 4> weights{{(1-fx)*(1-fy), fx*(1-fy), (1-fx)*fy, fx*fy}};
          const std::array<int, 4> xs{{x0, x0+1, x0, x0+1}}, ys{{y0, y0, y0+1, y0+1}};
          for (int tap = 0; tap < 4; ++tap) sampled[0] += read(xs[tap], ys[tap], 0) * weights[tap];
          for (int channel = 1; channel < 4; ++channel) {
            for (int tap = 0; tap < 4; ++tap) {
              double value = read(xs[tap], ys[tap], channel);
              if (mode_flags == 1) value *= read(xs[tap], ys[tap], 0) / maximum;
              sampled[channel] += value * weights[tap];
            }
            if (mode_flags == 1 && sampled[0] > 0.0)
              sampled[channel] /= sampled[0] / maximum;
          }
        }
      }
      double coverage = 1.0;
      if (mask_world) {
        const double mask_x = source_x - mask_offset_x;
        const double mask_y = source_y - mask_offset_y;
        auto mask_value = [&](int mx, int my) {
          if (mask_flags & 2u)
            return (0.299 * read_mask(mx, my, 1) + 0.587 * read_mask(mx, my, 2) +
                    0.114 * read_mask(mx, my, 3)) / maximum;
          return read_mask(mx, my, 0) / maximum;
        };
        if (quality == 0) {
          int mx = 0, my = 0;
          coverage = floor_to_sample_coord(mask_x + 0.5, &mx) &&
                     floor_to_sample_coord(mask_y + 0.5, &my) ? mask_value(mx, my) : 0.0;
        } else {
          int mx0 = 0, my0 = 0;
          if (floor_to_sample_coord(mask_x, &mx0) && floor_to_sample_coord(mask_y, &my0)) {
            const double mfx = mask_x - mx0, mfy = mask_y - my0;
            coverage = mask_value(mx0, my0) * (1-mfx) * (1-mfy) +
                mask_value(mx0+1, my0) * mfx * (1-mfy) +
                mask_value(mx0, my0+1) * (1-mfx) * mfy +
                mask_value(mx0+1, my0+1) * mfx * mfy;
          } else {
            coverage = 0.0;
          }
        }
        coverage = std::clamp(coverage, 0.0, 1.0);
        if (mask_flags & 1u) coverage = 1.0 - coverage;
      }
      auto* output = destination + static_cast<std::size_t>(y) * destination_info.rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      for (int channel = rgb_only ? 1 : 0; channel < 4; ++channel) {
        const double old = pixel_bytes == 4 ? output[channel] :
            (pixel_bytes == 8 ? reinterpret_cast<uint16_t*>(output)[channel] :
                                reinterpret_cast<float*>(output)[channel]);
        const double effective_opacity = opacity_fraction * coverage;
        write(output, channel, sampled[channel] * effective_opacity +
                               old * (1.0 - effective_opacity));
      }
    }
  }
  ++*g_context.telemetry.calls;
  *g_context.telemetry.last_x = static_cast<int32_t>(std::lround(matrix[6]));
  *g_context.telemetry.last_y = static_cast<int32_t>(std::lround(matrix[7]));
  *g_context.telemetry.last_opacity = opacity;
  return 0;
}

bool verify_world_transform_affine() {
  DispatchWorldFormatScope formats;
  std::array<uint8_t, 3 * 2 * 4> source_pixels{};
  std::array<uint8_t, 4 * 3 * 4> destination_pixels{};
  LocalEffectWorld source{}, destination{};
  source.data = source_pixels.data(); source.rowbytes = 3 * 4; source.width = 3; source.height = 2;
  destination.data = destination_pixels.data(); destination.rowbytes = 4 * 4;
  destination.width = 4; destination.height = 3;
  for (int index = 0; index < 6; ++index) {
    source_pixels[index * 4] = 255;
    source_pixels[index * 4 + 1] = static_cast<uint8_t>((index + 1) * 10);
  }
  if (!formats.register_world(&source, kPixelFormatArgb32) ||
      !formats.register_world(&destination, kPixelFormatArgb32)) return false;
  std::array<std::byte, 12> composite{};
  const int32_t copy = 0; const uint8_t opacity = 255; const uint16_t opacity16 = 32768;
  std::memcpy(composite.data(), &copy, sizeof(copy));
  std::memcpy(composite.data() + 8, &opacity, sizeof(opacity));
  std::memcpy(composite.data() + 10, &opacity16, sizeof(opacity16));
  LegacyRect bounds{0, 0, 4, 3};
  const std::array<double, 9> source_to_destination{{1,0,0, 0,1,0, 1,1,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      source_to_destination.data(), 1, 1, &bounds, &destination) != 0)
    return false;
  const auto red = [&](int x, int y) { return destination_pixels[(y * 4 + x) * 4 + 1]; };
  if (red(1,1) != 10 || red(2,1) != 20 || red(3,1) != 30 ||
      red(1,2) != 40 || red(2,2) != 50 || red(3,2) != 60 || red(0,0) != 0) return false;
  destination_pixels.fill(0);
  const std::array<double, 9> destination_to_source{{1,0,0, 0,1,0, -1,-1,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      destination_to_source.data(), 1, 0, &bounds, &destination) != 0 ||
      red(1,1) != 10 || red(3,2) != 60) return false;
  destination_pixels.fill(0);
  const std::array<double, 9> scale{{2,0,0, 0,2,0, 0,0,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      scale.data(), 1, 1, &bounds, &destination) != 0 ||
      red(0,0) != 10 || red(1,0) != 10 || red(2,0) != 20 || red(3,0) != 20 ||
      red(0,1) != 10 || red(1,1) != 10 || red(2,1) != 20 || red(3,1) != 20 ||
      red(0,2) != 40 || red(1,2) != 40 || red(2,2) != 50 || red(3,2) != 50) return false;
  destination_pixels.fill(0);
  std::array<uint8_t, 3 * 2 * 4> mask_pixels{};
  for (int y = 0; y < 2; ++y) {
    mask_pixels[(y * 3) * 4] = 0;
    mask_pixels[(y * 3 + 1) * 4] = 128;
    mask_pixels[(y * 3 + 2) * 4] = 255;
  }
  std::array<std::byte, kEffectWorldSize + 12> mask{};
  void* mask_data = mask_pixels.data();
  const int32_t mask_rowbytes = 12, mask_width = 3, mask_height = 2;
  std::memcpy(mask.data() + 24, &mask_data, sizeof(mask_data));
  std::memcpy(mask.data() + 32, &mask_rowbytes, sizeof(mask_rowbytes));
  std::memcpy(mask.data() + 36, &mask_width, sizeof(mask_width));
  std::memcpy(mask.data() + 40, &mask_height, sizeof(mask_height));
  const std::array<double, 9> identity{{1,0,0, 0,1,0, 0,0,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), mask.data(),
                      identity.data(), 1, 1, &bounds, &destination) != 0 ||
      red(0,0) != 0 || red(1,0) != 10 || red(2,0) != 30 ||
      red(0,1) != 0 || red(1,1) != 25 || red(2,1) != 60) return false;
  const auto before_invalid_mask = destination_pixels;
  const uint32_t invalid_mask_flags = 4;
  std::memcpy(mask.data() + kEffectWorldSize + 8, &invalid_mask_flags,
              sizeof(invalid_mask_flags));
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), mask.data(),
                      identity.data(), 1, 1, &bounds, &destination) !=
          kPfErrBadCallbackParam || destination_pixels != before_invalid_mask) return false;
  destination_pixels.fill(0);
  const std::array<double, 9> projective_destination_to_source{{
      1,0,0.25, 0,1,0, 0,0,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      projective_destination_to_source.data(), 1, 0, &bounds,
                      &destination) != 0 || red(0,0) != 10 || red(1,0) != 20 ||
      red(2,0) != 20 || red(3,0) != 20) return false;
  const auto direct_projective_result = destination_pixels;
  destination_pixels.fill(0);
  const std::array<double, 9> projective_source_to_destination{{
      1,0,-0.25, 0,1,0, 0,0,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      projective_source_to_destination.data(), 1, 1, &bounds,
                      &destination) != 0 || destination_pixels != direct_projective_result)
    return false;
  return true;
}

template <typename Channel, int32_t PixelFormat>
int32_t transfer_rect_registered(int32_t quality, uint32_t mode_flags, int32_t field,
                                 const LegacyRect* source_rect, const void* source_world,
                                 int32_t transfer_mode, int32_t random_seed,
                                 uint8_t opacity8, uint8_t rgb_only,
                                 uint16_t opacity16, const void* mask_world,
                                 int32_t destination_x,
                                 int32_t destination_y, void* destination_world) {
  constexpr double maximum = std::is_same_v<Channel, uint8_t> ? 255.0 :
      (std::is_same_v<Channel, uint16_t> ? 32768.0 : 1.0);
  DispatchWorldFormat source_info{}, destination_info{};
  if (quality < 0 || quality > 1 || mode_flags > 1 || field < 0 || field > 2 ||
      !resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != PixelFormat || destination_info.pixel_format != PixelFormat ||
      source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * sizeof(Channel) * 4 ||
      destination_info.rowbytes <
          static_cast<int64_t>(destination_info.width) * sizeof(Channel) * 4)
    return kPfErrBadCallbackParam;
  LegacyRect bounds{};
  if (!normalize_legacy_rect(source_rect, source_info.width, source_info.height, bounds))
    return kPfErrBadCallbackParam;
  const int64_t clipped_left = (std::max<int64_t>)(bounds.left,
      static_cast<int64_t>(bounds.left) - destination_x);
  const int64_t clipped_top = (std::max<int64_t>)(bounds.top,
      static_cast<int64_t>(bounds.top) - destination_y);
  const int64_t clipped_right = (std::min<int64_t>)(bounds.right,
      static_cast<int64_t>(bounds.left) - destination_x + destination_info.width);
  const int64_t clipped_bottom = (std::min<int64_t>)(bounds.bottom,
      static_cast<int64_t>(bounds.top) - destination_y + destination_info.height);
  if (clipped_right <= clipped_left || clipped_bottom <= clipped_top) return 0;
  const std::size_t width = static_cast<std::size_t>(clipped_right - clipped_left);
  const std::size_t height = static_cast<std::size_t>(clipped_bottom - clipped_top);
  if (width > SIZE_MAX / height || width * height > 16'777'216)
    return kPfErrBadCallbackParam;
  using Pixel = std::array<Channel, 4>;
  std::vector<Pixel> snapshot;
  std::vector<double> mask_coverage;
  try {
    snapshot.resize(width * height);
    if (mask_world) mask_coverage.resize(width * height);
  } catch (...) { return kPfErrBadCallbackParam; }
  const auto* source = static_cast<const unsigned char*>(source_info.data);
  for (std::size_t row = 0; row < height; ++row)
    std::memcpy(snapshot.data() + row * width,
        source + (static_cast<std::size_t>(clipped_top) + row) * source_info.rowbytes +
            static_cast<std::size_t>(clipped_left) * sizeof(Pixel), width * sizeof(Pixel));
  if (mask_world) {
    const auto* mask_bytes = static_cast<const std::byte*>(mask_world);
    void* mask_data{};
    int32_t mask_rowbytes{}, mask_width{}, mask_height{}, mask_offset_x{}, mask_offset_y{};
    uint32_t mask_flags{};
    std::memcpy(&mask_data, mask_bytes + 24, sizeof(mask_data));
    std::memcpy(&mask_rowbytes, mask_bytes + 32, sizeof(mask_rowbytes));
    std::memcpy(&mask_width, mask_bytes + 36, sizeof(mask_width));
    std::memcpy(&mask_height, mask_bytes + 40, sizeof(mask_height));
    std::memcpy(&mask_offset_x, mask_bytes + kEffectWorldSize, sizeof(mask_offset_x));
    std::memcpy(&mask_offset_y, mask_bytes + kEffectWorldSize + 4, sizeof(mask_offset_y));
    std::memcpy(&mask_flags, mask_bytes + kEffectWorldSize + 8, sizeof(mask_flags));
    if (!mask_data || mask_width <= 0 || mask_height <= 0 || mask_width > 4096 ||
        mask_height > 4096 || mask_rowbytes < static_cast<int64_t>(mask_width) * sizeof(Pixel) ||
        (mask_flags & ~3u) != 0) return kPfErrBadCallbackParam;
    const auto* mask_pixels = static_cast<const unsigned char*>(mask_data);
    for (std::size_t row = 0; row < height; ++row) {
      const int64_t mask_y = clipped_top + static_cast<int64_t>(row) - mask_offset_y;
      for (std::size_t column = 0; column < width; ++column) {
        const int64_t mask_x = clipped_left + static_cast<int64_t>(column) - mask_offset_x;
        double coverage = 0.0;
        if (mask_x >= 0 && mask_y >= 0 && mask_x < mask_width && mask_y < mask_height) {
          const auto* pixel = reinterpret_cast<const Pixel*>(mask_pixels +
              static_cast<std::size_t>(mask_y) * mask_rowbytes +

              static_cast<std::size_t>(mask_x) * sizeof(Pixel));
          if (mask_flags & 2u) {
            coverage = (0.299 * (*pixel)[1] + 0.587 * (*pixel)[2] +
                        0.114 * (*pixel)[3]) / maximum;
          } else {
            coverage = static_cast<double>((*pixel)[0]) / maximum;
          }
        }
        coverage = std::clamp(coverage, 0.0, 1.0);
        if (mask_flags & 1u) coverage = 1.0 - coverage;
        mask_coverage[row * width + column] = coverage;
      }
    }
  }
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  const double opacity = std::is_same_v<Channel, uint8_t>
      ? opacity8 / 255.0 : opacity16 / 32768.0;
  const auto store = [](double value) -> Channel {
    if constexpr (std::is_same_v<Channel, float>) return static_cast<float>(value);
    else return static_cast<Channel>(std::clamp(std::lround(value), 0l,
        std::is_same_v<Channel, uint8_t> ? 255l : 32768l));
  };
  const auto clip_color = [](std::array<double, 3> color) {
    const double luminance = 0.30 * color[0] + 0.59 * color[1] + 0.11 * color[2];
    const double minimum = (std::min)({color[0], color[1], color[2]});
    const double maximum_value = (std::max)({color[0], color[1], color[2]});
    if (minimum < 0.0) for (double& component : color)
      component = luminance + (component - luminance) * luminance / (luminance - minimum);
    if (maximum_value > 1.0) for (double& component : color)
      component = luminance + (component - luminance) * (1.0 - luminance) /
          (maximum_value - luminance);
    return color;
  };
  const auto set_luminance = [&](std::array<double, 3> color, double luminance) {
    const double delta = luminance - (0.30 * color[0] + 0.59 * color[1] + 0.11 * color[2]);
    for (double& component : color) component += delta;
    return clip_color(color);
  };
  const auto saturation = [](const std::array<double, 3>& color) {
    return (std::max)({color[0], color[1], color[2]}) -
        (std::min)({color[0], color[1], color[2]});
  };
  const auto set_saturation = [](std::array<double, 3> color, double target) {
    int minimum_index = 0, maximum_index = 0;
    for (int index = 1; index < 3; ++index) {
      if (color[index] < color[minimum_index]) minimum_index = index;
      if (color[index] > color[maximum_index]) maximum_index = index;
    }
    const int middle_index = 3 - minimum_index - maximum_index;
    if (color[maximum_index] > color[minimum_index]) {
      color[middle_index] = (color[middle_index] - color[minimum_index]) * target /
          (color[maximum_index] - color[minimum_index]);
      color[maximum_index] = target;
    } else {
      color[middle_index] = color[maximum_index] = 0.0;
    }
    color[minimum_index] = 0.0;
    return color;
  };
  const auto blend_component = [](int32_t mode, double source, double destination) {
    switch (mode) {
      case 4: case 29: return source + destination;
      case 5: return source * destination;
      case 6: return source + destination - source * destination;
      case 7: return destination <= 0.5 ? 2.0 * source * destination :
          1.0 - 2.0 * (1.0 - source) * (1.0 - destination);
      case 8: return source <= 0.5 ? destination - (1.0 - 2.0 * source) * destination *
          (1.0 - destination) : destination + (2.0 * source - 1.0) *
          ((destination <= 0.25 ? ((16.0 * destination - 12.0) * destination + 4.0) *
          destination : std::sqrt((std::max)(destination, 0.0))) - destination);
      case 9: return source <= 0.5 ? 2.0 * source * destination :
          1.0 - 2.0 * (1.0 - source) * (1.0 - destination);
      case 10: return (std::min)(source, destination);
      case 11: return (std::max)(source, destination);
      case 12: case 26: return std::abs(destination - source);
      case 23: case 27: return source >= 1.0 ? 1.0 :
          (std::min)(1.0, destination / (1.0 - source));
      case 24: case 28: return source <= 0.0 ? 0.0 :
          1.0 - (std::min)(1.0, (1.0 - destination) / source);
      case 25: return source + destination - 2.0 * source * destination;
      case 30: return source + destination - 1.0;
      case 31: return source <= 0.5 ? destination + 2.0 * source - 1.0 :
          destination + 2.0 * (source - 0.5);
      case 32: return source <= 0.5 ? (source <= 0.0 ? 0.0 :
          1.0 - (std::min)(1.0, (1.0 - destination) / (2.0 * source))) :
          (source >= 1.0 ? 1.0 : (std::min)(1.0, destination / (2.0 * (1.0 - source))));
      case 33: return source <= 0.5 ? (std::min)(destination, 2.0 * source) :
          (std::max)(destination, 2.0 * source - 1.0);
      case 34: {
        const double vivid = source <= 0.5 ? (source <= 0.0 ? 0.0 :
            1.0 - (std::min)(1.0, (1.0 - destination) / (2.0 * source))) :
            (source >= 1.0 ? 1.0 :
            (std::min)(1.0, destination / (2.0 * (1.0 - source))));
        return vivid < 0.5 ? 0.0 : 1.0;
      }
      case 37: return destination - source;
      case 38: return source <= 0.0 ? 1.0 : destination / source;
      default: return source;
    }
  };
  for (std::size_t row = 0; row < height; ++row) {
    const int64_t source_y = clipped_top + static_cast<int64_t>(row);
    const int64_t output_y = destination_y + source_y - bounds.top;
    if ((field == 1 && (output_y & 1)) || (field == 2 && !(output_y & 1))) continue;
    for (std::size_t column = 0; column < width; ++column) {
      const int64_t source_x = clipped_left + static_cast<int64_t>(column);
      const int64_t output_x = destination_x + source_x - bounds.left;
      const Pixel& input = snapshot[row * width + column];
      auto* output = reinterpret_cast<Pixel*>(destination +
          static_cast<std::size_t>(output_y) * destination_info.rowbytes +
          static_cast<std::size_t>(output_x) * sizeof(Pixel));
      const double effective_opacity = opacity *
          (mask_world ? mask_coverage[row * width + column] : 1.0);
      if (transfer_mode == 0) {
        for (int channel = rgb_only ? 1 : 0; channel < 4; ++channel) {
          (*output)[channel] = store(input[channel] * effective_opacity +
                                     (*output)[channel] * (1.0 - effective_opacity));
        }
        continue;
      }
      const double raw_source_alpha = input[0] / maximum;
      if (transfer_mode >= 17 && transfer_mode <= 20) {
        const double source_luminance = (0.30 * input[1] + 0.59 * input[2] +
                                         0.11 * input[3]) / maximum;
        const double factor = transfer_mode == 17 ? raw_source_alpha :
            (transfer_mode == 18 ? source_luminance :
             (transfer_mode == 19 ? 1.0 - raw_source_alpha : 1.0 - source_luminance));
        (*output)[0] = store((*output)[0] *
            (1.0 - effective_opacity + effective_opacity * factor));
        continue;
      }
      if (transfer_mode == 22) {
        if (!rgb_only) (*output)[0] = store((*output)[0] + input[0] * effective_opacity);
        continue;
      }
      if (transfer_mode == 3) {
        uint32_t hash = static_cast<uint32_t>(random_seed) ^
            (static_cast<uint32_t>(source_x) * 0x9e3779b9u) ^
            (static_cast<uint32_t>(source_y) * 0x85ebca6bu);
        hash ^= hash >> 16; hash *= 0x7feb352du; hash ^= hash >> 15;
        if ((hash & 0x00ffffffu) >= static_cast<uint32_t>(
                std::clamp(effective_opacity, 0.0, 1.0) * 16777216.0)) continue;
      }
      const double source_alpha = raw_source_alpha *
          (transfer_mode == 3 ? 1.0 : effective_opacity);
      const double destination_alpha = (*output)[0] / maximum;
      const bool behind = transfer_mode == 1;
      if (transfer_mode >= 4 && transfer_mode != 21) {
        std::array<double, 3> source_color{}, destination_color{}, blended{};
        for (int index = 0; index < 3; ++index) {
          source_color[index] = input[index + 1] / maximum;
          destination_color[index] = (*output)[index + 1] / maximum;
        }
        if (transfer_mode >= 13 && transfer_mode <= 16) {
          if (transfer_mode == 13)
            blended = set_luminance(set_saturation(source_color,
                saturation(destination_color)), 0.30 * destination_color[0] +
                0.59 * destination_color[1] + 0.11 * destination_color[2]);
          else if (transfer_mode == 14)
            blended = set_luminance(set_saturation(destination_color,
                saturation(source_color)), 0.30 * destination_color[0] +
                0.59 * destination_color[1] + 0.11 * destination_color[2]);
          else if (transfer_mode == 15)
            blended = set_luminance(source_color, 0.30 * destination_color[0] +
                0.59 * destination_color[1] + 0.11 * destination_color[2]);
          else
            blended = set_luminance(destination_color, 0.30 * source_color[0] +
                0.59 * source_color[1] + 0.11 * source_color[2]);
        } else if (transfer_mode == 35 || transfer_mode == 36) {
          const double source_luminance = 0.30 * source_color[0] +
              0.59 * source_color[1] + 0.11 * source_color[2];
          const double destination_luminance = 0.30 * destination_color[0] +
              0.59 * destination_color[1] + 0.11 * destination_color[2];
          blended = (transfer_mode == 35 ? source_luminance > destination_luminance :
              source_luminance < destination_luminance) ? source_color : destination_color;
        } else {
          for (int index = 0; index < 3; ++index)
            blended[index] = blend_component(transfer_mode, source_color[index],
                                             destination_color[index]);
        }
        if (rgb_only) {
          for (int index = 0; index < 3; ++index)
            (*output)[index + 1] = store((destination_color[index] *
                (1.0 - effective_opacity) + blended[index] * effective_opacity) * maximum);
          continue;
        }
        for (int index = 0; index < 3; ++index) {
          const double result = (1.0 - source_alpha) * destination_color[index] +
              source_alpha * ((1.0 - destination_alpha) * source_color[index] +
                              destination_alpha * blended[index]);
          (*output)[index + 1] = store(result * maximum);
        }
        if (!rgb_only) (*output)[0] = store((source_alpha + destination_alpha *
                                             (1.0 - source_alpha)) * maximum);
        continue;
      }
      const double output_alpha = behind
          ? destination_alpha + source_alpha * (1.0 - destination_alpha)
          : source_alpha + destination_alpha * (1.0 - source_alpha);
      for (int channel = 1; channel < 4; ++channel) {
        double value = 0.0;
        if (transfer_mode == 21) {
          value = (*output)[channel] + input[channel] * effective_opacity;
        } else if (mode_flags == 1) {
          if (output_alpha > 0.0) {
            value = behind
                ? ((*output)[channel] * destination_alpha + input[channel] * source_alpha *
                    (1.0 - destination_alpha)) / output_alpha
                : (input[channel] * source_alpha + (*output)[channel] * destination_alpha *
                    (1.0 - source_alpha)) / output_alpha;
          }
        } else {
          value = behind
              ? (*output)[channel] + input[channel] * effective_opacity *
                  (1.0 - destination_alpha)
              : input[channel] * effective_opacity +
                  (*output)[channel] * (1.0 - source_alpha);
        }
        (*output)[channel] = store(value);
      }
      if (!rgb_only) (*output)[0] = store(output_alpha * maximum);
    }
  }
  return 0;
}

int32_t __cdecl copy_world_hq(void* effect_ref, void* source_world, void* destination_world,
                              const LegacyRect* source_rect,
                              const LegacyRect* destination_rect) {
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info))
    return kPfErrBadCallbackParam;
  LegacyRect source_bounds{}, destination_bounds{};
  if (!normalize_legacy_rect(source_rect, source_info.width, source_info.height, source_bounds) ||
      !normalize_legacy_rect(destination_rect, destination_info.width, destination_info.height,
                             destination_bounds) ||
      source_bounds.right - source_bounds.left !=
          destination_bounds.right - destination_bounds.left ||
      source_bounds.bottom - source_bounds.top !=
          destination_bounds.bottom - destination_bounds.top)
    return kPfErrBadCallbackParam;
  return copy_world8(effect_ref, source_world, destination_world,
                     &source_bounds, &destination_bounds);
}

int32_t __cdecl transfer_rect(void* effect_ref, int32_t quality, uint32_t mode_flags,
                              int32_t field,
                              const LegacyRect* source_rect, const void* source_world,
                              const void* composite_mode, const void* mask_world,
                              int32_t destination_x, int32_t destination_y,
                              void* destination_world) {
  if (!effect_ref || !source_world || !composite_mode ||
      destination_x < -4096 || destination_x > 4096 ||
      destination_y < -4096 || destination_y > 4096) return kPfErrBadCallbackParam;
  int32_t transfer_mode{};
  int32_t random_seed{};
  uint8_t opacity{}, rgb_only{};
  uint16_t opacity16{};
  std::memcpy(&transfer_mode, composite_mode, sizeof(transfer_mode));
  std::memcpy(&random_seed, static_cast<const std::byte*>(composite_mode) + 4,
              sizeof(random_seed));
  std::memcpy(&opacity, static_cast<const std::byte*>(composite_mode) + 8, sizeof(opacity));
  std::memcpy(&rgb_only, static_cast<const std::byte*>(composite_mode) + 9, sizeof(rgb_only));
  std::memcpy(&opacity16, static_cast<const std::byte*>(composite_mode) + 10, sizeof(opacity16));
  if (transfer_mode < 0 || transfer_mode > 38 || rgb_only > 1 || opacity16 > 32768)
    return kPfErrBadCallbackParam;
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format)
    return kPfErrBadCallbackParam;
  if (source_info.pixel_format == kPixelFormatArgb32)
    return transfer_rect_registered<uint8_t, kPixelFormatArgb32>(quality, mode_flags, field,
        source_rect, source_world, transfer_mode, random_seed, opacity, rgb_only, opacity16,
        mask_world,
        destination_x, destination_y, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb64)
    return transfer_rect_registered<uint16_t, kPixelFormatArgb64>(quality, mode_flags, field,
        source_rect, source_world, transfer_mode, random_seed, opacity, rgb_only, opacity16,
        mask_world,
        destination_x, destination_y, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb128)
    return transfer_rect_registered<float, kPixelFormatArgb128>(quality, mode_flags, field,
        source_rect, source_world, transfer_mode, random_seed, opacity, rgb_only, opacity16,
        mask_world,
        destination_x, destination_y, destination_world);
  return kPfErrBadCallbackParam;
}

bool verify_world_transform_transfer_mask() {
  DispatchWorldFormatScope formats;
  std::array<uint8_t, 12> source_pixels{255, 200, 0, 0, 255, 200, 0, 0,
                                         255, 200, 0, 0};
  std::array<uint8_t, 12> destination_pixels{};
  std::array<uint8_t, 12> mask_pixels{0, 0, 0, 0, 128, 128, 128, 128,
                                      255, 255, 255, 255};
  LocalEffectWorld source{}, destination{};
  source.data = source_pixels.data(); source.rowbytes = 12; source.width = 3; source.height = 1;
  destination.data = destination_pixels.data(); destination.rowbytes = 12;
  destination.width = 3; destination.height = 1;
  if (!formats.register_world(&source, kPixelFormatArgb32) ||
      !formats.register_world(&destination, kPixelFormatArgb32)) return false;
  std::array<std::byte, kEffectWorldSize + 12> mask{};
  void* mask_data = mask_pixels.data();
  const int32_t mask_rowbytes = 12, mask_width = 3, mask_height = 1;
  std::memcpy(mask.data() + 24, &mask_data, sizeof(mask_data));
  std::memcpy(mask.data() + 32, &mask_rowbytes, sizeof(mask_rowbytes));
  std::memcpy(mask.data() + 36, &mask_width, sizeof(mask_width));
  std::memcpy(mask.data() + 40, &mask_height, sizeof(mask_height));
  std::array<std::byte, 12> composite{};
  const int32_t in_front = 2;
  const uint8_t opacity = 255;
  const uint16_t opacity16 = 32768;
  std::memcpy(composite.data(), &in_front, sizeof(in_front));
  std::memcpy(composite.data() + 8, &opacity, sizeof(opacity));
  std::memcpy(composite.data() + 10, &opacity16, sizeof(opacity16));
  LegacyRect bounds{0, 0, 3, 1};
  auto run = [&] {
    return transfer_rect(&source, 0, 0, 0, &bounds, &source, composite.data(), mask.data(),
                         0, 0, &destination);
  };
  if (run() != 0 || destination_pixels[1] != 0 || destination_pixels[5] != 100 ||
      destination_pixels[9] != 200 || destination_pixels[0] != 0 ||
      destination_pixels[4] != 128 || destination_pixels[8] != 255) return false;
  destination_pixels.fill(0);
  uint32_t flags = 1;
  std::memcpy(mask.data() + kEffectWorldSize + 8, &flags, sizeof(flags));
  if (run() != 0 || destination_pixels[1] != 200 || destination_pixels[5] != 100 ||
      destination_pixels[9] != 0) return false;
  destination_pixels.fill(0);
  flags = 2;
  std::memcpy(mask.data() + kEffectWorldSize + 8, &flags, sizeof(flags));
  if (run() != 0 || destination_pixels[1] != 0 || destination_pixels[5] != 100 ||
      destination_pixels[9] != 200) return false;
  destination_pixels.fill(0);
  flags = 0;
  const int32_t offset_x = 1;
  std::memcpy(mask.data() + kEffectWorldSize, &offset_x, sizeof(offset_x));
  if (run() != 0 || destination_pixels[1] != 0 || destination_pixels[5] != 0 ||
      destination_pixels[9] != 100) return false;
  int32_t transfer_mode = 0;
  const uint8_t half_opacity = 128;
  const uint16_t half_opacity16 = 16384;
  std::memcpy(composite.data(), &transfer_mode, sizeof(transfer_mode));
  std::memcpy(composite.data() + 8, &half_opacity, sizeof(half_opacity));
  std::memcpy(composite.data() + 10, &half_opacity16, sizeof(half_opacity16));
  for (std::size_t pixel = 0; pixel < 3; ++pixel) {
    destination_pixels[pixel * 4] = 255;
    destination_pixels[pixel * 4 + 1] = 40;
  }
  if (transfer_rect(&source, 0, 0, 0, &bounds, &source, composite.data(), nullptr,
                    0, 0, &destination) != 0 || destination_pixels[1] != 120 ||
      destination_pixels[5] != 120 || destination_pixels[9] != 120) return false;
  transfer_mode = 1;
  std::memcpy(composite.data(), &transfer_mode, sizeof(transfer_mode));
  std::memcpy(composite.data() + 8, &opacity, sizeof(opacity));
  std::memcpy(composite.data() + 10, &opacity16, sizeof(opacity16));
  for (std::size_t pixel = 0; pixel < 3; ++pixel) {
    destination_pixels[pixel * 4] = 128;
    destination_pixels[pixel * 4 + 1] = 20;
    destination_pixels[pixel * 4 + 2] = 0;
    destination_pixels[pixel * 4 + 3] = 0;
  }
  if (transfer_rect(&source, 0, 0, 0, &bounds, &source, composite.data(), nullptr,
                    0, 0, &destination) != 0 || destination_pixels[0] != 255 ||
      destination_pixels[1] != 120) return false;
  const LegacyRect one_pixel{0, 0, 1, 1};
  const auto verify_mode = [&](int32_t mode, const std::array<uint8_t, 4>& source_pixel,
                               const std::array<uint8_t, 4>& destination_pixel,
                               const std::array<uint8_t, 4>& expected) {
    std::copy(source_pixel.begin(), source_pixel.end(), source_pixels.begin());
    std::copy(destination_pixel.begin(), destination_pixel.end(), destination_pixels.begin());
    std::memcpy(composite.data(), &mode, sizeof(mode));
    std::memcpy(composite.data() + 8, &opacity, sizeof(opacity));
    std::memcpy(composite.data() + 10, &opacity16, sizeof(opacity16));
    return transfer_rect(&source, 0, 0, 0, &one_pixel, &source, composite.data(), nullptr,
                         0, 0, &destination) == 0 &&
        std::equal(expected.begin(), expected.end(), destination_pixels.begin());
  };
  if (!verify_mode(5, {255, 128, 64, 32}, {255, 64, 128, 192},
                   {255, 32, 32, 24}) ||
      !verify_mode(6, {255, 128, 64, 32}, {255, 64, 128, 192},
                   {255, 160, 160, 200}) ||
      !verify_mode(37, {255, 128, 64, 32}, {255, 64, 128, 192},
                   {255, 0, 64, 160}) ||
      !verify_mode(17, {128, 128, 64, 32}, {200, 64, 128, 192},
                   {100, 64, 128, 192})) return false;
  destination_pixels.fill(0x5a);
  flags = 4;
  std::memcpy(mask.data() + kEffectWorldSize + 8, &flags, sizeof(flags));
  const auto before = destination_pixels;
  return run() == kPfErrBadCallbackParam && destination_pixels == before;
}


uint8_t composite_divide_255(uint32_t numerator) {
  return static_cast<uint8_t>(std::min<uint32_t>(255, (numerator + 127) / 255));
}

uint8_t composite_divide_65025(uint64_t numerator) {
  return static_cast<uint8_t>(std::min<uint64_t>(255, (numerator + 32'512) / 65'025));
}

int32_t __cdecl composite_rect8_legacy(void* effect_ref, LegacyRect* source_rect,
                                int32_t source_opacity, void* source_world,
                                int32_t destination_x, int32_t destination_y,
                                int32_t field, int32_t transfer_mode,
                                void* destination_world) {
  constexpr int32_t kFieldFrame = 0;
  constexpr int32_t kFieldUpper = 1;
  constexpr int32_t kFieldLower = 2;
  constexpr int32_t kTransferCopy = 0;
  constexpr int32_t kTransferBehind = 1;
  constexpr int32_t kTransferInFront = 2;
  if (!effect_ref || !source_rect || source_opacity < 0 || source_opacity > 255 ||
      (field != kFieldFrame && field != kFieldUpper && field != kFieldLower) ||
      (transfer_mode != kTransferCopy && transfer_mode != kTransferBehind &&
       transfer_mode != kTransferInFront)) {
    return kPfErrBadCallbackParam;
  }

  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if (!bounded_argb8(source_world, source, source_rowbytes, source_width, source_height) ||
      !bounded_argb8(destination_world, destination, destination_rowbytes,
                           destination_width, destination_height)) {
    return kPfErrBadCallbackParam;
  }
  if (source_rect->right < source_rect->left || source_rect->bottom < source_rect->top) {
    return kPfErrBadCallbackParam;
  }

  // Map the requested source rectangle's upper-left to the destination, then clip in 64-bit.
  const int64_t source_left = std::max<int64_t>(source_rect->left, 0);
  const int64_t source_top = std::max<int64_t>(source_rect->top, 0);
  const int64_t source_right = std::min<int64_t>(source_rect->right, source_width);
  const int64_t source_bottom = std::min<int64_t>(source_rect->bottom, source_height);
  const int64_t destination_left = static_cast<int64_t>(destination_x) +
      source_left - source_rect->left;
  const int64_t destination_top = static_cast<int64_t>(destination_y) +
      source_top - source_rect->top;
  const int64_t clipped_destination_left = std::max<int64_t>(destination_left, 0);
  const int64_t clipped_destination_top = std::max<int64_t>(destination_top, 0);
  const int64_t clipped_destination_right = std::min<int64_t>(
      destination_left + (source_right - source_left), destination_width);
  const int64_t clipped_destination_bottom = std::min<int64_t>(
      destination_top + (source_bottom - source_top), destination_height);
  if (source_right <= source_left || source_bottom <= source_top ||
      clipped_destination_right <= clipped_destination_left ||
      clipped_destination_bottom <= clipped_destination_top || source_opacity == 0) {
    return 0;
  }

  const int64_t clipped_source_left = source_left + clipped_destination_left - destination_left;
  const int64_t clipped_source_top = source_top + clipped_destination_top - destination_top;
  const std::size_t width = static_cast<std::size_t>(
      clipped_destination_right - clipped_destination_left);
  const std::size_t height = static_cast<std::size_t>(
      clipped_destination_bottom - clipped_destination_top);
  if (width > 4096 || height > 4096 || width > SIZE_MAX / 4 ||
      height > SIZE_MAX / (width * 4)) {
    return kPfErrBadCallbackParam;
  }

  std::vector<unsigned char> snapshot;
  try {
    snapshot.resize(width * height * 4);
  } catch (const std::bad_alloc&) {
    return 4;
  }
  for (std::size_t row = 0; row < height; ++row) {
    std::memcpy(snapshot.data() + row * width * 4,
                source + static_cast<std::size_t>(clipped_source_top + row) * source_rowbytes +
                    static_cast<std::size_t>(clipped_source_left) * 4,
                width * 4);
  }

  const uint32_t opacity = static_cast<uint32_t>(source_opacity);
  for (std::size_t row = 0; row < height; ++row) {
    const int64_t output_y = clipped_destination_top + static_cast<int64_t>(row);
    if ((field == kFieldUpper && (output_y & 1) != 0) ||
        (field == kFieldLower && (output_y & 1) == 0)) {
      continue;
    }
    for (std::size_t column = 0; column < width; ++column) {
      const auto* input = snapshot.data() + (row * width + column) * 4;
      auto* output = destination + static_cast<std::size_t>(output_y) * destination_rowbytes +
          static_cast<std::size_t>(clipped_destination_left + column) * 4;
      if (transfer_mode == kTransferCopy) {
        for (int channel = 0; channel < 4; ++channel) {
          output[channel] = composite_divide_255(
              static_cast<uint32_t>(input[channel]) * opacity +
              static_cast<uint32_t>(output[channel]) * (255 - opacity));
        }
      } else if (transfer_mode == kTransferInFront) {
        const uint32_t destination_weight = 65'025 - input[0] * opacity;
        for (int channel = 0; channel < 4; ++channel) {
          output[channel] = composite_divide_65025(
              static_cast<uint64_t>(input[channel]) * opacity * 255 +
              static_cast<uint64_t>(output[channel]) * destination_weight);
        }
      } else {
        const uint32_t source_weight = opacity * (255 - output[0]);
        for (int channel = 0; channel < 4; ++channel) {
          output[channel] = composite_divide_65025(
              static_cast<uint64_t>(output[channel]) * 65'025 +
              static_cast<uint64_t>(input[channel]) * source_weight);
        }
      }
    }
  }
  return 0;
}

template <typename Channel, uint32_t Maximum>
int32_t composite_rect_registered(void* effect_ref, LegacyRect* source_rect,
                                  int32_t source_opacity, void* source_world,
                                  int32_t destination_x, int32_t destination_y,
                                  int32_t field, int32_t transfer_mode,
                                  void* destination_world) {
  if (!effect_ref || !source_rect || source_opacity < 0 || source_opacity > 255 ||
      field < 0 || field > 2 || transfer_mode < 0 || transfer_mode > 2 ||
      source_rect->right < source_rect->left || source_rect->bottom < source_rect->top)
    return kPfErrBadCallbackParam;
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format ||
      source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * sizeof(Channel) * 4 ||
      destination_info.rowbytes <
          static_cast<int64_t>(destination_info.width) * sizeof(Channel) * 4)
    return kPfErrBadCallbackParam;

  const int64_t sl = std::max<int64_t>(source_rect->left, 0);
  const int64_t st = std::max<int64_t>(source_rect->top, 0);
  const int64_t sr = std::min<int64_t>(source_rect->right, source_info.width);
  const int64_t sb = std::min<int64_t>(source_rect->bottom, source_info.height);
  const int64_t dl = static_cast<int64_t>(destination_x) + sl - source_rect->left;
  const int64_t dt = static_cast<int64_t>(destination_y) + st - source_rect->top;
  const int64_t cdl = std::max<int64_t>(dl, 0);
  const int64_t cdt = std::max<int64_t>(dt, 0);
  const int64_t cdr = std::min<int64_t>(dl + sr - sl, destination_info.width);
  const int64_t cdb = std::min<int64_t>(dt + sb - st, destination_info.height);
  if (sr <= sl || sb <= st || cdr <= cdl || cdb <= cdt || source_opacity == 0) return 0;
  const int64_t csl = sl + cdl - dl;
  const int64_t cst = st + cdt - dt;
  const std::size_t width = static_cast<std::size_t>(cdr - cdl);
  const std::size_t height = static_cast<std::size_t>(cdb - cdt);
  if (width > SIZE_MAX / height || width * height > 16'777'216) return kPfErrBadCallbackParam;

  using Pixel = std::array<Channel, 4>;
  std::vector<Pixel> snapshot;
  try { snapshot.resize(width * height); } catch (const std::bad_alloc&) { return 4; }
  const auto* source = static_cast<const unsigned char*>(source_info.data);
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  for (std::size_t row = 0; row < height; ++row)
    std::memcpy(snapshot.data() + row * width,
                source + static_cast<std::size_t>(cst + row) * source_info.rowbytes +
                    static_cast<std::size_t>(csl) * sizeof(Pixel),
                width * sizeof(Pixel));

  const uint64_t opacity = static_cast<uint64_t>(source_opacity);
  const uint64_t denominator = static_cast<uint64_t>(Maximum) * 255;
  auto rounded = [](uint64_t numerator, uint64_t divisor) -> Channel {
    return static_cast<Channel>(std::min<uint64_t>(Maximum, (numerator + divisor / 2) / divisor));
  };
  for (std::size_t row = 0; row < height; ++row) {
    const int64_t output_y = cdt + static_cast<int64_t>(row);
    if ((field == 1 && (output_y & 1)) || (field == 2 && !(output_y & 1))) continue;
    for (std::size_t column = 0; column < width; ++column) {
      const Pixel& input = snapshot[row * width + column];
      auto* output = reinterpret_cast<Pixel*>(destination +
          static_cast<std::size_t>(output_y) * destination_info.rowbytes +
          static_cast<std::size_t>(cdl + column) * sizeof(Pixel));
      const uint64_t destination_alpha = (*output)[0];
      for (std::size_t channel = 0; channel < 4; ++channel) {
        uint64_t numerator{};
        uint64_t divisor{};
        if (transfer_mode == 0) {
          numerator = static_cast<uint64_t>(input[channel]) * opacity +
              static_cast<uint64_t>((*output)[channel]) * (255 - opacity);
          divisor = 255;
        } else if (transfer_mode == 2) {
          const uint64_t destination_weight = denominator -
              static_cast<uint64_t>(input[0]) * opacity;
          numerator = static_cast<uint64_t>(input[channel]) * opacity * Maximum +
              static_cast<uint64_t>((*output)[channel]) * destination_weight;
          divisor = denominator;
        } else {
          const uint64_t source_weight = opacity * (Maximum - destination_alpha);
          numerator = static_cast<uint64_t>((*output)[channel]) * denominator +
              static_cast<uint64_t>(input[channel]) * source_weight;
          divisor = denominator;
        }
        (*output)[channel] = rounded(numerator, divisor);
      }
    }
  }
  return 0;
}
int32_t composite_rect_float(void* effect_ref, LegacyRect* source_rect,
                             int32_t source_opacity, void* source_world,
                             int32_t destination_x, int32_t destination_y,
                             int32_t field, int32_t transfer_mode,
                             void* destination_world) {
  if (!effect_ref || !source_rect || source_opacity < 0 || source_opacity > 255 ||
      field < 0 || field > 2 || transfer_mode < 0 || transfer_mode > 2)
    return kPfErrBadCallbackParam;
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != kPixelFormatArgb128 ||
      destination_info.pixel_format != kPixelFormatArgb128 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * 16 ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * 16)
    return kPfErrBadCallbackParam;
  const int64_t sl=(std::max<int64_t>)(source_rect->left,0), st=(std::max<int64_t>)(source_rect->top,0),
      sr=(std::min<int64_t>)(source_rect->right,source_info.width), sb=(std::min<int64_t>)(source_rect->bottom,source_info.height);
  const int64_t dl=destination_x+sl-source_rect->left, dt=destination_y+st-source_rect->top;
  const int64_t cdl=(std::max<int64_t>)(dl,0), cdt=(std::max<int64_t>)(dt,0),
      cdr=(std::min<int64_t>)(dl+sr-sl,destination_info.width), cdb=(std::min<int64_t>)(dt+sb-st,destination_info.height);
  if(sr<=sl||sb<=st||cdr<=cdl||cdb<=cdt||source_opacity==0) return 0;
  const int64_t csl=sl+cdl-dl,cst=st+cdt-dt;
  const std::size_t width=static_cast<std::size_t>(cdr-cdl),height=static_cast<std::size_t>(cdb-cdt);
  if(width>SIZE_MAX/height||width*height>16'777'216) return kPfErrBadCallbackParam;
  using Pixel=std::array<float,4>; std::vector<Pixel> snapshot;
  try{snapshot.resize(width*height);}catch(const std::bad_alloc&){return 4;}
  const auto* source=static_cast<const unsigned char*>(source_info.data); auto* destination=static_cast<unsigned char*>(destination_info.data);
  for(std::size_t row=0;row<height;++row) std::memcpy(snapshot.data()+row*width,
      source+static_cast<std::size_t>(cst+row)*source_info.rowbytes+static_cast<std::size_t>(csl)*sizeof(Pixel),width*sizeof(Pixel));
  const double opacity=source_opacity/255.0;
  for(std::size_t row=0;row<height;++row){const int64_t output_y=cdt+static_cast<int64_t>(row);
    if((field==1&&(output_y&1))||(field==2&&!(output_y&1)))continue;
    for(std::size_t column=0;column<width;++column){const Pixel& input=snapshot[row*width+column];
      auto* output=reinterpret_cast<Pixel*>(destination+static_cast<std::size_t>(output_y)*destination_info.rowbytes+static_cast<std::size_t>(cdl+column)*sizeof(Pixel));
      const double destination_alpha=(*output)[0];
      for(int channel=0;channel<4;++channel){
        if(transfer_mode==0)(*output)[channel]=static_cast<float>(input[channel]*opacity+(*output)[channel]*(1.0-opacity));
        else if(transfer_mode==2)(*output)[channel]=static_cast<float>(input[channel]*opacity+(*output)[channel]*(1.0-input[0]*opacity));
        else (*output)[channel]=static_cast<float>((*output)[channel]+input[channel]*opacity*(1.0-destination_alpha));
      }
    }
  }
  return 0;
}

int32_t __cdecl composite_rect8(void* effect_ref, LegacyRect* source_rect,
                                int32_t source_opacity, void* source_world,
                                int32_t destination_x, int32_t destination_y,
                                int32_t field, int32_t transfer_mode,
                                void* destination_world) {
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format)
    return kPfErrBadCallbackParam;
  if (source_info.pixel_format == kPixelFormatArgb32)
    return composite_rect_registered<uint8_t, 255>(effect_ref, source_rect, source_opacity,
        source_world, destination_x, destination_y, field, transfer_mode, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb64)
    return composite_rect_registered<uint16_t, 32768>(effect_ref, source_rect, source_opacity,
        source_world, destination_x, destination_y, field, transfer_mode, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb128)
    return composite_rect_float(effect_ref, source_rect, source_opacity, source_world,
        destination_x, destination_y, field, transfer_mode, destination_world);
  return kPfErrBadCallbackParam;
}

bool verify_world_transform_composite_rect() {
  DispatchWorldFormatScope dispatch_worlds;
  auto make_world = [&](std::array<std::byte, 64>& world, unsigned char* pixels,
                       int32_t rowbytes, int32_t width, int32_t height) {
    world.fill(std::byte{});
    std::memcpy(world.data() + 24, &pixels, sizeof(pixels));
    std::memcpy(world.data() + 32, &rowbytes, sizeof(rowbytes));
    std::memcpy(world.data() + 36, &width, sizeof(width));
    std::memcpy(world.data() + 40, &height, sizeof(height));
    dispatch_worlds.register_world(world.data(), kPixelFormatArgb32);
  };
  auto run_pixel_case = [&](int32_t mode, const std::array<uint8_t, 4>& expected) {
    std::array<unsigned char, 4> source{128, 64, 32, 16};
    std::array<unsigned char, 4> destination{64, 20, 10, 5};
    std::array<std::byte, 64> source_world{}, destination_world{};
    make_world(source_world, source.data(), 4, 1, 1);
    make_world(destination_world, destination.data(), 4, 1, 1);
    LegacyRect rect{0, 0, 1, 1};
    return composite_rect8(&source_world, &rect, 128, &source_world, 0, 0, 0, mode,
                           &destination_world) == 0 && destination == expected;
  };
  if (!run_pixel_case(0, {96, 42, 21, 11}) ||
      !run_pixel_case(1, {112, 44, 22, 11}) ||
      !run_pixel_case(2, {112, 47, 24, 12})) {
    std::cerr << "composite diagnostic: pixel matrix\n";
    return false;
  }

  constexpr int32_t rowbytes = 16;
  std::array<unsigned char, rowbytes * 3 + 16> source_guarded{};
  std::array<unsigned char, rowbytes * 3 + 16> destination_guarded{};
  source_guarded.fill(0xA5);
  destination_guarded.fill(0xCC);
  auto* source = source_guarded.data() + 8;
  auto* destination = destination_guarded.data() + 8;
  for (int y = 0; y < 3; ++y) {
    for (int x = 0; x < 3; ++x) {
      const std::array<unsigned char, 4> pixel{
          255, static_cast<unsigned char>(10 + y * 3 + x), 0, 0};
      std::memcpy(source + y * rowbytes + x * 4, pixel.data(), 4);
    }
  }
  std::array<std::byte, 64> source_world{}, destination_world{};
  make_world(source_world, source, rowbytes, 3, 3);
  make_world(destination_world, destination, rowbytes, 3, 3);
  LegacyRect rect{0, 0, 3, 3};
  if (composite_rect8(&source_world, &rect, 255, &source_world, -1, 0, 1, 0,
                      &destination_world) != 0) {
    std::cerr << "composite diagnostic: clipped upper field call\n";
    return false;
  }
  // Clipping drops source column zero; upper field updates destination rows 0 and 2 only.
  if (destination[1] != 11 || destination[5] != 12 || destination[rowbytes] != 0xCC ||
      destination[2 * rowbytes + 1] != 17 || destination[2 * rowbytes + 5] != 18) {
    std::cerr << "composite diagnostic: clipped upper field values\n";
    return false;
  }
  for (int y = 0; y < 3; ++y) {
    for (int x = 12; x < rowbytes; ++x) {
      if (destination[y * rowbytes + x] != 0xCC) return false;
    }
  }
  std::memset(destination, 0xCC, rowbytes * 3);
  if (composite_rect8(&source_world, &rect, 255, &source_world, 0, 0, 2, 0,
                      &destination_world) != 0 || destination[0] != 0xCC ||
      destination[rowbytes] != 255 || destination[rowbytes + 1] != 13 ||
      destination[2 * rowbytes] != 0xCC) {
    std::cerr << "composite diagnostic: lower field\n";
    return false;
  }
  if (!std::all_of(source_guarded.begin(), source_guarded.begin() + 8,
                   [](unsigned char value) { return value == 0xA5; }) ||
      !std::all_of(source_guarded.end() - 8, source_guarded.end(),
                   [](unsigned char value) { return value == 0xA5; }) ||
      !std::all_of(destination_guarded.begin(), destination_guarded.begin() + 8,
                   [](unsigned char value) { return value == 0xCC; }) ||
      !std::all_of(destination_guarded.end() - 8, destination_guarded.end(),
                   [](unsigned char value) { return value == 0xCC; })) {
    std::cerr << "composite diagnostic: guards\n";
    return false;
  }

  std::array<unsigned char, 12> alias_pixels{255, 1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0};
  std::array<std::byte, 64> alias_world{};
  make_world(alias_world, alias_pixels.data(), 12, 3, 1);
  LegacyRect alias_rect{0, 0, 2, 1};
  if (composite_rect8(&alias_world, &alias_rect, 255, &alias_world, 1, 0, 0, 0,
                      &alias_world) != 0 || alias_pixels[5] != 1 || alias_pixels[9] != 2) {
    std::cerr << "composite diagnostic: alias\n";
    return false;
  }
  if (composite_rect8(nullptr, &alias_rect, 255, &alias_world, 0, 0, 0, 0,
                      &alias_world) != kPfErrBadCallbackParam ||
      composite_rect8(&alias_world, nullptr, 255, &alias_world, 0, 0, 0, 0,
                      &alias_world) != kPfErrBadCallbackParam ||
      composite_rect8(&alias_world, &alias_rect, 256, &alias_world, 0, 0, 0, 0,
                      &alias_world) != kPfErrBadCallbackParam ||
      composite_rect8(&alias_world, &alias_rect, 255, &alias_world, 0, 0, 3, 0,
                      &alias_world) != kPfErrBadCallbackParam) {
    std::cerr << "composite diagnostic: invalid arguments\n";
    return false;
  }

  // A padded ARGB16 row can resemble 32F by width; provenance must win over layout.
  std::array<uint16_t, 12> source16{32768, 0, 32768, 1, 32768, 32768, 0, 32767};
  std::array<uint16_t, 12> destination16{};
  std::array<std::byte, 64> registered16{}, shallow16{}, output16{};
  auto make16 = [](auto& world, void* data) {
    world.fill(std::byte{});
    const int32_t flags = 1, rowbytes = 24, width = 2, height = 1;
    std::memcpy(world.data() + 16, &flags, sizeof(flags));
    std::memcpy(world.data() + 24, &data, sizeof(data));
    std::memcpy(world.data() + 32, &rowbytes, sizeof(rowbytes));
    std::memcpy(world.data() + 36, &width, sizeof(width));
    std::memcpy(world.data() + 40, &height, sizeof(height));
  };
  make16(registered16, source16.data());
  shallow16 = registered16;
  make16(output16, destination16.data());
  if (!dispatch_worlds.register_world(registered16.data(), kPixelFormatArgb64) ||
      !dispatch_worlds.register_world(output16.data(), kPixelFormatArgb64)) {
    std::cerr << "composite diagnostic: register 16\n";
    return false;
  }
  LegacyRect rect16{0, 0, 2, 1};
  if (composite_rect8(registered16.data(), &rect16, 255, shallow16.data(), 0, 0, 0, 0,
      output16.data()) != 0 ||
      !std::equal(source16.begin(), source16.begin() + 8, destination16.begin())) {
    std::cerr << "composite diagnostic: copy 16\n";
    return false;
  }

  std::atomic_bool thread16{false}, thread32{false};
  std::thread deep_thread([&] {
    DispatchWorldFormatScope scope;
    scope.register_world(registered16.data(), kPixelFormatArgb64);
    scope.register_world(output16.data(), kPixelFormatArgb64);
    thread16 = composite_rect8(registered16.data(), &rect16, 255, registered16.data(),
                               0, 0, 0, 0, output16.data()) == 0;
  });
  std::thread float_thread([&] {
    DispatchWorldFormatScope scope;
    scope.register_world(registered16.data(), kPixelFormatArgb128);
    scope.register_world(output16.data(), kPixelFormatArgb128);
    thread32 = composite_rect8(registered16.data(), &rect16, 255, registered16.data(),
                               0, 0, 0, 0, output16.data()) == kPfErrBadCallbackParam;
  });
  deep_thread.join();
  float_thread.join();
  if (!thread16 || !thread32) {
    std::cerr << "composite diagnostic: concurrency " << thread16 << ',' << thread32 << '\n';
    return false;
  }
  std::array<float, 8> source32{{0.5f,2.0f,-0.5f,4.0f, 1.0f,8.0f,0.25f,-2.0f}};
  std::array<float, 8> destination32{};
  LocalEffectWorld source_world32{}, destination_world32{};
  source_world32.data=source32.data(); source_world32.rowbytes=32;
  source_world32.width=2; source_world32.height=1;
  destination_world32.data=destination32.data(); destination_world32.rowbytes=32;
  destination_world32.width=2; destination_world32.height=1;
  const bool source32_registered =
      dispatch_worlds.register_world(&source_world32, kPixelFormatArgb128);
  const bool destination32_registered =
      dispatch_worlds.register_world(&destination_world32, kPixelFormatArgb128);
  const int32_t composite32_result = composite_rect8(
      &source_world32, &rect16, 255, &source_world32, 0, 0, 0, 0, &destination_world32);
  if (!source32_registered || !destination32_registered || composite32_result != 0 ||
      destination32 != source32) {
    std::cerr << "float composite diagnostic: source_registered=" << source32_registered
              << ", destination_registered=" << destination32_registered
              << ", result=" << composite32_result;
    for (float value : destination32) std::cerr << ',' << value;
    std::cerr << '\n';
    return false;
  }
  return true;
}


namespace {
struct WorldTransformSuite1 {
  decltype(&composite_rect8) composite_rect;
  decltype(&blend_world) blend;
  decltype(&convolve_world) convolve;
  decltype(&copy_world8) copy;
  decltype(&copy_world_hq) copy_hq;
  decltype(&transfer_rect) transfer_rect;
  decltype(&transform_world) transform_world;
};
static_assert(sizeof(WorldTransformSuite1) == 7 * sizeof(void*));
static_assert(offsetof(WorldTransformSuite1, composite_rect) == 0 * sizeof(void*));
static_assert(offsetof(WorldTransformSuite1, transform_world) == 6 * sizeof(void*));
WorldTransformSuite1 g_world_transform_suite1{};
std::array<void*, 7> g_fill_matte_suite2{};
}  // namespace

const void* provide_world_transform1(void*) {
  g_world_transform_suite1 = {&composite_rect8, &blend_world, &convolve_world,
      &copy_world8, &copy_world_hq, &transfer_rect, &transform_world};
  return &g_world_transform_suite1;
}

const void* provide_fill_matte2(void*) {
  void* callbacks[] = {reinterpret_cast<void*>(&fill_world8),
      reinterpret_cast<void*>(&fill_world16), reinterpret_cast<void*>(&fill_world_float),
      reinterpret_cast<void*>(&premultiply_world8), reinterpret_cast<void*>(&premultiply_color8),
      reinterpret_cast<void*>(&premultiply_color16),
      reinterpret_cast<void*>(&premultiply_color_float)};
  std::copy(std::begin(callbacks), std::end(callbacks), g_fill_matte_suite2.begin());
  return g_fill_matte_suite2.data();
}

}  // namespace aexcompat::pf_world_transform
