#include "worker_pf_suites_internal.hpp"
#include "worker_world_safety.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <mutex>
#include <string>
#include <thread>
#include <type_traits>
#include <unordered_map>
#include <vector>

using aexcompat::world_safety::DispatchWorldFormat;
using aexcompat::world_safety::DispatchWorldFormatScope;
using aexcompat::world_safety::kEffectWorldSize;
using aexcompat::world_safety::bounded_argb8_world;

namespace {

constexpr int32_t kPfBadCallbackParam = 4;
constexpr int32_t kPfErrBadCallbackParam = 516;
constexpr int32_t kPixelFormatArgb32 = 1650946657;
constexpr int32_t kPixelFormatArgb64 = 1650946658;
constexpr int32_t kPixelFormatArgb128 = 1650946659;
constexpr uint64_t kMaxAsyncReceiptBytes = 64ULL * 1024 * 1024;
constexpr std::size_t kInSize = 408;
constexpr std::size_t kInEffectRef = 184;
constexpr std::size_t kUtilsSize = 552;
constexpr std::size_t kUtilsFill = 72;
constexpr std::size_t kUtilsPremultiply = 96;
constexpr std::size_t kUtilsPremultiplyColor = 104;
constexpr std::size_t kUtilsFill16 = 488;
constexpr std::size_t kUtilsPremultiplyColor16 = 496;

PfHostContext g_pf_host{};
bool g_pf_host_configured{};

bool resolve_world(void* world, int32_t pixel_bytes, unsigned char*& pixels,
                   int32_t& rowbytes, int32_t& width, int32_t& height) {
  return g_pf_host_configured && g_pf_host.hooks.resolve_world &&
      g_pf_host.hooks.resolve_world(world, pixel_bytes, pixels, rowbytes, width, height);
}

bool resolve_dispatch_world_format(const void* world, DispatchWorldFormat& result) {
  return g_pf_host_configured && g_pf_host.hooks.resolve_dispatch_world_format &&
      g_pf_host.hooks.resolve_dispatch_world_format(world, result);
}

const char* pixel_format() {
  return g_pf_host_configured && g_pf_host.hooks.pixel_format
      ? g_pf_host.hooks.pixel_format() : "";
}

bool set_pixel_format(const char* value) {
  return g_pf_host_configured && g_pf_host.hooks.set_pixel_format &&
      g_pf_host.hooks.set_pixel_format(value);
}

int32_t acquire_host_suite(const char* name, int32_t version, const void** suite) {
  if (!suite) return kPfBadCallbackParam;
  *suite = nullptr;
  return g_pf_host_configured && g_pf_host.hooks.acquire_suite
      ? g_pf_host.hooks.acquire_suite(name, version, suite) : kPfBadCallbackParam;
}

int32_t release_host_suite(const char* name, int32_t version) {
  return g_pf_host_configured && g_pf_host.hooks.release_suite
      ? g_pf_host.hooks.release_suite(name, version) : kPfBadCallbackParam;
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

namespace {

struct ColorValues { double r, g, b; };
struct HlsValues { double h, l, s; };

bool finite3(double a, double b, double c) {
  return std::isfinite(a) && std::isfinite(b) && std::isfinite(c);
}

double color_from_fixed(PfFixed value) {
  return static_cast<double>(value) / 65536.0;
}

HlsValues rgb_to_hls_values(const ColorValues& c) {
  const double hi = (std::max)({c.r, c.g, c.b});
  const double lo = (std::min)({c.r, c.g, c.b});
  const double l = (hi + lo) * 0.5;
  if (hi == lo) return {0.0, l, 0.0};
  const double d = hi - lo;
  const double denominator = l <= 0.5 ? hi + lo : 2.0 - hi - lo;
  const double s = denominator == 0.0 ? 0.0 : d / denominator;
  double h = c.r == hi ? (c.g - c.b) / d
           : c.g == hi ? 2.0 + (c.b - c.r) / d
                        : 4.0 + (c.r - c.g) / d;
  h /= 6.0;
  h -= std::floor(h);
  return {h, l, s};
}

double hls_component(double p, double q, double t) {
  t -= std::floor(t);
  if (t < 1.0 / 6.0) return p + (q - p) * 6.0 * t;
  if (t < 0.5) return q;
  if (t < 2.0 / 3.0) return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
  return p;
}

ColorValues hls_to_rgb_values(const HlsValues& hls) {
  if (hls.s == 0.0) return {hls.l, hls.l, hls.l};
  const double q = hls.l < 0.5 ? hls.l * (1.0 + hls.s)
                               : hls.l + hls.s - hls.l * hls.s;
  const double p = 2.0 * hls.l - q;
  return {hls_component(p, q, hls.h + 1.0 / 3.0), hls_component(p, q, hls.h),
          hls_component(p, q, hls.h - 1.0 / 3.0)};
}

ColorValues rgb_to_yiq_values(const ColorValues& c) {
  return {0.2989 * c.r + 0.5866 * c.g + 0.1144 * c.b,
          0.5959 * c.r - 0.2741 * c.g - 0.3218 * c.b,
          0.2113 * c.r - 0.5227 * c.g + 0.3113 * c.b};
}

ColorValues yiq_to_rgb_values(const ColorValues& c) {
  return {c.r + 0.9562 * c.g + 0.6210 * c.b,
          c.r - 0.2717 * c.g - 0.6485 * c.b,
          c.r - 1.1053 * c.g + 1.7020 * c.b};
}

template <class Pixel> struct ColorPixelTraits;
template <> struct ColorPixelTraits<PfPixel8> {
  static ColorValues read(const PfPixel8& p) {
    return {p.red / 255.0, p.green / 255.0, p.blue / 255.0};
  }
  static void write(PfPixel8& p, const ColorValues& c) {
    auto channel = [](double value) {
      return static_cast<uint8_t>(std::lround(
          (std::max)(0.0, (std::min)(1.0, value)) * 255.0));
    };
    p.red = channel(c.r); p.green = channel(c.g); p.blue = channel(c.b);
  }
  static constexpr double scale = 255.0;
};
template <> struct ColorPixelTraits<PfPixel16> {
  static ColorValues read(const PfPixel16& p) {
    return {p.red / 32768.0, p.green / 32768.0, p.blue / 32768.0};
  }
  static void write(PfPixel16& p, const ColorValues& c) {
    auto channel = [](double value) {
      return static_cast<uint16_t>(std::lround(
          (std::max)(0.0, (std::min)(1.0, value)) * 32768.0));
    };
    p.red = channel(c.r); p.green = channel(c.g); p.blue = channel(c.b);
  }
  static constexpr double scale = 32768.0;
};
template <> struct ColorPixelTraits<PfPixelFloat> {
  static ColorValues read(const PfPixelFloat& p) { return {p.red, p.green, p.blue}; }
  static void write(PfPixelFloat& p, const ColorValues& c) {
    p.red = static_cast<float>(c.r);
    p.green = static_cast<float>(c.g);
    p.blue = static_cast<float>(c.b);
  }
  static constexpr double scale = 1.0;
};

template <class Pixel> int32_t __cdecl color_rgb_to_hls(
    void*, Pixel* rgb, PfFixedTriple out) {
  if (!rgb || !out) return kPfErrBadCallbackParam;
  const ColorValues c = ColorPixelTraits<Pixel>::read(*rgb);
  if (!finite3(c.r, c.g, c.b)) return kPfErrBadCallbackParam;
  const HlsValues hls = rgb_to_hls_values(c);
  if (!finite3(hls.h, hls.l, hls.s)) return kPfErrBadCallbackParam;
  PfFixed result[3]{pf_color_to_fixed(hls.h * 360.0), pf_color_to_fixed(hls.l),
                    pf_color_to_fixed(hls.s)};
  std::memcpy(out, result, sizeof(result));
  return 0;
}

template <class Pixel> int32_t __cdecl color_hls_to_rgb(
    void*, PfFixedTriple in, Pixel* rgb) {
  if (!in || !rgb) return kPfErrBadCallbackParam;
  const HlsValues hls{color_from_fixed(in[0]) / 360.0, color_from_fixed(in[1]),
                      color_from_fixed(in[2])};
  const ColorValues c = hls_to_rgb_values(hls);
  if (!finite3(c.r, c.g, c.b)) return kPfErrBadCallbackParam;
  ColorPixelTraits<Pixel>::write(*rgb, c);
  return 0;
}

template <class Pixel> int32_t __cdecl color_rgb_to_yiq(
    void*, Pixel* rgb, PfFixedTriple out) {
  if (!rgb || !out) return kPfErrBadCallbackParam;
  const ColorValues c = ColorPixelTraits<Pixel>::read(*rgb);
  if (!finite3(c.r, c.g, c.b)) return kPfErrBadCallbackParam;
  const ColorValues yiq = rgb_to_yiq_values(c);
  PfFixed result[3]{pf_color_to_fixed(yiq.r), pf_color_to_fixed(yiq.g),
                    pf_color_to_fixed(yiq.b)};
  std::memcpy(out, result, sizeof(result));
  return 0;
}

template <class Pixel> int32_t __cdecl color_yiq_to_rgb(
    void*, PfFixedTriple in, Pixel* rgb) {
  if (!in || !rgb) return kPfErrBadCallbackParam;
  const ColorValues yiq{color_from_fixed(in[0]), color_from_fixed(in[1]),
                        color_from_fixed(in[2])};
  const ColorValues c = yiq_to_rgb_values(yiq);
  if (!finite3(c.r, c.g, c.b)) return kPfErrBadCallbackParam;
  ColorPixelTraits<Pixel>::write(*rgb, c);
  return 0;
}

template <class Pixel, class Scalar, int Which>
int32_t __cdecl color_scalar(void*, Pixel* rgb, Scalar* out) {
  if (!rgb || !out) return kPfErrBadCallbackParam;
  const ColorValues c = ColorPixelTraits<Pixel>::read(*rgb);
  if (!finite3(c.r, c.g, c.b)) return kPfErrBadCallbackParam;
  const HlsValues hls = rgb_to_hls_values(c);
  const double value = Which == 0 ? rgb_to_yiq_values(c).r
                     : Which == 1 ? hls.h : Which == 2 ? hls.l : hls.s;
  if (!std::isfinite(value)) return kPfErrBadCallbackParam;
  if constexpr (std::is_same_v<Scalar, float>) {
    *out = static_cast<float>(Which == 1 ? value * 360.0 : value);
  } else {
    const double scale = Which == 0 ? 100.0 * ColorPixelTraits<Pixel>::scale
                         : Which == 1 ? 255.0 : ColorPixelTraits<Pixel>::scale;
    *out = static_cast<int32_t>(std::lround(value * scale));
  }
  return 0;
}

}  // namespace

PfFixed pf_color_to_fixed(double value) {
  const double scaled = value * 65536.0;
  if (scaled >= static_cast<double>(INT32_MAX)) return INT32_MAX;
  if (scaled <= static_cast<double>(INT32_MIN)) return INT32_MIN;
  // This follows the SDK macro; unobserved AE tie behavior is not asserted here.
  return static_cast<PfFixed>(scaled + (scaled < 0.0 ? -0.5 : 0.5));
}

#define PF_COLOR_SUITE(P, S) {&color_rgb_to_hls<P>, &color_hls_to_rgb<P>, \
  &color_rgb_to_yiq<P>, &color_yiq_to_rgb<P>, &color_scalar<P, S, 0>, \
  &color_scalar<P, S, 1>, &color_scalar<P, S, 2>, &color_scalar<P, S, 3>}
PfColorCallbacks8 g_color_suite8 = PF_COLOR_SUITE(PfPixel8, int32_t);
PfColorCallbacks16 g_color_suite16 = PF_COLOR_SUITE(PfPixel16, int32_t);
PfColorCallbacksFloat g_color_suite_float = PF_COLOR_SUITE(PfPixelFloat, float);
#undef PF_COLOR_SUITE

static_assert(sizeof(PfColorCallbacks8) == 8 * sizeof(void*));
static_assert(sizeof(PfColorCallbacks16) == 8 * sizeof(void*));
static_assert(sizeof(PfColorCallbacksFloat) == 8 * sizeof(void*));
static_assert(offsetof(PfColorCallbacks8, RGBtoHLS) == 0 * sizeof(void*));
static_assert(offsetof(PfColorCallbacks8, Saturation) == 7 * sizeof(void*));

PfPathPoint eval_pf_cubic(const PfPathCubic& c, double t) {
  auto lerp = [](const PfPathPoint& a, const PfPathPoint& b, double amount) {
    return PfPathPoint{a[0] + (b[0] - a[0]) * amount,
                       a[1] + (b[1] - a[1]) * amount};
  };
  const auto a = lerp(c[0], c[1], t);
  const auto b = lerp(c[1], c[2], t);
  const auto d = lerp(c[2], c[3], t);
  return lerp(lerp(a, b, t), lerp(b, d, t), t);
}

PfPathPoint deriv_pf_cubic(const PfPathCubic& c, double t) {
  const double u = 1.0 - t;
  return {3.0 * (u * u * (c[1][0] - c[0][0]) +
                       2.0 * u * t * (c[2][0] - c[1][0]) +
                       t * t * (c[3][0] - c[2][0])),
          3.0 * (u * u * (c[1][1] - c[0][1]) +
                       2.0 * u * t * (c[2][1] - c[1][1]) +
                       t * t * (c[3][1] - c[2][1]))};
}

double pf_path_point_distance(const PfPathPoint& a, const PfPathPoint& b) {
  return std::hypot(b[0] - a[0], b[1] - a[1]);
}

void append_adaptive_pf_cubic(const PfPathCubic& c, double t0, double t1,
                              double tolerance, int depth,
                              std::vector<double>& parameters,
                              std::vector<PfPathPoint>& points) {
  constexpr int kMaxDepth = 20;
  constexpr std::size_t kMaxPoints = 65537;
  const double chord = pf_path_point_distance(c[0], c[3]);
  const double polygon = pf_path_point_distance(c[0], c[1]) +
      pf_path_point_distance(c[1], c[2]) + pf_path_point_distance(c[2], c[3]);
  if (depth >= kMaxDepth || points.size() >= kMaxPoints ||
      polygon - chord <= tolerance) {
    parameters.push_back(t1);
    points.push_back(c[3]);
    return;
  }
  auto lerp = [](const PfPathPoint& a, const PfPathPoint& b) {
    return PfPathPoint{(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5};
  };
  const auto p01 = lerp(c[0], c[1]);
  const auto p12 = lerp(c[1], c[2]);
  const auto p23 = lerp(c[2], c[3]);
  const auto p012 = lerp(p01, p12);
  const auto p123 = lerp(p12, p23);
  const auto midpoint = lerp(p012, p123);
  const double tm = (t0 + t1) * 0.5;
  append_adaptive_pf_cubic({c[0], p01, p012, midpoint}, t0, tm,
                           tolerance * 0.5, depth + 1, parameters, points);
  append_adaptive_pf_cubic({midpoint, p123, p23, c[3]}, tm, t1,
                           tolerance * 0.5, depth + 1, parameters, points);
}

Iterate8Suite2 g_iterate8_suite2{reinterpret_cast<void*>(&iterate_world8), &iterate_origin8, &iterate_lut8,
    &iterate_origin_non_clip8, &iterate_generic};
Iterate16Suite2 g_iterate16_suite2{&iterate_world16, &iterate_origin16,
    &iterate_origin_non_clip16};
IterateFloatSuite2 g_iterate_float_suite2{&iterate_world_float, &iterate_origin_float,
    &iterate_origin_non_clip_float};
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
    &unsupported_batch_sample_func};
std::array<void*, 7> g_fill_matte_suite2{
    reinterpret_cast<void*>(&fill_world8),
    reinterpret_cast<void*>(&fill_world16),
    reinterpret_cast<void*>(&fill_world_float),
    reinterpret_cast<void*>(&premultiply_world8),
    reinterpret_cast<void*>(&premultiply_color8),
    reinterpret_cast<void*>(&premultiply_color16),
    reinterpret_cast<void*>(&premultiply_color_float)};

static_assert(sizeof(Iterate8Suite2) == 5 * sizeof(void*));
static_assert(offsetof(Iterate8Suite2, iterate_lut) == 2 * sizeof(void*));
static_assert(offsetof(Iterate8Suite2, iterate_generic) == 4 * sizeof(void*));
static_assert(sizeof(Iterate16Suite2) == 3 * sizeof(void*));
static_assert(sizeof(IterateFloatSuite2) == 3 * sizeof(void*));
static_assert(sizeof(PfBatchSamplingSuite1) == 4 * sizeof(void*));
static_assert(offsetof(PfBatchSamplingSuite1, get_batch_func) == 2 * sizeof(void*));

namespace {

constexpr std::size_t kMaxLiveEffectSequences = 64;
struct LiveEffectSequence {
  void* effect_ref{};
  PfConstHandle sequence_handle{};
  uint64_t generation{};
};
std::mutex g_effect_sequence_mutex;
std::vector<LiveEffectSequence> g_live_effect_sequences;
uint64_t g_effect_sequence_generation{};
uint64_t g_effect_sequence_publication_count{};
uint64_t g_effect_sequence_invalidation_count{};

int32_t __cdecl get_effect_sequence_data(
    void* effect_ref, PfConstHandle* sequence_handle) {
  if (!sequence_handle) return kPfErrBadCallbackParam;
  *sequence_handle = nullptr;
  if (!effect_ref) return kPfErrBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  const auto found = std::find_if(
      g_live_effect_sequences.begin(), g_live_effect_sequences.end(),
      [effect_ref](const auto& entry) { return entry.effect_ref == effect_ref; });
  if (found == g_live_effect_sequences.end() || !found->sequence_handle)
    return kPfErrBadCallbackParam;
  *sequence_handle = found->sequence_handle;
  return 0;
}

}  // namespace

PfEffectSequenceDataSuite1 g_effect_sequence_data_suite1{
    &get_effect_sequence_data};
static_assert(sizeof(PfEffectSequenceDataSuite1) == sizeof(void*));

void invalidate_effect_sequence(void* effect_ref) {
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  const auto old_size = g_live_effect_sequences.size();
  g_live_effect_sequences.erase(
      std::remove_if(g_live_effect_sequences.begin(), g_live_effect_sequences.end(),
          [effect_ref](const auto& entry) { return entry.effect_ref == effect_ref; }),
      g_live_effect_sequences.end());
  if (g_live_effect_sequences.size() != old_size)
    ++g_effect_sequence_invalidation_count;
}

bool publish_effect_sequence(void* effect_ref, void* sequence_handle) {
  if (!effect_ref || !sequence_handle) return false;
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  auto found = std::find_if(
      g_live_effect_sequences.begin(), g_live_effect_sequences.end(),
      [effect_ref](const auto& entry) { return entry.effect_ref == effect_ref; });
  if (found == g_live_effect_sequences.end()) {
    if (g_live_effect_sequences.size() >= kMaxLiveEffectSequences) return false;
    g_live_effect_sequences.push_back(
        {effect_ref, reinterpret_cast<PfConstHandle>(sequence_handle),
         ++g_effect_sequence_generation});
  } else {
    found->sequence_handle = reinterpret_cast<PfConstHandle>(sequence_handle);
    found->generation = ++g_effect_sequence_generation;
  }
  ++g_effect_sequence_publication_count;
  return true;
}

uint64_t effect_sequence_publications() {
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  return g_effect_sequence_publication_count;
}

uint64_t effect_sequence_invalidations() {
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  return g_effect_sequence_invalidation_count;
}

std::size_t live_effect_sequence_count() {
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  return g_live_effect_sequences.size();
}

void configure_pf_host_context(const PfHostContext& context) {
  g_pf_host = context;
  g_pf_host_configured = context.hooks.resolve_world && context.hooks.pixel_format &&
      context.hooks.set_pixel_format && context.hooks.acquire_suite &&
      context.hooks.release_suite && context.hooks.resolve_dispatch_world_format &&
      context.effect_ref && context.batch_sampling_suite &&
      context.transform_telemetry.calls && context.transform_telemetry.last_x &&
      context.transform_telemetry.last_y && context.transform_telemetry.last_opacity;
}

bool pf_host_context_configured() { return g_pf_host_configured; }

int32_t fill_world_typed(int32_t pixel_bytes, const void* color,
                         const LegacyRect* requested, void* world) {
  unsigned char* pixels{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(world, pixel_bytes, pixels, rowbytes, width, height)) return 4;
  const std::array<unsigned char, 16> transparent_black{};
  if (!color) color = transparent_black.data();
  LegacyRect bounds{};
  if (!normalize_legacy_rect(requested, width, height, bounds)) return 4;
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
      deep[channel] = static_cast<uint16_t>(static_cast<const uint8_t*>(color)[channel] * 128u);
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
  if (!source_world || !destination_world || !matte) return 4;
  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if (!resolve_world(source_world, pixel_bytes, source, source_rowbytes,
                           source_width, source_height) ||
      !resolve_world(destination_world, pixel_bytes, destination, destination_rowbytes,
                           destination_width, destination_height) ||
      source_width != destination_width || source_height != destination_height) return 4;
  const std::size_t packed_row = static_cast<std::size_t>(source_width) * pixel_bytes;
  if (packed_row > SIZE_MAX / static_cast<std::size_t>(source_height)) return 4;
  std::vector<unsigned char> snapshot;
  try {
    snapshot.resize(packed_row * static_cast<std::size_t>(source_height));
  } catch (...) {
    return 4;
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
  ok = ok && fill8(nullptr, color8.data(), &invalid_rect, &world8) == 4 &&
      guarded8 == before_error && fill8(nullptr, color8.data(), nullptr, nullptr) == 4;

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
      premultiply8(nullptr, nullptr, color8.data(), 1, &world8) == 4 &&
      premultiply16(nullptr, &world16, nullptr, 1, &world16) == 4;
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
  if (!active_format) return 4;
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
      source_width != destination_width || source_height != destination_height) return 4;
  LegacyRect bounds{};
  if (!normalize_legacy_rect(requested, source_width, source_height, bounds)) return 4;
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
    if (std::abs(divisors[channel]) < 1e-12) return 4;
  }
  const uint64_t packed_rowbytes = static_cast<uint64_t>(source_width) * pixel_bytes;
  const uint64_t source_bytes = packed_rowbytes * source_height;
  if (!source_bytes || source_bytes > kMaxAsyncReceiptBytes) return 4;
  std::vector<unsigned char> snapshot;
  try {
    snapshot.resize(static_cast<std::size_t>(source_bytes));
  } catch (...) { return 4; }
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
  if (ratio < 0 || ratio > 65536) return 4;
  DispatchWorldFormat first_info{}, second_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world1, first_info) ||
      !resolve_dispatch_world_format(source_world2, second_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      first_info.pixel_format != second_info.pixel_format ||
      first_info.pixel_format != destination_info.pixel_format ||
      first_info.width != second_info.width || first_info.width != destination_info.width ||
      first_info.height != second_info.height || first_info.height != destination_info.height ||
      first_info.width > 4096 || first_info.height > 4096) return 4;
  const int32_t pixel_bytes = first_info.pixel_format == kPixelFormatArgb32 ? 4 :
      (first_info.pixel_format == kPixelFormatArgb64 ? 8 :
       (first_info.pixel_format == kPixelFormatArgb128 ? 16 : 0));
  if (!pixel_bytes || first_info.rowbytes < static_cast<int64_t>(first_info.width) * pixel_bytes ||
      second_info.rowbytes < static_cast<int64_t>(second_info.width) * pixel_bytes ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * pixel_bytes)
    return 4;
  const std::size_t packed_row = static_cast<std::size_t>(first_info.width) * pixel_bytes;
  std::vector<unsigned char> first_copy, second_copy;
  try {
    first_copy.resize(packed_row * first_info.height);
    second_copy.resize(packed_row * first_info.height);
  } catch (...) { return 4; }
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
    return 4;
  auto* source = static_cast<unsigned char*>(source_info.data);
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  LegacyRect src{}, dst{};
  if (!normalize_legacy_rect(source_rect, source_info.width, source_info.height, src) ||
      !normalize_legacy_rect(destination_rect, destination_info.width, destination_info.height, dst))
    return 4;
  const int32_t copy_width = std::min(src.right - src.left, dst.right - dst.left);
  const int32_t copy_height = std::min(src.bottom - src.top, dst.bottom - dst.top);
  if (copy_width <= 0 || copy_height <= 0) return 0;
  const std::size_t row_size = static_cast<std::size_t>(copy_width) * pixel_bytes;
  std::vector<unsigned char> temporary(row_size * copy_height);
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

