#include "worker_flt_blur_suite.hpp"

#include "worker_world_registry.hpp"

#include <iostream>

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
                    bool gaussian, int32_t iterations,
                    const char** reason = nullptr) {
  const auto refused = [reason](const char* value) {
    if (reason) *reason = value;
    return kBadCallbackParam;
  };
  int32_t pixel_bytes{};
  if (!compatible_worlds(source, destination, pixel_bytes))
    return refused("world_pair");
  if (!valid_radius(radius_x) || !valid_radius(radius_y) ||
      !valid_flags(flags) || iterations <= 0 ||
      iterations > kMaximumIterations)
    return refused("invalid_arguments");
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
    return refused("allocation_failed");
  }
}

// Answers a refused FLT call naming the condition that refused it, the
// copy_denied shape (worker_pf_world_transform_runtime.cpp, issue #1037): the
// plug-in folds the 4 into its own frame error and names neither the callback
// nor the argument, so without the marker the refusing check is recoverable
// only by rebuilding the worker with prints (issue #1069 filed Cartoon as
// "frame_error:4 with no trace" for exactly this gap). Always on; both fields
// are lower-case identifiers, the shape the broker's `callback_denials`
// parser vouches for.
int32_t flt_denied(const char* callback, const char* reason) {
  std::cerr << "stage:callback_denied callback=" << callback
            << " reason=" << reason << "\n" << std::flush;
  return kBadCallbackParam;
}

// A blur operand the dispatch-format registry has never seen, admitted the way
// copy_world8 admits its foreign operands (issue #1037's Wave Warp pattern):
// by the declared-stride bounds check, not by ownership. Cartoon builds its
// edge-detection scratch in its own allocations, wraps them in stack
// PF_EffectWorlds, and hands both to FLT box_blur; AE's FLT.dll takes any
// PF_World the caller can describe, and refusing them failed the whole frame
// with 4 (issue #1069). The same two refusals survive for the same reasons as
// copy_world8's: a reference the registry already knows (a registered struct
// gone stale, or a re-declaration of a registered pixel base) stays the
// registry's fail-closed mismatch refusal, and a pixel base the host allocated
// keeps its own allocation's geometry check authoritative. Both guards hold on
// the thread that holds the dispatch scope (the registry is thread_local),
// which is every thread the host itself dispatches on; a plug-in-spawned
// thread bypasses the scope and is contained by the worker process, the same
// residual copy_world8 documents.
bool resolve_foreign_blur_world(const void* world, int32_t pixel_format,
                                world_safety::DispatchWorldFormat& result) {
  if (!g_hooks.bounded_world || !g_hooks.world_reference_known ||
      !g_hooks.world_pixels_owned || !g_hooks.session_pixel_format ||
      g_hooks.world_reference_known(world))
    return false;
  auto* mutable_world = const_cast<void*>(world);
  if (g_hooks.world_pixels_owned(mutable_world)) return false;
  const int32_t pixel_bytes = bytes_per_pixel(pixel_format);
  unsigned char* pixels{};
  int32_t rowbytes{}, width{}, height{};
  if (!pixel_bytes || !g_hooks.bounded_world(mutable_world, pixel_bytes, pixels,
                                             rowbytes, width, height))
    return false;
  result = {};
  result.world = world;
  result.data = pixels;
  result.width = width;
  result.height = height;
  result.rowbytes = rowbytes;
  result.pixel_format = pixel_format;
  return true;
}

int32_t session_pixel_format_value() {
  const char* name = g_hooks.session_pixel_format
      ? g_hooks.session_pixel_format() : nullptr;
  if (!name) return 0;
  if (std::strcmp(name, "argb8") == 0) return world_registry::kPixelFormatArgb32;
  if (std::strcmp(name, "argb16") == 0) return world_registry::kPixelFormatArgb64;
  if (std::strcmp(name, "argb32f") == 0) return world_registry::kPixelFormatArgb128;
  return 0;
}

