#include "worker_pf_suites_internal.hpp"
#include "worker_callback_diagnostics.hpp"
#include "worker_extended_diag.hpp"
#include "worker_pf_sampling_runtime.hpp"
#include "worker_world_registry.hpp"
#include "worker_world_safety.hpp"

#ifdef _WIN32
#include <windows.h>
#endif

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <iostream>
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

constexpr int32_t kPfErrBadCallbackParam = 516;
constexpr int32_t kPixelFormatArgb32 = 1650946657;
using aexcompat::world_registry::kPixelFormatArgb64;
using aexcompat::world_registry::kPixelFormatArgb128;
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

int32_t finish_callback(aexcompat::callback_diagnostics::Callback callback, int32_t result,
                        aexcompat::callback_diagnostics::Reason reason =
                            aexcompat::callback_diagnostics::Reason::None) {
  aexcompat::callback_diagnostics::record(callback, result, reason);
  if (aexcompat::l2_detail::extended_diag_enabled()) {
    std::cerr << "extended_diag:"
              << aexcompat::callback_diagnostics::CALLBACK_NAMES[
                     static_cast<std::size_t>(callback)]
              << " -> " << result;
    if (result != 0)
      std::cerr << " ("
                << aexcompat::callback_diagnostics::REASON_NAMES[
                       static_cast<std::size_t>(reason)]
                << ')';
    std::cerr << '\n' << std::flush;
  }
  return result;
}

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
  if (!suite) return kPfErrBadCallbackParam;
  *suite = nullptr;
  return g_pf_host_configured && g_pf_host.hooks.acquire_suite
      ? g_pf_host.hooks.acquire_suite(name, version, suite) : kPfErrBadCallbackParam;
}

