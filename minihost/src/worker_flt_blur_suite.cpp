#include "worker_flt_blur_suite.hpp"

#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <new>
#include <vector>

namespace aexcompat::flt_blur {
namespace {

constexpr int32_t kBadCallbackParam = 4;
constexpr int32_t kAllChannels = 0x0f;
constexpr int32_t kKnownFlags =
    kAllChannels | kRepeatEdgePixels | kVertical | kHorizontal;
constexpr int32_t kMaximumIterations = 128;
constexpr float kMaximumRadius = 4096.0f;

Hooks g_hooks{};

int32_t bytes_per_pixel(int32_t pixel_format) noexcept {
  switch (pixel_format) {
    case world_registry::kPixelFormatArgb32: return 4;
    case world_registry::kPixelFormatArgb64: return 8;
    case world_registry::kPixelFormatArgb128: return 16;
    default: return 0;
  }
}

bool valid_flags(int32_t flags) noexcept {
  return (flags & ~kKnownFlags) == 0 && (flags & kAllChannels) != 0 &&
      (flags & (kHorizontal | kVertical)) != 0;
}

bool valid_radius(float value) noexcept {
  return std::isfinite(value) && value >= 0.0f && value <= kMaximumRadius;
}

bool valid_progress(int32_t base, int32_t final) noexcept {
  return base >= 0 && final >= base;
}

bool compatible_worlds(const world_safety::DispatchWorldFormat& source,
                       const world_safety::DispatchWorldFormat& destination,
                       int32_t& pixel_bytes) noexcept {
  pixel_bytes = bytes_per_pixel(source.pixel_format);
  if (!pixel_bytes || source.pixel_format != destination.pixel_format ||
      !source.data || !destination.data || source.width <= 0 ||
      source.height <= 0 || source.width != destination.width ||
      source.height != destination.height)
    return false;
  const int64_t tight = static_cast<int64_t>(source.width) * pixel_bytes;
  const int64_t source_bytes =
      static_cast<int64_t>(source.rowbytes) * source.height;
  const int64_t destination_bytes =
      static_cast<int64_t>(destination.rowbytes) * destination.height;
  return tight > 0 && source.rowbytes >= tight && destination.rowbytes >= tight &&
      source_bytes > 0 && destination_bytes > 0 &&
      source_bytes <= 256LL * 1024 * 1024 &&
      destination_bytes <= 256LL * 1024 * 1024;
}

template <class Channel>
float load_channel(const std::byte* pixel, int32_t channel) noexcept {
  Channel value{};
  std::memcpy(&value, pixel + channel * sizeof(Channel), sizeof(value));
  return static_cast<float>(value);
}

template <>
float load_channel<float>(const std::byte* pixel, int32_t channel) noexcept {
  float value{};
  std::memcpy(&value, pixel + channel * sizeof(float), sizeof(value));
  return value;
}

template <class Channel>
void store_channel(std::byte* pixel, int32_t channel, float value) noexcept {
  const float maximum = static_cast<float>((std::numeric_limits<Channel>::max)());
  const Channel output = static_cast<Channel>(
      std::lround(std::clamp(value, 0.0f, maximum)));
  std::memcpy(pixel + channel * sizeof(Channel), &output, sizeof(output));
}

template <>
void store_channel<float>(std::byte* pixel, int32_t channel,
                          float value) noexcept {
  std::memcpy(pixel + channel * sizeof(float), &value, sizeof(value));
}

template <class Channel>
void load_pixels(const world_safety::DispatchWorldFormat& world,
                 std::vector<float>& pixels) {
  const auto* base = static_cast<const std::byte*>(world.data);
  for (int32_t y = 0; y < world.height; ++y) {
    const auto* row = base + static_cast<std::size_t>(y) * world.rowbytes;
    for (int32_t x = 0; x < world.width; ++x) {
      const auto* pixel = row + static_cast<std::size_t>(x) * 4 * sizeof(Channel);
      const std::size_t index =
          (static_cast<std::size_t>(y) * world.width + x) * 4;
      for (int32_t channel = 0; channel < 4; ++channel)
        pixels[index + channel] = load_channel<Channel>(pixel, channel);
    }
  }
}

template <class Channel>
void store_pixels(const std::vector<float>& pixels,
                  const world_safety::DispatchWorldFormat& world) noexcept {
  auto* base = static_cast<std::byte*>(world.data);
  for (int32_t y = 0; y < world.height; ++y) {
    auto* row = base + static_cast<std::size_t>(y) * world.rowbytes;
    for (int32_t x = 0; x < world.width; ++x) {
      auto* pixel = row + static_cast<std::size_t>(x) * 4 * sizeof(Channel);
      const std::size_t index =
          (static_cast<std::size_t>(y) * world.width + x) * 4;
      for (int32_t channel = 0; channel < 4; ++channel)
        store_channel<Channel>(pixel, channel, pixels[index + channel]);
    }
  }
}

std::vector<float> gaussian_weights(float radius) {
  const int32_t extent = static_cast<int32_t>(std::ceil(radius));
  if (extent <= 0) return {1.0f};
  const float sigma = std::max(radius / 3.0f, 0.5f);
  const float denominator = 2.0f * sigma * sigma;
  std::vector<float> weights(static_cast<std::size_t>(extent) * 2 + 1);
  float sum{};
  for (int32_t offset = -extent; offset <= extent; ++offset) {
    const float weight = std::exp(-static_cast<float>(offset * offset) /
                                  denominator);
    weights[static_cast<std::size_t>(offset + extent)] = weight;
    sum += weight;
  }
  for (float& weight : weights) weight /= sum;
  return weights;
}

std::vector<float> box_weights(float radius) {
  const int32_t extent = static_cast<int32_t>(std::ceil(radius));
  if (extent <= 0) return {1.0f};
  const std::size_t count = static_cast<std::size_t>(extent) * 2 + 1;
  return std::vector<float>(count, 1.0f / static_cast<float>(count));
}

void convolve_axis(const std::vector<float>& input, std::vector<float>& output,
                   int32_t width, int32_t height,
                   const std::vector<float>& weights, bool horizontal,
                   bool repeat_edge, int32_t channel_flags) noexcept {
  const int32_t extent = static_cast<int32_t>(weights.size() / 2);
  output = input;
  for (int32_t y = 0; y < height; ++y) {
    for (int32_t x = 0; x < width; ++x) {
      const std::size_t destination =
          (static_cast<std::size_t>(y) * width + x) * 4;
      for (int32_t channel = 0; channel < 4; ++channel) {
        if ((channel_flags & (1 << channel)) == 0) continue;
        float value{};
        for (int32_t offset = -extent; offset <= extent; ++offset) {
          int32_t sample_x = horizontal ? x + offset : x;
          int32_t sample_y = horizontal ? y : y + offset;
          if (repeat_edge) {
            sample_x = std::clamp(sample_x, 0, width - 1);
            sample_y = std::clamp(sample_y, 0, height - 1);
          } else if (sample_x < 0 || sample_x >= width || sample_y < 0 ||
                     sample_y >= height) {
            continue;
          }
          const std::size_t source =
              (static_cast<std::size_t>(sample_y) * width + sample_x) * 4;
          value += input[source + channel] *
              weights[static_cast<std::size_t>(offset + extent)];
        }
        output[destination + channel] = value;
      }
    }
  }
}

int32_t blur_worlds(const world_safety::DispatchWorldFormat& source,
                    const world_safety::DispatchWorldFormat& destination,
                    float radius_x, float radius_y, int32_t flags,
                    bool gaussian, int32_t iterations) {
  int32_t pixel_bytes{};
  if (!compatible_worlds(source, destination, pixel_bytes) ||
      !valid_radius(radius_x) || !valid_radius(radius_y) ||
      !valid_flags(flags) || iterations <= 0 ||
      iterations > kMaximumIterations)
    return kBadCallbackParam;
  try {
    const std::size_t values = static_cast<std::size_t>(source.width) *
        source.height * 4;
    std::vector<float> current(values);
    std::vector<float> scratch;
    if (pixel_bytes == 4) load_pixels<uint8_t>(source, current);
    else if (pixel_bytes == 8) load_pixels<uint16_t>(source, current);
    else load_pixels<float>(source, current);

    const auto horizontal_weights = gaussian ? gaussian_weights(radius_x)
                                              : box_weights(radius_x);
    const auto vertical_weights = gaussian ? gaussian_weights(radius_y)
                                            : box_weights(radius_y);
    const bool repeat_edge = (flags & kRepeatEdgePixels) != 0;
    const int32_t channels = flags & kAllChannels;
    for (int32_t iteration = 0; iteration < iterations; ++iteration) {
      if ((flags & kHorizontal) != 0 && radius_x > 0.0f) {
        convolve_axis(current, scratch, source.width, source.height,
                      horizontal_weights, true, repeat_edge, channels);
        current.swap(scratch);
      }
      if ((flags & kVertical) != 0 && radius_y > 0.0f) {
        convolve_axis(current, scratch, source.width, source.height,
                      vertical_weights, false, repeat_edge, channels);
        current.swap(scratch);
      }
    }
    if (pixel_bytes == 4) store_pixels<uint8_t>(current, destination);
    else if (pixel_bytes == 8) store_pixels<uint16_t>(current, destination);
    else store_pixels<float>(current, destination);
    return 0;
  } catch (const std::bad_alloc&) {
    return kBadCallbackParam;
  }
}

int32_t __cdecl gaussian_blur(
    void* effect_ref, const void* source_world, float radius_x, float radius_y,
    int32_t flags, int32_t quality, int32_t progress_base,
    int32_t progress_final, void* destination_world) {
  if (!g_hooks.effect_ref || effect_ref != g_hooks.effect_ref ||
      !g_hooks.resolve_world || (quality != 0 && quality != 1) ||
      !valid_progress(progress_base, progress_final))
    return kBadCallbackParam;
  world_safety::DispatchWorldFormat source{}, destination{};
  if (!g_hooks.resolve_world(source_world, source) ||
      !g_hooks.resolve_world(destination_world, destination))
    return kBadCallbackParam;
  return blur_worlds(source, destination, radius_x, radius_y, flags, true, 1);
}

int32_t __cdecl box_blur(
    void* effect_ref, const void* source_world, float radius_x, float radius_y,
    int32_t iterations, int32_t flags, int32_t progress_base,
    int32_t progress_final, void* destination_world) {
  if (!g_hooks.effect_ref || effect_ref != g_hooks.effect_ref ||
      !g_hooks.resolve_world || !valid_progress(progress_base, progress_final))
    return kBadCallbackParam;
  world_safety::DispatchWorldFormat source{}, destination{};
  if (!g_hooks.resolve_world(source_world, source) ||
      !g_hooks.resolve_world(destination_world, destination))
    return kBadCallbackParam;
  return blur_worlds(source, destination, radius_x, radius_y, flags, false,
                     iterations);
}

// AE's FLT_ComputeDirectionalBlurRadii, byte-for-byte from FLT.dll (0x1800324c0):
//   rad = angle_degrees * (pi/180)
//   *radius_x = (int)ceil(|sin(rad)| * x_scale * magnitude)
//   *radius_y = (int)ceil(|cos(rad)| * y_scale * magnitude)
// AE rounds toward +inf (vroundsd imm 2) then truncates to int, and returns 0
// unconditionally. The host matches the math exactly for finite inputs and adds
// two things AE lacks: a null-pointer guard (fail-closed, like the other slots)
// and a bounded cast so a non-finite product cannot make the int conversion
// undefined. Directional Blur is the only observed caller (issue #1093).
constexpr double kDegreesToRadians =
    3.14159265358979311599796346854418516159057617187500 / 180.0;

int32_t bounded_ceil_to_int(double value) noexcept {
  if (!std::isfinite(value)) return 0;
  const double ceiled = std::ceil(value);
  if (ceiled <= static_cast<double>(std::numeric_limits<int32_t>::min()))
    return std::numeric_limits<int32_t>::min();
  if (ceiled >= static_cast<double>(std::numeric_limits<int32_t>::max()))
    return std::numeric_limits<int32_t>::max();
  return static_cast<int32_t>(ceiled);
}

int32_t __cdecl compute_directional_blur_radii(
    double x_scale, double y_scale, double angle_degrees, double magnitude,
    int32_t* radius_x, int32_t* radius_y) {
  if (!radius_x || !radius_y) return kBadCallbackParam;
  const double radians = angle_degrees * kDegreesToRadians;
  *radius_x = bounded_ceil_to_int(std::fabs(std::sin(radians)) * x_scale * magnitude);
  *radius_y = bounded_ceil_to_int(std::fabs(std::cos(radians)) * y_scale * magnitude);
  return 0;
}

Suite1 g_suite1{&gaussian_blur, &box_blur, &compute_directional_blur_radii};

}  // namespace

bool configure(const Hooks& hooks) noexcept {
  if (!hooks.effect_ref || !hooks.resolve_world || !hooks.acquire_suite ||
      !hooks.release_suite)
    return false;
  g_hooks = hooks;
  return true;
}

const Suite1* suite1() noexcept { return &g_suite1; }

bool selftest() {
  constexpr int32_t width = 3;
  constexpr int32_t height = 1;
  const void* raw_suite{};
  const void* rejected_suite = reinterpret_cast<const void*>(1);
  if (g_hooks.acquire_suite(kSuiteName, kSuiteVersion1, &raw_suite) != 0 ||
      !raw_suite ||
      g_hooks.acquire_suite(kSuiteName, kSuiteVersion1 + 1,
                            &rejected_suite) == 0 ||
      rejected_suite != nullptr)
    return false;
  const auto* suite = static_cast<const Suite1*>(raw_suite);
  if (!suite->gaussian_blur || !suite->box_blur ||
      !suite->compute_directional_blur_radii) {
    g_hooks.release_suite(kSuiteName, kSuiteVersion1);
    return false;
  }

  const auto make_world = [=](void* pixels, int32_t rowbytes) {
    world_safety::LocalEffectWorld world{};
    world.data = pixels;
    world.rowbytes = rowbytes;
    world.width = width;
    world.height = height;
    world.extent_hint = {0, 0, width, height};
    world.pix_aspect_ratio = {1, 1};
    return world;
  };

  std::array<uint8_t, width * height * 4> source{
      255, 0, 0, 0, 255, 255, 60, 30, 255, 0, 0, 0};
  std::array<uint8_t, width * height * 4> destination{};
  const auto source_before = source;
  auto source_world = make_world(source.data(), width * 4);
  auto destination_world = make_world(destination.data(), width * 4);

  std::array<uint16_t, width * height * 4> source16{
      32768, 0, 0, 0, 32768, 32768, 1000, 500, 32768, 0, 0, 0};
  std::array<uint16_t, width * height * 4> destination16{};
  const auto source16_before = source16;
  auto source_world16 = make_world(source16.data(), width * 8);
  auto destination_world16 = make_world(destination16.data(), width * 8);

  std::array<float, width * height * 4> source_float{
      1.0f, 0.0f, 0.0f, 0.0f, 1.0f, 1.0f, 0.25f, 0.125f,
      1.0f, 0.0f, 0.0f, 0.0f};
  std::array<float, width * height * 4> destination_float{};
  const auto source_float_before = source_float;
  auto source_world_float = make_world(source_float.data(), width * 16);
  auto destination_world_float =
      make_world(destination_float.data(), width * 16);

  world_safety::DispatchWorldFormatScope formats;
  bool ok = formats.register_world(&source_world,
                                   world_registry::kPixelFormatArgb32) &&
      formats.register_world(&destination_world,
                             world_registry::kPixelFormatArgb32) &&
      formats.register_world(&source_world16,
                             world_registry::kPixelFormatArgb64) &&
      formats.register_world(&destination_world16,
                             world_registry::kPixelFormatArgb64) &&
      formats.register_world(&source_world_float,
                             world_registry::kPixelFormatArgb128) &&
      formats.register_world(&destination_world_float,
                             world_registry::kPixelFormatArgb128);

  const std::array<uint8_t, width * height * 4> expected8{
      170, 85, 20, 10, 255, 85, 20, 10, 170, 85, 20, 10};
  ok = ok && suite->box_blur(
      g_hooks.effect_ref, &source_world, 1.0f, 0.0f, 1,
      kHorizontal | kAllChannels, 0, 1, &destination_world) == 0 &&
      destination == expected8 && source == source_before;

  const std::array<uint16_t, width * height * 4> expected16{
      21845, 10923, 333, 167, 32768, 10923, 333, 167,
      21845, 10923, 333, 167};
  ok = ok && suite->box_blur(
      g_hooks.effect_ref, &source_world16, 1.0f, 0.0f, 1,
      kHorizontal | kAllChannels, 0, 1, &destination_world16) == 0 &&
      destination16 == expected16 && source16 == source16_before;

  ok = ok && suite->gaussian_blur(
      g_hooks.effect_ref, &source_world_float, 1.0f, 0.0f,
      kHorizontal | kRepeatEdgePixels | kAllChannels, 1, 0, 1,
      &destination_world_float) == 0 &&
      source_float == source_float_before;
  constexpr std::array<float, 3> expected_red{
      0.10650698f, 0.78698605f, 0.10650698f};
  for (int32_t x = 0; x < width; ++x) {
    const std::size_t pixel = static_cast<std::size_t>(x) * 4;
    ok = ok && std::abs(destination_float[pixel] - 1.0f) < 1e-6f &&
        std::abs(destination_float[pixel + 1] - expected_red[x]) < 1e-5f &&
        std::abs(destination_float[pixel + 2] - expected_red[x] * 0.25f) <
            1e-5f &&
        std::abs(destination_float[pixel + 3] - expected_red[x] * 0.125f) <
            1e-5f;
  }

  ok = ok && suite->gaussian_blur(
      reinterpret_cast<void*>(1), &source_world, 1.0f, 0.0f,
      kHorizontal | kAllChannels, 1, 0, 1, &destination_world) ==
          kBadCallbackParam &&
      suite->gaussian_blur(
          g_hooks.effect_ref, &source_world, 1.0f, 0.0f,
          kHorizontal | kAllChannels, 2, 0, 1, &destination_world) ==
          kBadCallbackParam &&
      suite->gaussian_blur(
          g_hooks.effect_ref, &source_world, 1.0f, 0.0f,
          kHorizontal | kAllChannels, 1, 2, 1, &destination_world) ==
          kBadCallbackParam &&
      suite->box_blur(
          g_hooks.effect_ref, &source_world, -1.0f, 0.0f, 1,
          kHorizontal | kAllChannels, 0, 1, &destination_world) ==
          kBadCallbackParam &&
      suite->box_blur(
          g_hooks.effect_ref, &source_world, 1.0f, 0.0f, 0,
          kHorizontal | kAllChannels, 0, 1, &destination_world) ==
          kBadCallbackParam &&
      suite->box_blur(
          g_hooks.effect_ref, &source_world, 1.0f, 0.0f, 1, 0, 0, 1,
          &destination_world) == kBadCallbackParam &&
      suite->box_blur(
          g_hooks.effect_ref, nullptr, 1.0f, 0.0f, 1,
          kHorizontal | kAllChannels, 0, 1, &destination_world) ==
          kBadCallbackParam;

  // compute_directional_blur_radii (slot 2): the smear projected onto x/y,
  // rounded up. At 90 degrees the blur is horizontal, so x takes the whole
  // magnitude (ceil(1*2*5)=10) while y is |cos(pi/2)|=6.1e-17 rounded up to 1 -
  // AE's own ceil-of-epsilon result, reproduced here because the host runs the
  // same sin/cos and ceil. At 0 degrees sin is exactly 0 so x collapses to 0
  // and y takes it all; 45 degrees splits by 1/sqrt(2). These match AE's
  // FLT_ComputeDirectionalBlurRadii exactly.
  int32_t radius_x = -1, radius_y = -1;
  ok = ok &&
      suite->compute_directional_blur_radii(2.0, 3.0, 90.0, 5.0, &radius_x,
                                            &radius_y) == 0 &&
      radius_x == 10 && radius_y == 1 &&
      suite->compute_directional_blur_radii(2.0, 3.0, 0.0, 5.0, &radius_x,
                                            &radius_y) == 0 &&
      radius_x == 0 && radius_y == 15 &&
      suite->compute_directional_blur_radii(1.0, 1.0, 45.0, 10.0, &radius_x,
                                            &radius_y) == 0 &&
      radius_x == 8 && radius_y == 8 &&
      suite->compute_directional_blur_radii(1.0, 1.0, 0.0, 1.0, nullptr,
                                            &radius_y) == kBadCallbackParam &&
      suite->compute_directional_blur_radii(1.0, 1.0, 0.0, 1.0, &radius_x,
                                            nullptr) == kBadCallbackParam;

  return g_hooks.release_suite(kSuiteName, kSuiteVersion1) == 0 && ok;
}

}  // namespace aexcompat::flt_blur