// Resolves the source/destination pair, admitting foreign operands against a
// format anchor: the resolved side's format when one side is known, else the
// session's negotiated pixel format (Cartoon passes two foreign scratch worlds
// inside an argb32f render, so there is no resolved side to borrow from). A
// misanchored foreign world fails closed in the stride check -
// `bounded_world` requires rowbytes >= width * pixel_bytes at the anchor's
// pixel size, and `compatible_worlds` in the caller re-checks the pair as a
// whole - so the anchor can under-read a wider world's row but never walk
// outside what the plug-in declared.
bool resolve_blur_worlds(const void* source_world, void* destination_world,
                         world_safety::DispatchWorldFormat& source,
                         world_safety::DispatchWorldFormat& destination) {
  if (!g_hooks.resolve_world) return false;
  const bool source_known = g_hooks.resolve_world(source_world, source);
  const bool destination_known =
      g_hooks.resolve_world(destination_world, destination);
  if (source_known && destination_known) return true;
  const int32_t anchor = source_known ? source.pixel_format
      : destination_known ? destination.pixel_format
                          : session_pixel_format_value();
  if (!bytes_per_pixel(anchor)) return false;
  if (!source_known && !resolve_foreign_blur_world(source_world, anchor, source))
    return false;
  return destination_known ||
      resolve_foreign_blur_world(destination_world, anchor, destination);
}

int32_t __cdecl gaussian_blur(
    void* effect_ref, const void* source_world, float radius_x, float radius_y,
    int32_t flags, int32_t quality, int32_t progress_base,
    int32_t progress_final, void* destination_world) {
  if (!g_hooks.effect_ref || effect_ref != g_hooks.effect_ref ||
      !g_hooks.resolve_world || (quality != 0 && quality != 1) ||
      !valid_progress(progress_base, progress_final))
    return flt_denied("flt_gaussian_blur", "invalid_arguments");
  world_safety::DispatchWorldFormat source{}, destination{};
  if (!resolve_blur_worlds(source_world, destination_world, source, destination))
    return flt_denied("flt_gaussian_blur", "unresolved_world");
  const char* reason = "";
  const int32_t result = blur_worlds(source, destination, radius_x, radius_y,
                                     flags, true, 1, &reason);
  return result == 0 ? 0 : flt_denied("flt_gaussian_blur", reason);
}