int32_t __cdecl transform_world(void* effect_ref, int32_t quality, uint32_t mode_flags,
                                int32_t field,
                                const void* source_world, const void* composite_mode,
                                const void* mask_world, const void* matrices,
                                int32_t matrix_count, uint8_t source_to_destination,
                                const LegacyRect* destination_rect, void* destination_world) {
  if (!effect_ref || !source_world || !composite_mode || !matrices ||
      matrix_count != 1 || source_to_destination > 1 || quality < 0 || quality > 1 ||
      mode_flags > 1 || field < 0 || field > 2) return 4;
  int32_t transfer_mode{};
  uint8_t opacity{}, rgb_only{};
  uint16_t opacity16{};
  std::memcpy(&transfer_mode, composite_mode, sizeof(transfer_mode));
  std::memcpy(&opacity, static_cast<const std::byte*>(composite_mode) + 8, sizeof(opacity));
  std::memcpy(&rgb_only, static_cast<const std::byte*>(composite_mode) + 9, sizeof(rgb_only));
  std::memcpy(&opacity16, static_cast<const std::byte*>(composite_mode) + 10, sizeof(opacity16));
  if (transfer_mode != 0 || rgb_only > 1 || opacity16 > 32768) return 4;
  std::array<double, 9> matrix{};
  std::memcpy(matrix.data(), matrices, sizeof(matrix));
  if (!std::all_of(matrix.begin(), matrix.end(),
                   [](double value) { return std::isfinite(value); })) return 4;
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format) return 4;
  const int32_t pixel_bytes = source_info.pixel_format == kPixelFormatArgb32 ? 4 :
      (source_info.pixel_format == kPixelFormatArgb64 ? 8 :
       (source_info.pixel_format == kPixelFormatArgb128 ? 16 : 0));
  if (!pixel_bytes || source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * pixel_bytes ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * pixel_bytes)
    return 4;
  const double determinant = matrix[0] * (matrix[4] * matrix[8] - matrix[5] * matrix[7]) -
      matrix[1] * (matrix[3] * matrix[8] - matrix[5] * matrix[6]) +
      matrix[2] * (matrix[3] * matrix[7] - matrix[4] * matrix[6]);
  if (source_to_destination && std::abs(determinant) < 1e-12) return 4;
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
                             destination_info.height, bounds)) return 4;
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
  } catch (...) { return kPfErrBadCallbackParam; }
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
  ++*g_pf_host.transform_telemetry.calls;
  *g_pf_host.transform_telemetry.last_x = static_cast<int32_t>(std::lround(matrix[6]));
  *g_pf_host.transform_telemetry.last_y = static_cast<int32_t>(std::lround(matrix[7]));
  *g_pf_host.transform_telemetry.last_opacity = opacity;
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
      !resolve_dispatch_world_format(destination_world, destination_info)) return 4;
  LegacyRect source_bounds{}, destination_bounds{};
  if (!normalize_legacy_rect(source_rect, source_info.width, source_info.height, source_bounds) ||
      !normalize_legacy_rect(destination_rect, destination_info.width, destination_info.height,
                             destination_bounds) ||
      source_bounds.right - source_bounds.left !=
          destination_bounds.right - destination_bounds.left ||
      source_bounds.bottom - source_bounds.top !=
          destination_bounds.bottom - destination_bounds.top) return 4;
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