int32_t release_host_suite(const char* name, int32_t version) {
  return g_pf_host_configured && g_pf_host.hooks.release_suite
      ? g_pf_host.hooks.release_suite(name, version) : kPfErrBadCallbackParam;
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
static_assert(sizeof(Iterate8Suite2) == 5 * sizeof(void*));
static_assert(offsetof(Iterate8Suite2, iterate_lut) == 2 * sizeof(void*));
static_assert(offsetof(Iterate8Suite2, iterate_generic) == 4 * sizeof(void*));
static_assert(sizeof(Iterate16Suite2) == 3 * sizeof(void*));
static_assert(sizeof(IterateFloatSuite2) == 3 * sizeof(void*));

void configure_pf_host_context(const PfHostContext& context) {
  g_pf_host = context;
  configure_pf_sampling_runtime({context.hooks.resolve_world,
      context.hooks.acquire_suite, context.hooks.release_suite,
      context.effect_ref, context.batch_sampling_suite});
  g_pf_host_configured = context.hooks.resolve_world && context.hooks.pixel_format &&
      context.hooks.set_pixel_format && context.hooks.acquire_suite &&
      context.hooks.release_suite && context.hooks.resolve_dispatch_world_format &&
      context.effect_ref && context.batch_sampling_suite;
}

bool pf_host_context_configured() { return g_pf_host_configured; }

int32_t iterate_world_typed(void* in_data, int32_t progress_base, int32_t progress_final,
                            int32_t pixel_bytes, void* source_world, const LegacyRect* area,
                            void* refcon, IteratePixelRaw pixel_function,
                            void* destination_world) {
  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  // A null source world is the documented "iterate over the destination only"
  // mode (the SDK lets the src world be NULL; effects that generate or transform
  // in place pass NULL). Resolve the source only when one was supplied;
  // otherwise bound the walk to the destination and alias each callback's input
  // pixel to the destination pixel it is processing.
  const bool has_source = source_world != nullptr;
  using aexcompat::callback_diagnostics::Callback;
  using aexcompat::callback_diagnostics::Reason;
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:iterate_args base=" << progress_base
              << " final=" << progress_final << " bytes=" << pixel_bytes
              << " src=" << (has_source ? 1 : 0) << "\n" << std::flush;
  // On a resolve_world failure the close report only records "missing_world",
  // which does not say which side failed or why bounded_typed_world rejected it
  // (issue #1035). Dump the rejected world's raw fields under extended diag so a
  // trace can tell a depth/rowbytes mismatch from an unregistered world without
  // disassembly. Env-gated, integer-only, path-free.
  const auto diag_unresolved = [pixel_bytes](const char* side, void* world) {
    if (!aexcompat::l2_detail::extended_diag_enabled()) return;
    int32_t flags{}, rowbytes{}, width{}, height{};
    void* pixels{};
    if (world) {
      auto* bytes = static_cast<const std::byte*>(world);
      std::memcpy(&flags, bytes + 16, sizeof(flags));
      std::memcpy(&pixels, bytes + 24, sizeof(pixels));
      std::memcpy(&rowbytes, bytes + 32, sizeof(rowbytes));
      std::memcpy(&width, bytes + 36, sizeof(width));
      std::memcpy(&height, bytes + 40, sizeof(height));
    }
    std::cerr << "extended_diag:iterate_world_unresolved side=" << side
              << " world=" << (world ? 1 : 0) << " pixels=" << (pixels ? 1 : 0)
              << " w=" << width << " h=" << height << " rowbytes=" << rowbytes
              << " flags=" << flags << " pixel_bytes=" << pixel_bytes << "\n"
              << std::flush;
  };
  if (!pixel_function)
    return finish_callback(Callback::Iterate, kPfErrBadCallbackParam,
                           Reason::InvalidArguments);
  if (has_source && !resolve_world(source_world, pixel_bytes, source, source_rowbytes,
                                   source_width, source_height)) {
    diag_unresolved("src", source_world);
    return finish_callback(Callback::Iterate, kPfErrBadCallbackParam,
                           Reason::MissingWorld);
  }
  if (!resolve_world(destination_world, pixel_bytes, destination,
                     destination_rowbytes, destination_width, destination_height)) {
    diag_unresolved("dst", destination_world);
    return finish_callback(Callback::Iterate, kPfErrBadCallbackParam,
                           Reason::MissingWorld);
  }
  const int32_t bound_width = has_source
      ? std::min(source_width, destination_width) : destination_width;
  const int32_t bound_height = has_source
      ? std::min(source_height, destination_height) : destination_height;
  // An area outside the walkable box is clipped to it, not refused (issue
  // #1034): PowerPin hands the full output extent with a smaller intermediate
  // world, Slant an extent one column past its source, and both die on a
  // refusal that AE evidently does not make. Clipping the source walk is also
  // what the utility table's own vocabulary implies the default is - the SDK
  // names a separate `iterate_origin_non_clip_src` slot for not doing it. An
  // empty or inverted intersection writes nothing and succeeds, the same
  // answer PF_COPY gives a wholly-outside rectangle (issue #962); the row
  // loop can still tick progress/abort callbacks for a rect empty in x only,
  // which is what the pre-clip code did for an accepted empty rect too. The
  // walk stays inside both worlds, so the clip loses no bounds protection.
  const LegacyRect requested = area ? *area : LegacyRect{0, 0, bound_width, bound_height};
  LegacyRect bounds{};
  bounds.left = std::max(requested.left, 0);
  bounds.top = std::max(requested.top, 0);
  bounds.right = std::min(requested.right, bound_width);
  bounds.bottom = std::min(requested.bottom, bound_height);
  if (bounds.right < bounds.left) bounds.right = bounds.left;
  if (bounds.bottom < bounds.top) bounds.bottom = bounds.top;
  if (aexcompat::l2_detail::extended_diag_enabled() &&
      (bounds.left != requested.left || bounds.top != requested.top ||
       bounds.right != requested.right || bounds.bottom != requested.bottom))
    std::cerr << "extended_diag:iterate area clipped l=" << requested.left
              << " t=" << requested.top << " r=" << requested.right
              << " b=" << requested.bottom << " bound=" << bound_width << "x"
              << bound_height << " bytes=" << pixel_bytes
              << " src=" << (has_source ? 1 : 0) << "\n" << std::flush;
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
      void* destination_pixel = destination + static_cast<std::size_t>(y) * destination_rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      // With no source world the SDK aliases the input pixel to the destination
      // pixel (in-place destination-only walk), so C/C++ effects that read `in`
      // still get a valid pixel instead of dereferencing null.
      void* source_pixel = has_source
          ? source + static_cast<std::size_t>(y) * source_rowbytes +
                static_cast<std::size_t>(x) * pixel_bytes
          : destination_pixel;
      const int32_t error = pixel_function(refcon, x, y, source_pixel, destination_pixel);
      if (error != 0)
        return finish_callback(Callback::Iterate, error, Reason::PixelCallback);
    }
    const int32_t completed_rows = y - bounds.top + 1;
    const bool reverse_progress = progress_final < progress_base;
    const int64_t progress_span = reverse_progress
        ? static_cast<int64_t>(progress_base) - progress_final
        : static_cast<int64_t>(progress_final) - progress_base;
    if (progress_span > std::numeric_limits<int32_t>::max())
      return finish_callback(Callback::Iterate, kPfErrBadCallbackParam,
                             Reason::InvalidArguments);
    const int32_t current = static_cast<int32_t>(reverse_progress
        ? progress_span * completed_rows / rows
        : static_cast<int64_t>(progress_base) + progress_span * completed_rows / rows);
    const int32_t callback_total = reverse_progress
        ? static_cast<int32_t>(progress_span)
        : progress_final;
    // progress_base == progress_final == 0 is the SDK's "no progress
    // contribution" idiom (Bevel_Edges and Drop_Shadow iterate that way), and
    // the callback here is the host's own report_progress read back out of
    // in_data. Synthesizing a (0, 0) report would trip that callback's own
    // total<=0 validation and fail the whole iterate with 4 (issue #1054), so
    // only report when there is a progress range to report. A plug-in calling
    // the callback directly still gets the full fail-closed validation.
    if (progress_callback && callback_total > 0) {
      const int32_t error = progress_callback(effect_ref, current, callback_total);
      if (error != 0)
        return finish_callback(Callback::Iterate, error, Reason::ProgressCallback);
    }
    if (completed_rows < rows && abort_callback) {
      const int32_t error = abort_callback(effect_ref);
      if (error != 0)
        return finish_callback(Callback::Iterate, error, Reason::AbortCallback);
    }
  }
  return finish_callback(Callback::Iterate, 0);
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
  // A null source world is legal (destination-only mode, issue #219). Leaving
  // source_width/source_height at 0 makes the in-bounds check below fall through
  // to the zero source pixel for every location, matching how out-of-source
  // locations are already handled.
  const bool has_source = source_world != nullptr;
  using aexcompat::callback_diagnostics::Callback;
  using aexcompat::callback_diagnostics::Reason;
  if (!origin || !pixel_function)
    return finish_callback(Callback::IterateOrigin, kPfErrBadCallbackParam,
                           Reason::InvalidArguments);
  if (has_source && !resolve_world(source_world, pixel_bytes, source, source_rowbytes,
                                   source_width, source_height))
    return finish_callback(Callback::IterateOrigin, kPfErrBadCallbackParam,
                           Reason::MissingWorld);
  if (!resolve_world(destination_world, pixel_bytes, destination, destination_rowbytes,
                     destination_width, destination_height))
    return finish_callback(Callback::IterateOrigin, kPfErrBadCallbackParam,
                           Reason::MissingWorld);
  LegacyRect bounds{};
  if (!normalize_legacy_rect(area, destination_width, destination_height, bounds))
    return finish_callback(Callback::IterateOrigin, kPfErrBadCallbackParam,
                           Reason::InvalidArea);
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
      if (error != 0)
        return finish_callback(Callback::IterateOrigin, error, Reason::PixelCallback);
    }
  }
  return finish_callback(Callback::IterateOrigin, 0);
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
                           destination_width, destination_height)) return kPfErrBadCallbackParam;
  LegacyRect bounds{};
  if (!normalize_legacy_rect(area, std::min(source_width, destination_width),
                             std::min(source_height, destination_height), bounds))
    return kPfErrBadCallbackParam;
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
                    (iterations <= 0 || iterations > kMaxIterations)))
    return finish_callback(aexcompat::callback_diagnostics::Callback::IterateGeneric,
                           kPfErrBadCallbackParam,
                           aexcompat::callback_diagnostics::Reason::InvalidArguments);
  const int32_t actual_iterations = iterations == kOncePerProcessor ? 1 : iterations;
  for (int32_t index = 0; index < actual_iterations; ++index) {
    const int32_t error = callback(refcon, 0, index, actual_iterations);
    if (error != 0)
      return finish_callback(aexcompat::callback_diagnostics::Callback::IterateGeneric,
                             error,
                             aexcompat::callback_diagnostics::Reason::CallbackError);
  }
  return finish_callback(aexcompat::callback_diagnostics::Callback::IterateGeneric, 0);
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