int32_t __cdecl box_blur(
    void* effect_ref, const void* source_world, float radius_x, float radius_y,
    int32_t iterations, int32_t flags, int32_t progress_base,
    int32_t progress_final, void* destination_world) {
  if (!g_hooks.effect_ref || effect_ref != g_hooks.effect_ref ||
      !g_hooks.resolve_world || !valid_progress(progress_base, progress_final))
    return flt_denied("flt_box_blur", "invalid_arguments");
  world_safety::DispatchWorldFormat source{}, destination{};
  if (!resolve_blur_worlds(source_world, destination_world, source, destination))
    return flt_denied("flt_box_blur", "unresolved_world");
  const char* reason = "";
  const int32_t result = blur_worlds(source, destination, radius_x, radius_y,
                                     flags, false, iterations, &reason);
  return result == 0 ? 0 : flt_denied("flt_box_blur", reason);
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

// Slot 3 (FLT_DirectionalBlur). The tap bound mirrors slot 0/1's kMaximumRadius:
// a smear whose half-length would exceed it is rejected rather than run, so the
// per-pixel accumulation stays bounded even for an adversarial length.
constexpr int32_t kMaximumDirectionalTaps = 4096;

// Bilinear sample of the interleaved ARGB float buffer at fractional pixel
// coordinates. Samples outside the source contribute transparent black (0),
// matching FLT's padded intermediate world (FUN_18002ebb0 builds a larger
// PF_World, so the smear fades to transparent at the source edges) and the
// destination the plug-in pre-cleared to 0.
float sample_bilinear(const std::vector<float>& pixels, int32_t width,
                      int32_t height, double fx, double fy,
                      int32_t channel) noexcept {
  const double x0f = std::floor(fx);
  const double y0f = std::floor(fy);
  const int32_t x0 = static_cast<int32_t>(x0f);
  const int32_t y0 = static_cast<int32_t>(y0f);
  const float tx = static_cast<float>(fx - x0f);
  const float ty = static_cast<float>(fy - y0f);
  const auto at = [&](int32_t x, int32_t y) -> float {
    if (x < 0 || x >= width || y < 0 || y >= height) return 0.0f;
    return pixels[(static_cast<std::size_t>(y) * width + x) * 4 + channel];
  };
  const float top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * tx;
  const float bottom = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * tx;
  return top + (bottom - top) * ty;
}

int32_t __cdecl directional_blur(
    void* effect_ref, int32_t quality, double downsample_x, double downsample_y,
    double length, double direction_degrees, const void* source_world,
    void* destination_world) {
  if (!g_hooks.effect_ref || effect_ref != g_hooks.effect_ref ||
      !g_hooks.resolve_world || (quality != 0 && quality != 1) ||
      !std::isfinite(length) || length <= 0.0 ||
      length > static_cast<double>(kMaximumRadius) ||
      !std::isfinite(direction_degrees) || !std::isfinite(downsample_x) ||
      !std::isfinite(downsample_y) || downsample_x <= 0.0 ||
      downsample_y <= 0.0)
    return flt_denied("flt_directional_blur", "invalid_arguments");
  world_safety::DispatchWorldFormat source{}, destination{};
  if (!resolve_blur_worlds(source_world, destination_world, source, destination))
    return flt_denied("flt_directional_blur", "unresolved_world");
  int32_t pixel_bytes{};
  if (!compatible_worlds(source, destination, pixel_bytes))
    return flt_denied("flt_directional_blur", "world_pair");

  const double radians = direction_degrees * kDegreesToRadians;
  const double sin_theta = std::sin(radians);
  const double cos_theta = std::cos(radians);
  // AE's visible smear extents (FLT FUN_180032690 param_6[4]/[5]): the blur
  // projects onto x by |sin| and onto y by |cos|, each scaled by the matching
  // downsample. Same sin->x / cos->y convention as slot 2.
  const double extent_x = length * downsample_x * std::fabs(sin_theta);
  const double extent_y = length * downsample_y * std::fabs(cos_theta);
  const double half_length = std::hypot(extent_x, extent_y);
  // Bound the extent as a double before the cast. downsample_x/y have no upper
  // bound, so half_length can exceed INT32_MAX, where a double->int32 cast is
  // undefined; on x64 it yields INT_MIN, which would slip past an int-domain
  // bound check straight into the zero-extent branch and pass an out-of-range
  // input off as a successful passthrough. Rejecting in the double domain keeps
  // the tap count in [0, kMaximumDirectionalTaps] so the cast is well defined,
  // matching slot 2's bounded_ceil_to_int guard.
  if (!std::isfinite(half_length) ||
      half_length > static_cast<double>(kMaximumDirectionalTaps))
    return flt_denied("flt_directional_blur", "extent_bound");
  const int32_t taps = static_cast<int32_t>(std::ceil(half_length));
  // Signed half-vector of the centered smear (both directions blend around the
  // pixel, so the sign only orients the axis, not the result).
  const double vector_x = (sin_theta >= 0.0 ? 1.0 : -1.0) * extent_x;
  const double vector_y = (cos_theta >= 0.0 ? 1.0 : -1.0) * extent_y;

  try {
    const std::size_t values = static_cast<std::size_t>(source.width) *
        source.height * 4;
    std::vector<float> input(values);
    if (pixel_bytes == 4) load_pixels<uint8_t>(source, input);
    else if (pixel_bytes == 8) load_pixels<uint16_t>(source, input);
    else load_pixels<float>(source, input);

    std::vector<float> output(values);
    const int32_t width = source.width;
    const int32_t height = source.height;
    if (taps <= 0) {
      // Zero-extent case: half_length underflowed to exactly 0 (both axis
      // extents collapsed, e.g. a subnormal downsample), so the centered box is
      // the identity. A sub-1px extent is not this case: it rounds up to one
      // tap and runs the blur below.
      output = input;
    } else {
      const double inverse_taps = 1.0 / static_cast<double>(taps);
      const float weight = 1.0f / static_cast<float>(2 * taps + 1);
      for (int32_t y = 0; y < height; ++y) {
        for (int32_t x = 0; x < width; ++x) {
          float accumulator[4] = {0.0f, 0.0f, 0.0f, 0.0f};
          for (int32_t tap = -taps; tap <= taps; ++tap) {
            const double t = static_cast<double>(tap) * inverse_taps;
            const double sample_x = static_cast<double>(x) + t * vector_x;
            const double sample_y = static_cast<double>(y) + t * vector_y;
            for (int32_t channel = 0; channel < 4; ++channel)
              accumulator[channel] += sample_bilinear(input, width, height,
                                                       sample_x, sample_y,
                                                       channel);
          }
          const std::size_t destination_index =
              (static_cast<std::size_t>(y) * width + x) * 4;
          for (int32_t channel = 0; channel < 4; ++channel)
            output[destination_index + channel] =
                accumulator[channel] * weight;
        }
      }
    }
    if (pixel_bytes == 4) store_pixels<uint8_t>(output, destination);
    else if (pixel_bytes == 8) store_pixels<uint16_t>(output, destination);
    else store_pixels<float>(output, destination);
    return 0;
  } catch (const std::bad_alloc&) {
    return flt_denied("flt_directional_blur", "allocation_failed");
  }
}

Suite1 g_suite1{&gaussian_blur, &box_blur, &compute_directional_blur_radii,
                &directional_blur};

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
      !suite->compute_directional_blur_radii || !suite->directional_blur) {
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

  // Foreign-operand admission (issue #1069, the copy_world8 latitude from
  // issue #1037): worlds the registry never saw are admitted by the
  // declared-stride bounds check. Cartoon hands box_blur two of its own
  // scratch worlds, so both the one-sided anchor (format borrowed from the
  // resolved side) and the both-foreign anchor (the session's negotiated
  // format, "argb8" here) must answer. A re-declaration of a registered
  // world's pixel base under a different geometry stays refused: that
  // reference is the registry's to validate, and it must not degrade into
  // foreign admission.
  std::array<uint8_t, width * height * 4> foreign_source{
      255, 0, 0, 0, 255, 255, 60, 30, 255, 0, 0, 0};
  std::array<uint8_t, width * height * 4> foreign_destination{};
  auto foreign_source_world = make_world(foreign_source.data(), width * 4);
  auto foreign_destination_world =
      make_world(foreign_destination.data(), width * 4);
  destination = {};
  ok = ok && suite->box_blur(
      g_hooks.effect_ref, &foreign_source_world, 1.0f, 0.0f, 1,
      kHorizontal | kAllChannels, 0, 1, &destination_world) == 0 &&
      destination == expected8;
  ok = ok && suite->box_blur(
      g_hooks.effect_ref, &foreign_source_world, 1.0f, 0.0f, 1,
      kHorizontal | kAllChannels, 0, 1, &foreign_destination_world) == 0 &&
      foreign_destination == expected8;
  auto aliased_registered_base = make_world(source.data(), width * 4);
  aliased_registered_base.height = height + 1;
  ok = ok && suite->box_blur(
      g_hooks.effect_ref, &aliased_registered_base, 1.0f, 0.0f, 1,
      kHorizontal | kAllChannels, 0, 1, &foreign_destination_world) ==
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

  // directional_blur (slot 3): a five-wide row with a single opaque-white
  // impulse at x=2. Direction 90 degrees is a horizontal smear (|sin 90|=1 on
  // x, |cos 90|~0 on y); length 1 at downsample 1 gives extent 1px, so each
  // output pixel averages three integer-aligned taps {x-1, x, x+1}. Only the
  // taps that land on x=2 carry the impulse, and 255/3 rounds to 85, so the
  // white spreads to x in {1,2,3} and the edges stay clear. Bilinear sampling
  // outside the row yields transparent black, so no channel exceeds the
  // impulse. This checks direction, extent, and edge handling deterministically
  // without depending on AE's RenderGraph normalization.
  constexpr int32_t dir_width = 5;
  constexpr int32_t dir_height = 1;
  const auto make_dir_world = [=](void* pixels, int32_t rowbytes) {
    world_safety::LocalEffectWorld world{};
    world.data = pixels;
    world.rowbytes = rowbytes;
    world.width = dir_width;
    world.height = dir_height;
    world.extent_hint = {0, 0, dir_width, dir_height};
    world.pix_aspect_ratio = {1, 1};
    return world;
  };
  std::array<uint8_t, dir_width * dir_height * 4> dir_source{
      0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0};
  std::array<uint8_t, dir_width * dir_height * 4> dir_destination{};
  const auto dir_source_before = dir_source;
  auto dir_source_world = make_dir_world(dir_source.data(), dir_width * 4);
  auto dir_destination_world =
      make_dir_world(dir_destination.data(), dir_width * 4);
  world_safety::DispatchWorldFormatScope dir_formats;
  ok = ok &&
      dir_formats.register_world(&dir_source_world,
                                 world_registry::kPixelFormatArgb32) &&
      dir_formats.register_world(&dir_destination_world,
                                 world_registry::kPixelFormatArgb32);
  const std::array<uint8_t, dir_width * dir_height * 4> dir_expected{
      0, 0, 0, 0, 85, 85, 85, 85, 85, 85, 85, 85, 85, 85, 85, 85, 0, 0, 0, 0};
  ok = ok && suite->directional_blur(
      g_hooks.effect_ref, 1, 1.0, 1.0, 1.0, 90.0, &dir_source_world,
      &dir_destination_world) == 0 &&
      dir_destination == dir_expected && dir_source == dir_source_before;
  // Fail-closed rejections, all of which must return kBadCallbackParam without
  // touching the destination:
  //   - foreign identity;
  //   - non-positive length (0), and over-cap length isolated from the extent
  //     bound (5000 > kMaximumRadius but extent 5000*0.5 = 2500 < tap bound,
  //     so only the length clause rejects it);
  //   - non-finite length (inf);
  //   - the extent/tap bound, both in range (2.0,4096) and via an overflow-range
  //     downsample (1e9,4096) that would make the int cast undefined without the
  //     pre-cast double-domain bound;
  //   - zero, negative, and non-finite downsample on both axes;
  //   - non-finite direction; out-of-range quality; and null/unresolvable
  //     worlds.
  constexpr double dir_inf = std::numeric_limits<double>::infinity();
  constexpr double dir_nan = std::numeric_limits<double>::quiet_NaN();
  ok = ok &&
      suite->directional_blur(reinterpret_cast<void*>(1), 1, 1.0, 1.0, 1.0,
                              90.0, &dir_source_world,
                              &dir_destination_world) == kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 1.0, 1.0, 0.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 0.5, 0.5, 5000.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 1.0, 1.0, dir_inf, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 2.0, 1.0, 4096.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 1e9, 1.0, 4096.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 0.0, 1.0, 1.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 1.0, 0.0, 1.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, -1.0, 1.0, 1.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 1.0, -1.0, 1.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, dir_inf, 1.0, 1.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 1.0, dir_inf, 1.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 1.0, 1.0, 1.0, dir_nan,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 2, 1.0, 1.0, 1.0, 90.0,
                              &dir_source_world, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 1.0, 1.0, 1.0, 90.0,
                              nullptr, &dir_destination_world) ==
          kBadCallbackParam &&
      suite->directional_blur(g_hooks.effect_ref, 1, 1.0, 1.0, 1.0, 90.0,
                              &dir_source_world, nullptr) == kBadCallbackParam;
  // None of the rejected calls may have written the destination.
  ok = ok && dir_destination == dir_expected;

  return g_hooks.release_suite(kSuiteName, kSuiteVersion1) == 0 && ok;
}

}  // namespace aexcompat::flt_blur