int32_t iterate_world_typed(void* in_data, int32_t progress_base, int32_t progress_final,
                            int32_t pixel_bytes, void* source_world, const LegacyRect* area,
                            void* refcon, IteratePixelRaw pixel_function,
                            void* destination_world) {
  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if (!pixel_function ||
      !resolve_world(source_world, pixel_bytes, source, source_rowbytes,
                           source_width, source_height) ||
      !resolve_world(destination_world, pixel_bytes, destination,
                           destination_rowbytes, destination_width, destination_height)) return 4;
  LegacyRect bounds{};
  if (!normalize_legacy_rect(area, std::min(source_width, destination_width),
                             std::min(source_height, destination_height), bounds)) return 4;
  IterateAbortCallback abort_callback{};
  IterateProgressCallback progress_callback{};
  void* effect_ref{};
  if (in_data) {
    const auto* bytes = static_cast<const std::byte*>(in_data);
    std::memcpy(&abort_callback, bytes + 24, sizeof(abort_callback));
    std::memcpy(&progress_callback, bytes + 32, sizeof(progress_callback));
    std::memcpy(&effect_ref, bytes + kInEffectRef, sizeof(effect_ref));
  }
  const int32_t rows = bounds.bottom - bounds.top;
  for (int32_t y = bounds.top; y < bounds.bottom; ++y) {
    for (int32_t x = bounds.left; x < bounds.right; ++x) {
      void* source_pixel = source + static_cast<std::size_t>(y) * source_rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      void* destination_pixel = destination + static_cast<std::size_t>(y) * destination_rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      const int32_t error = pixel_function(refcon, x, y, source_pixel, destination_pixel);
      if (error != 0) return error;
    }
    const int32_t completed_rows = y - bounds.top + 1;
    const bool reverse_progress = progress_final < progress_base;
    const int64_t progress_span = reverse_progress
        ? static_cast<int64_t>(progress_base) - progress_final
        : static_cast<int64_t>(progress_final) - progress_base;
    if (progress_span > std::numeric_limits<int32_t>::max()) return 4;
    const int32_t current = static_cast<int32_t>(reverse_progress
        ? progress_span * completed_rows / rows
        : static_cast<int64_t>(progress_base) + progress_span * completed_rows / rows);
    const int32_t callback_total = reverse_progress
        ? static_cast<int32_t>(progress_span)
        : progress_final;
    if (progress_callback) {
      const int32_t error = progress_callback(effect_ref, current, callback_total);
      if (error != 0) return error;
    }
    if (completed_rows < rows && abort_callback) {
      const int32_t error = abort_callback(effect_ref);
      if (error != 0) return error;
    }
  }
  return 0;
}