// Destination-only iteration (null source world): the host must alias the
// callback's input pixel to the destination pixel being processed.
int32_t __cdecl iterate_test_null_source_pixel(void* opaque, int32_t, int32_t,
                                               void* input, void* output) {
  auto& state = *static_cast<IterateInteractionTestState*>(opaque);
  ++state.pixel_calls;
  if (input != output) return 74;
  *static_cast<unsigned char*>(output) = 42;
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

  // An area past the walkable box is clipped, not refused (issue #1034):
  // the walk covers exactly the intersection, and a wholly-outside or
  // inverted rect walks nothing and succeeds, like PF_COPY (issue #962).
  // Shapes from the corpus: Slant's rect one column past the source,
  // PowerPin's full output extent against a smaller world.
  {
    IterateInteractionTestState clip{};
    const LegacyRect oversized{-3, -2, 9, 7};
    if (iterate_world_typed(nullptr, 0, 1, 4, &source_world, &oversized, &clip,
                            &iterate_test_pixel, &destination_world) != 0 ||
        clip.pixel_calls != 2) return false;
    clip = {};
    const LegacyRect shifted{1, 0, 3, 1};
    if (iterate_world_typed(nullptr, 0, 1, 4, &source_world, &shifted, &clip,
                            &iterate_test_pixel, &destination_world) != 0 ||
        clip.pixel_calls != 1) return false;
    clip = {};
    const LegacyRect outside{5, 5, 9, 9};
    const LegacyRect inverted{2, 1, 0, 0};
    if (iterate_world_typed(nullptr, 0, 1, 4, &source_world, &outside, &clip,
                            &iterate_test_pixel, &destination_world) != 0 ||
        iterate_world_typed(nullptr, 0, 1, 4, &source_world, &inverted, &clip,
                            &iterate_test_pixel, &destination_world) != 0 ||
        clip.pixel_calls != 0) return false;
  }

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
      iterate_generic(0, &state, generic_callback) != kPfErrBadCallbackParam ||
      iterate_generic(16'777'217, &state, generic_callback) != kPfErrBadCallbackParam ||
      iterate_generic(1, &state, nullptr) != kPfErrBadCallbackParam) return false;
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

  // A null source world walks the destination and supplies the zero source
  // pixel everywhere (issue #219), instead of failing with A_Err_ALLOC.
  PixelState null_origin_state{};
  if (iterate_origin_non_clip8(nullptr, 0, 1, nullptr, nullptr, origin.data(),
                               &null_origin_state, pixel_callback, &destination_world) != 0 ||
      null_origin_state.calls != 3 || !null_origin_state.saw_zero) return false;

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

    // progress_base == progress_final == 0 is the "no progress contribution"
    // idiom (Bevel_Edges, Drop_Shadow iterate that way): the host must not
    // synthesize a (0, 0) report that its own progress callback rejects as
    // total<=0, and the abort cadence stays unchanged (issue #1054).
    interaction = {};
    if (iterate_world_typed(input.data(), 0, 0, pixel_bytes, &typed_source_world, &four_rows,
            &interaction, &iterate_test_pixel, &typed_destination_world) != 0 ||
        interaction.pixel_calls != 4 || interaction.abort_calls != 3 ||
        !interaction.progress.empty()) return false;
  }

  // A float world that leaves the DEEP bit clear must still resolve for a float
  // iterate (issue #1035): AE builds the video-frame float worlds through its
  // own PPix suite, and those come back with world_flags 0x02000000 (bit 0
  // clear), the exact shape Cartoon's iterateFloat handed the host. The 8-vs-16
  // distinction still rides the DEEP bit, but float acceptance must not, or a
  // valid AE float world fails the callback with missing_world.
  for (const int32_t float_flags : {0, 0x02000000}) {
    std::array<unsigned char, 64> ae_source{}, ae_destination{};
    LocalEffectWorld ae_source_world{}, ae_destination_world{};
    ae_source_world.data = ae_source.data();
    ae_source_world.rowbytes = 16;
    ae_source_world.width = 1;
    ae_source_world.height = 4;
    ae_source_world.world_flags = float_flags;
    ae_destination_world = ae_source_world;
    ae_destination_world.data = ae_destination.data();
    IterateInteractionTestState interaction{};
    g_iterate_interaction_test = &interaction;
    if (iterate_world_typed(input.data(), 10, 14, 16, &ae_source_world, &four_rows,
            &interaction, &iterate_test_pixel, &ae_destination_world) != 0 ||
        interaction.pixel_calls != 4) return false;
  }

  // A null source world selects the documented destination-only walk (issue
  // #219): the host must bound the iteration to the destination, hand the
  // callback a null source pixel, and still report progress. Rejecting this
  // with A_Err_ALLOC surfaced as OutOfMemory in crate-based effects that
  // iterate in place.
  {
    std::array<unsigned char, 16> dst_only_pixels{};
    LocalEffectWorld dst_only_world{};
    dst_only_world.data = dst_only_pixels.data();
    dst_only_world.rowbytes = 4;
    dst_only_world.width = 1;
    dst_only_world.height = 4;
    dst_only_world.world_flags = 0;
    IterateInteractionTestState interaction{};
    g_iterate_interaction_test = &interaction;
    const LegacyRect all_rows{0, 0, 1, 4};
    if (iterate_world_typed(input.data(), 10, 14, 4, nullptr, &all_rows,
            &interaction, &iterate_test_null_source_pixel, &dst_only_world) != 0 ||
        interaction.pixel_calls != 4 || interaction.abort_calls != 3 ||
        interaction.progress != std::vector<int32_t>({11, 12, 13, 14}) ||
        dst_only_pixels != std::array<unsigned char, 16>{{42, 0, 0, 0, 42, 0, 0, 0,
                                                          42, 0, 0, 0, 42, 0, 0, 0}})
      return false;
  }
  g_iterate_interaction_test = nullptr;
  return true;
}