int32_t __cdecl iterate_world8(void* in_data, int32_t progress_base, int32_t progress_final,
                               void* source_world, const LegacyRect* area, void* refcon,
                               IteratePixel8 pixel_function, void* destination_world) {
  return iterate_world_typed(in_data, progress_base, progress_final, 4, source_world, area,
      refcon, reinterpret_cast<IteratePixelRaw>(pixel_function), destination_world);
}

int32_t __cdecl iterate_world16(void* in_data, int32_t progress_base, int32_t progress_final,
                                void* source_world,
                                const LegacyRect* area, void* refcon,
                                IteratePixelRaw pixel_function, void* destination_world) {
  return iterate_world_typed(in_data, progress_base, progress_final, 8, source_world, area,
      refcon, pixel_function, destination_world);
}

int32_t __cdecl iterate_world_float(void* in_data, int32_t progress_base, int32_t progress_final,
                                    void* source_world,
                                    const LegacyRect* area, void* refcon,
                                    IteratePixelRaw pixel_function, void* destination_world) {
  return iterate_world_typed(in_data, progress_base, progress_final, 16, source_world, area,
      refcon, pixel_function, destination_world);
}

int32_t iterate_origin_typed(int32_t pixel_bytes, void* source_world, const LegacyRect* area,
                             const void* origin, void* refcon, IteratePixelRaw pixel_function,
                             void* destination_world) {
  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if (!origin || !pixel_function ||
      !resolve_world(source_world, pixel_bytes, source, source_rowbytes,
                           source_width, source_height) ||
      !resolve_world(destination_world, pixel_bytes, destination, destination_rowbytes,
                           destination_width, destination_height)) return 4;
  LegacyRect bounds{};
  if (!normalize_legacy_rect(area, destination_width, destination_height, bounds)) return 4;
  int16_t origin_x{}, origin_y{};
  std::memcpy(&origin_x, origin, sizeof(origin_x));
  std::memcpy(&origin_y, static_cast<const std::byte*>(origin) + 2, sizeof(origin_y));
  std::array<unsigned char, 16> zero{};
  for (int32_t y = bounds.top; y < bounds.bottom; ++y) {
    for (int32_t x = bounds.left; x < bounds.right; ++x) {
      void* input = zero.data();
      if (x >= 0 && x < source_width && y >= 0 && y < source_height)
        input = source + static_cast<std::size_t>(y) * source_rowbytes +
            static_cast<std::size_t>(x) * pixel_bytes;
      void* output = destination + static_cast<std::size_t>(y) * destination_rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      const int32_t error = pixel_function(refcon, x + origin_x, y + origin_y, input, output);
      if (error != 0) return error;
    }
  }
  return 0;
}

int32_t __cdecl iterate_origin8(void*, int32_t, int32_t, void* source_world,
                                const LegacyRect* area, const void* origin, void* refcon,
                                IteratePixelRaw pixel_function, void* destination_world) {
  return iterate_origin_typed(4, source_world, area, origin, refcon, pixel_function,
                              destination_world);
}
int32_t __cdecl iterate_origin16(void*, int32_t, int32_t, void* source_world,
                                 const LegacyRect* area, const void* origin, void* refcon,
                                 IteratePixelRaw pixel_function, void* destination_world) {
  return iterate_origin_typed(8, source_world, area, origin, refcon, pixel_function,
                              destination_world);
}
int32_t __cdecl iterate_origin_float(void*, int32_t, int32_t, void* source_world,
                                     const LegacyRect* area, const void* origin, void* refcon,
                                     IteratePixelRaw pixel_function, void* destination_world) {
  return iterate_origin_typed(16, source_world, area, origin, refcon, pixel_function,
                              destination_world);
}

int32_t __cdecl iterate_lut8(void*, int32_t, int32_t, void* source_world,
                             const LegacyRect* area, unsigned char* alpha_lut,
                             unsigned char* red_lut, unsigned char* green_lut,
                             unsigned char* blue_lut, void* destination_world) {
  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if (!bounded_argb8_world(source_world, source, source_rowbytes, source_width, source_height) ||
      !bounded_argb8_world(destination_world, destination, destination_rowbytes,
                           destination_width, destination_height)) return 4;
  LegacyRect bounds{};
  if (!normalize_legacy_rect(area, std::min(source_width, destination_width),
                             std::min(source_height, destination_height), bounds)) return 4;
  unsigned char* tables[4]{alpha_lut, red_lut, green_lut, blue_lut};
  for (int32_t y = bounds.top; y < bounds.bottom; ++y) {
    const auto* source_row = source + static_cast<std::size_t>(y) * source_rowbytes;
    auto* destination_row = destination + static_cast<std::size_t>(y) * destination_rowbytes;
    for (int32_t x = bounds.left; x < bounds.right; ++x) {
      for (int channel = 0; channel < 4; ++channel) {
        const unsigned char value = source_row[static_cast<std::size_t>(x) * 4 + channel];
        destination_row[static_cast<std::size_t>(x) * 4 + channel] =
            tables[channel] ? tables[channel][value] : value;
      }
    }
  }
  return 0;
}

int32_t __cdecl iterate_origin_non_clip8(void* in_data, int32_t progress_base,
                                          int32_t progress_final, void* source_world,
                                          const LegacyRect* area, const void* origin,
                                          void* refcon, IteratePixelRaw pixel_function,
                                          void* destination_world) {
  return iterate_origin_typed(4, source_world, area, origin, refcon, pixel_function,
                              destination_world);
}

int32_t __cdecl iterate_origin_non_clip16(void* in_data, int32_t progress_base,
                                           int32_t progress_final, void* source_world,
                                           const LegacyRect* area, const void* origin,
                                           void* refcon, IteratePixelRaw pixel_function,
                                           void* destination_world) {
  return iterate_origin_typed(8, source_world, area, origin, refcon, pixel_function,
                              destination_world);
}

int32_t __cdecl iterate_origin_non_clip_float(void* in_data, int32_t progress_base,
                                               int32_t progress_final, void* source_world,
                                               const LegacyRect* area, const void* origin,
                                               void* refcon, IteratePixelRaw pixel_function,
                                               void* destination_world) {
  return iterate_origin_typed(16, source_world, area, origin, refcon, pixel_function,
                              destination_world);
}

int32_t __cdecl iterate_generic(int32_t iterations, void* refcon,
                                 IterateGenericCallback callback) {
  constexpr int32_t kOncePerProcessor = -1;
  constexpr int32_t kMaxIterations = 16'777'216;
  if (!callback || (iterations != kOncePerProcessor &&
                    (iterations <= 0 || iterations > kMaxIterations))) return 4;
  const int32_t actual_iterations = iterations == kOncePerProcessor ? 1 : iterations;
  for (int32_t index = 0; index < actual_iterations; ++index) {
    const int32_t error = callback(refcon, 0, index, actual_iterations);
    if (error != 0) return error;
  }
  return 0;
}

struct IterateInteractionTestState {
  int32_t pixel_calls{};
  int32_t abort_calls{};
  int32_t cancel_on_abort{};
  int32_t pixel_error_call{};
  std::vector<int32_t> progress;
};

IterateInteractionTestState* g_iterate_interaction_test{};

int32_t __cdecl iterate_test_abort(void*) {
  auto& state = *g_iterate_interaction_test;
  ++state.abort_calls;
  return state.abort_calls == state.cancel_on_abort ? 1 : 0;
}

int32_t __cdecl iterate_test_progress(void*, int32_t current, int32_t total) {
  if (total != 14) return 92;
  g_iterate_interaction_test->progress.push_back(current);
  return 0;
}

int32_t __cdecl iterate_test_pixel(void* opaque, int32_t, int32_t, void* input, void* output) {
  auto& state = *static_cast<IterateInteractionTestState*>(opaque);
  ++state.pixel_calls;
  if (state.pixel_calls == state.pixel_error_call) return 73;
  *static_cast<unsigned char*>(output) = *static_cast<unsigned char*>(input);
  return 0;
}

bool verify_iterate_suites() {
  std::array<unsigned char, 8> source{{10, 20, 30, 40, 50, 60, 70, 80}};
  std::array<unsigned char, 8> destination{};
  LocalEffectWorld source_world{}, destination_world{};
  source_world.data = source.data();
  source_world.rowbytes = 8;
  source_world.width = 2;
  source_world.height = 1;
  destination_world.data = destination.data();
  destination_world.rowbytes = 8;
  destination_world.width = 2;
  destination_world.height = 1;
  std::array<unsigned char, 256> invert{};
  for (std::size_t index = 0; index < invert.size(); ++index)
    invert[index] = static_cast<unsigned char>(255 - index);
  if (iterate_lut8(nullptr, 0, 1, &source_world, nullptr, nullptr, invert.data(),
                   nullptr, nullptr, &destination_world) != 0 ||
      destination != std::array<unsigned char, 8>{{10, 235, 30, 40, 50, 195, 70, 80}})
    return false;

  struct GenericState { int32_t calls{}, expected{}; } state{};
  const auto generic_callback = [](void* opaque, int32_t thread_index, int32_t index,
                                   int32_t iterations) -> int32_t {
    auto& value = *static_cast<GenericState*>(opaque);
    if (thread_index != 0 || index != value.calls || iterations != value.expected) return 91;
    ++value.calls;
    return 0;
  };
  state.expected = 3;
  if (iterate_generic(3, &state, generic_callback) != 0 || state.calls != 3 ||
      iterate_generic(0, &state, generic_callback) == 0 ||
      iterate_generic(16'777'217, &state, generic_callback) == 0 ||
      iterate_generic(1, &state, nullptr) == 0) return false;
  state = {};
  state.expected = 1;
  if (iterate_generic(-1, &state, generic_callback) != 0 || state.calls != 1) return false;

  std::array<unsigned char, 12> wider_destination{};
  destination_world.data = wider_destination.data();
  destination_world.rowbytes = 12;
  destination_world.width = 3;
  struct PixelState { int32_t calls{}; bool saw_zero{}; } pixel_state{};
  const auto pixel_callback = [](void* opaque, int32_t, int32_t, void* input,
                                 void* output) -> int32_t {
    auto& value = *static_cast<PixelState*>(opaque);
    const auto* pixel = static_cast<unsigned char*>(input);
    value.saw_zero = value.saw_zero ||
        (pixel[0] == 0 && pixel[1] == 0 && pixel[2] == 0 && pixel[3] == 0);
    std::memcpy(output, input, 4);
    ++value.calls;
    return 0;
  };
  const std::array<int16_t, 2> origin{{0, 0}};
  if (iterate_origin_non_clip8(nullptr, 0, 1, &source_world, nullptr, origin.data(),
                               &pixel_state, pixel_callback, &destination_world) != 0 ||
      pixel_state.calls != 3 || !pixel_state.saw_zero) return false;
  const auto error_callback = [](void*, int32_t, int32_t, void*, void*) -> int32_t { return 73; };
  if (iterate_origin_non_clip8(nullptr, 0, 1, &source_world, nullptr, origin.data(),
                              nullptr, error_callback, &destination_world) != 73) return false;

  alignas(8) std::array<std::byte, kInSize> input{};
  write(input, 24, &iterate_test_abort);
  write(input, 32, &iterate_test_progress);
  write<void*>(input, kInEffectRef, g_pf_host.effect_ref);
  const LegacyRect four_rows{0, 0, 1, 4};
  for (const int32_t pixel_bytes : {4, 8, 16}) {
    std::array<unsigned char, 64> typed_source{}, typed_destination{};
    LocalEffectWorld typed_source_world{}, typed_destination_world{};
    typed_source_world.data = typed_source.data();
    typed_source_world.rowbytes = pixel_bytes;
    typed_source_world.width = 1;
    typed_source_world.height = 4;
    typed_source_world.world_flags = pixel_bytes == 4 ? 0 : 1;
    typed_destination_world = typed_source_world;
    typed_destination_world.data = typed_destination.data();
    IterateInteractionTestState interaction{};
    g_iterate_interaction_test = &interaction;
    const int32_t result = iterate_world_typed(input.data(), 10, 14, pixel_bytes,
        &typed_source_world, &four_rows, &interaction, &iterate_test_pixel,
        &typed_destination_world);
    if (result != 0 || interaction.pixel_calls != 4 || interaction.abort_calls != 3 ||
        interaction.progress != std::vector<int32_t>({11, 12, 13, 14})) return false;

    interaction = {};
    interaction.cancel_on_abort = 2;
    if (iterate_world_typed(input.data(), 10, 14, pixel_bytes, &typed_source_world, &four_rows,
            &interaction, &iterate_test_pixel, &typed_destination_world) != 1 ||
        interaction.pixel_calls != 2 || interaction.abort_calls != 2 ||
        interaction.progress != std::vector<int32_t>({11, 12})) return false;

    interaction = {};
    interaction.cancel_on_abort = 2;
    interaction.pixel_error_call = 2;
    if (iterate_world_typed(input.data(), 10, 14, pixel_bytes, &typed_source_world, &four_rows,
            &interaction, &iterate_test_pixel, &typed_destination_world) != 73 ||
        interaction.pixel_calls != 2 || interaction.abort_calls != 1 ||
        interaction.progress != std::vector<int32_t>({11})) return false;
  }
  g_iterate_interaction_test = nullptr;
  return true;
}

int32_t subpixel_sample_typed(int32_t pixel_bytes, void* effect_ref, int32_t fixed_x,
                              int32_t fixed_y, const void* sampling_params,
                              void* destination_pixel) {
  if (!effect_ref || !sampling_params || !destination_pixel) return 4;
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

int32_t nearest_sample_typed(int32_t pixel_bytes, void* effect_ref, int32_t fixed_x,
                             int32_t fixed_y, const void* sampling_params,
                             void* destination_pixel) {
  if (!effect_ref || !sampling_params || !destination_pixel) return 4;
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

int32_t area_sample_typed(int32_t pixel_bytes, void* effect_ref, int32_t fixed_x,
                          int32_t fixed_y, const void* sampling_params,
                          void* destination_pixel) {
  if (!effect_ref || !sampling_params || !destination_pixel) return 4;
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
  if (effect_ref != g_pf_host.effect_ref || !sampling_params || (quality != 0 && quality != 1))
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
  if (effect_ref != g_pf_host.effect_ref || !sampling_params) return kPfBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_legacy_sampling_mutex);
  const auto found = g_legacy_sampling_sessions.find(sampling_params);
  if (found == g_legacy_sampling_sessions.end() || found->second.quality != quality ||
      found->second.mode_flags != mode_flags ||
      found->second.thread_id != GetCurrentThreadId()) return kPfBadCallbackParam;
  g_legacy_sampling_sessions.erase(found);
  return 0;
}

int32_t __cdecl unsupported_batch_sample_func(void* effect_ref, int32_t quality,
                                               uint32_t mode_flags,
                                               const void* sampling_params,
                                               void** batch) {
  if (!batch) return kPfBadCallbackParam;
  *batch = nullptr;
  if (effect_ref != g_pf_host.effect_ref || !sampling_params || (quality != 0 && quality != 1))
    return kPfBadCallbackParam;
  (void)mode_flags;
  return 4;
}

bool verify_pf_batch_sampling_suite() {
  const void* acquired{};
  if (acquire_host_suite("PF Batch Sampling Suite", 1, &acquired) != 0 ||
      acquired != g_pf_host.batch_sampling_suite)
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
  bool passed = suite.begin_sampling(g_pf_host.effect_ref, 1, 0x12, params.data()) == 0 &&
      suite.begin_sampling(g_pf_host.effect_ref, 1, 0x12, params.data()) == kPfBadCallbackParam &&
      suite.end_sampling(g_pf_host.effect_ref, 0, 0x12, params.data()) == kPfBadCallbackParam &&
      suite.end_sampling(g_pf_host.effect_ref, 1, 0x13, params.data()) == kPfBadCallbackParam;

  int32_t cross_thread_result{};
  std::thread foreign_thread([&] {
    cross_thread_result = suite.end_sampling(g_pf_host.effect_ref, 1, 0x12, params.data());
  });
  foreign_thread.join();
  passed = passed && cross_thread_result == kPfBadCallbackParam &&
      suite.end_sampling(g_pf_host.effect_ref, 1, 0x12, params.data()) == 0 &&
      suite.end_sampling(g_pf_host.effect_ref, 1, 0x12, params.data()) == kPfBadCallbackParam &&
      suite.begin_sampling(nullptr, 1, 0, params.data()) == kPfBadCallbackParam &&
      suite.begin_sampling(g_pf_host.effect_ref, 2, 0, params.data()) == kPfBadCallbackParam &&
      suite.begin_sampling(g_pf_host.effect_ref, 1, 0, nullptr) == kPfBadCallbackParam;

  void* batch = reinterpret_cast<void*>(0x1234);
  passed = passed && suite.get_batch_func(g_pf_host.effect_ref, 1, 0, params.data(), &batch) == 4 &&
      batch == nullptr;
  batch = reinterpret_cast<void*>(0x5678);
  passed = passed && suite.get_batch_func16(g_pf_host.effect_ref, 0, 0, params.data(), &batch) == 4 &&
      batch == nullptr;
  batch = reinterpret_cast<void*>(0x9abc);
  passed = passed &&
      suite.get_batch_func(nullptr, 1, 0, params.data(), &batch) == kPfBadCallbackParam &&
      batch == nullptr &&
      suite.get_batch_func(g_pf_host.effect_ref, 1, 0, params.data(), nullptr) == kPfBadCallbackParam;

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

