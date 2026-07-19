using IteratePixel8 = int32_t(__cdecl*)(void*, int32_t, int32_t,
                                        unsigned char*, unsigned char*);
using IteratePixelRaw = int32_t(__cdecl*)(void*, int32_t, int32_t, void*, void*);
using IterateGenericCallback = int32_t(__cdecl*)(void*, int32_t, int32_t, int32_t);
using IterateAbortCallback = int32_t(__cdecl*)(void*);
using IterateProgressCallback = int32_t(__cdecl*)(void*, int32_t, int32_t);
// These callbacks are implemented by worker_pf_suites.cpp.  Keep C linkage so
// this header can be consumed from the L2 anonymous namespace without turning
// the translation unit boundary into a private mangled-symbol dependency.
extern "C" {
int32_t __cdecl copy_world8(void*, void*, void*, const LegacyRect*, const LegacyRect*);
int32_t __cdecl copy_world_hq(void*, void*, void*, const LegacyRect*, const LegacyRect*);
int32_t __cdecl iterate_world8(void*, int32_t, int32_t, void*, const LegacyRect*, void*,
                               IteratePixel8, void*);
int32_t __cdecl iterate_world16(void*, int32_t, int32_t, void*, const LegacyRect*, void*,
                                IteratePixelRaw, void*);
int32_t __cdecl iterate_world_float(void*, int32_t, int32_t, void*, const LegacyRect*, void*,
                                    IteratePixelRaw, void*);
int32_t __cdecl iterate_origin8(void*, int32_t, int32_t, void*, const LegacyRect*, const void*,
                                void*, IteratePixelRaw, void*);
int32_t __cdecl iterate_origin16(void*, int32_t, int32_t, void*, const LegacyRect*, const void*,
                                 void*, IteratePixelRaw, void*);
int32_t __cdecl iterate_origin_float(void*, int32_t, int32_t, void*, const LegacyRect*, const void*,
                                     void*, IteratePixelRaw, void*);
int32_t __cdecl iterate_lut8(void*, int32_t, int32_t, void*, const LegacyRect*,
                            unsigned char*, unsigned char*, unsigned char*, unsigned char*, void*);
int32_t __cdecl iterate_origin_non_clip8(void*, int32_t, int32_t, void*, const LegacyRect*,
                                         const void*, void*, IteratePixelRaw, void*);
int32_t __cdecl iterate_origin_non_clip16(void*, int32_t, int32_t, void*, const LegacyRect*,
                                          const void*, void*, IteratePixelRaw, void*);
int32_t __cdecl iterate_origin_non_clip_float(void*, int32_t, int32_t, void*, const LegacyRect*,
                                              const void*, void*, IteratePixelRaw, void*);
int32_t __cdecl iterate_generic(int32_t, void*, IterateGenericCallback);
int32_t __cdecl subpixel_sample8(void*, int32_t, int32_t, const void*, void*);
int32_t __cdecl subpixel_sample16(void*, int32_t, int32_t, const void*, void*);
int32_t __cdecl subpixel_sample_float(void*, int32_t, int32_t, const void*, void*);
int32_t __cdecl nearest_sample8(void*, int32_t, int32_t, const void*, void*);
int32_t __cdecl nearest_sample16(void*, int32_t, int32_t, const void*, void*);
int32_t __cdecl nearest_sample_float(void*, int32_t, int32_t, const void*, void*);
int32_t __cdecl area_sample8(void*, int32_t, int32_t, const void*, void*);
int32_t __cdecl area_sample16(void*, int32_t, int32_t, const void*, void*);
int32_t __cdecl area_sample_float(void*, int32_t, int32_t, const void*, void*);
int32_t __cdecl begin_sampling8(void*, int32_t, uint32_t, void*);
int32_t __cdecl end_sampling8(void*, int32_t, uint32_t, void*);
int32_t __cdecl unsupported_batch_sample_func(void*, int32_t, uint32_t,
                                               const void*, void**);
int32_t __cdecl fill_world8(void*, const void*, const LegacyRect*, void*);
int32_t __cdecl transform_world(void*, int32_t, uint32_t, int32_t, const void*,
                                const void*, const void*, const void*, int32_t, uint8_t,
                                const LegacyRect*, void*);
int32_t __cdecl transfer_rect(void*, int32_t, uint32_t, int32_t, const LegacyRect*,
                              const void*, const void*, const void*, int32_t, int32_t, void*);
int32_t __cdecl set_options_button_name(void*, const char*);
int32_t __cdecl get_layer_channel_count(void*, int32_t, int32_t*);
int32_t __cdecl get_layer_channel_indexed(void*, int32_t, int32_t, uint8_t*, void*, void*);
int32_t __cdecl get_layer_channel_typed(void*, int32_t, int32_t, uint8_t*, void*, void*);
int32_t __cdecl checkout_layer_channel(void*, void*, int32_t, int32_t, uint32_t, int32_t, void*);
int32_t __cdecl checkin_layer_channel(void*, void*, void*);
void reclaim_layer_channels();
bool verify_pf_ae_channel_suite();
int32_t __cdecl duck_quack(uint16_t);
int32_t __cdecl abort_render(void*);
int32_t __cdecl report_progress(void*, int32_t, int32_t);
int32_t __cdecl register_custom_ui(void*, const void*);
int32_t __cdecl adv_app_info_text(const char*, const char*);
int32_t __cdecl adv_app_info_text3(const char*, const char*, const char*);
int32_t __cdecl fill_world16(void*, const void*, const LegacyRect*, void*);
int32_t __cdecl fill_world_float(void*, const void*, const LegacyRect*, void*);
int32_t __cdecl premultiply_world8(void*, int32_t, void*);
int32_t __cdecl premultiply_color8(void*, void*, const void*, int32_t, void*);
int32_t __cdecl premultiply_color16(void*, void*, const void*, int32_t, void*);
int32_t __cdecl premultiply_color_float(void*, void*, const void*, int32_t, void*);
int32_t __cdecl convolve_world(void*, void*, const LegacyRect*, uint32_t, int32_t,
                               void*, void*, void*, void*, void*);
int32_t __cdecl blend_world(void*, const void*, const void*, int32_t, void*);
int32_t __cdecl composite_rect8(void*, LegacyRect*, int32_t, void*, int32_t, int32_t,
                                int32_t, int32_t, void*);
int32_t __cdecl legacy_new_world(void*, int32_t, int32_t, int32_t, void*);
int32_t __cdecl dispose_world(void*, void*);
bool verify_legacy_fill_matte_callbacks();
bool verify_world_transform_blend();
bool verify_world_transform_affine();
bool verify_world_transform_transfer_mask();
bool verify_iterate_suites();
bool verify_pf_batch_sampling_suite();
}
double __cdecl ansi_atan(double);
double __cdecl ansi_atan2(double, double);
double __cdecl ansi_ceil(double);
double __cdecl ansi_cos(double);
double __cdecl ansi_exp(double);
double __cdecl ansi_fabs(double);
double __cdecl ansi_floor(double);
double __cdecl ansi_fmod(double, double);
double __cdecl ansi_hypot(double, double);
double __cdecl ansi_log(double);
double __cdecl ansi_log10(double);
double __cdecl ansi_pow(double, double);
double __cdecl ansi_sin(double);
double __cdecl ansi_sqrt(double);
double __cdecl ansi_tan(double);
int __cdecl ansi_sprintf(char*, const char*, ...);
char* __cdecl ansi_strcpy(char*, const char*);
double __cdecl ansi_asin(double);
double __cdecl ansi_acos(double);
struct PfMaskSuite1 {
  decltype(&pf_mask_world_with_path) mask_world_with_path;
};
struct Iterate8Suite2 {
  void* iterate;
  decltype(&iterate_origin8) iterate_origin;
  decltype(&iterate_lut8) iterate_lut;
  decltype(&iterate_origin_non_clip8) iterate_origin_non_clip_src;
  decltype(&iterate_generic) iterate_generic;
};

using PfFixed = int32_t;
using PfFixedTriple = PfFixed*;
struct PfPixel8 { uint8_t alpha, red, green, blue; };
struct PfPixel16 { uint16_t alpha, red, green, blue; };
struct PfPixelFloat { float alpha, red, green, blue; };

template <class Pixel, class Scalar> struct PfColorCallbacks {
  int32_t (__cdecl *RGBtoHLS)(void*, Pixel*, PfFixedTriple);
  int32_t (__cdecl *HLStoRGB)(void*, PfFixedTriple, Pixel*);
  int32_t (__cdecl *RGBtoYIQ)(void*, Pixel*, PfFixedTriple);
  int32_t (__cdecl *YIQtoRGB)(void*, PfFixedTriple, Pixel*);
  int32_t (__cdecl *Luminance)(void*, Pixel*, Scalar*);
  int32_t (__cdecl *Hue)(void*, Pixel*, Scalar*);
  int32_t (__cdecl *Lightness)(void*, Pixel*, Scalar*);
  int32_t (__cdecl *Saturation)(void*, Pixel*, Scalar*);
};
using PfColorCallbacks8 = PfColorCallbacks<PfPixel8, int32_t>;
using PfColorCallbacks16 = PfColorCallbacks<PfPixel16, int32_t>;
using PfColorCallbacksFloat = PfColorCallbacks<PfPixelFloat, float>;
static_assert(sizeof(PfColorCallbacks8) == 8 * sizeof(void*));
static_assert(sizeof(PfColorCallbacks16) == 8 * sizeof(void*));
static_assert(sizeof(PfColorCallbacksFloat) == 8 * sizeof(void*));
#define PF_COLOR_OFFSET_ASSERT(T, M, N) static_assert(offsetof(T, M) == N * sizeof(void*))
PF_COLOR_OFFSET_ASSERT(PfColorCallbacks8, RGBtoHLS, 0);
PF_COLOR_OFFSET_ASSERT(PfColorCallbacks8, HLStoRGB, 1);
PF_COLOR_OFFSET_ASSERT(PfColorCallbacks8, RGBtoYIQ, 2);
PF_COLOR_OFFSET_ASSERT(PfColorCallbacks8, YIQtoRGB, 3);
PF_COLOR_OFFSET_ASSERT(PfColorCallbacks8, Luminance, 4);
PF_COLOR_OFFSET_ASSERT(PfColorCallbacks8, Hue, 5);
PF_COLOR_OFFSET_ASSERT(PfColorCallbacks8, Lightness, 6);
PF_COLOR_OFFSET_ASSERT(PfColorCallbacks8, Saturation, 7);
PF_COLOR_OFFSET_ASSERT(PfColorCallbacks16, Saturation, 7);
PF_COLOR_OFFSET_ASSERT(PfColorCallbacksFloat, Saturation, 7);
#undef PF_COLOR_OFFSET_ASSERT
static_assert(kUtilsColorCallbacks + sizeof(PfColorCallbacks8) == kUtilsGetPlatformData);

struct ColorValues { double r, g, b; };
struct HlsValues { double h, l, s; };
constexpr int32_t kPfInvalidIndex = 513;
constexpr int32_t kPfUnrecognizedParamType = 514;

bool finite3(double a, double b, double c) {
  return std::isfinite(a) && std::isfinite(b) && std::isfinite(c);
}

PfPathPoint lerp_pf_path_point(const PfPathPoint& a, const PfPathPoint& b, double t) {
  return {a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t};
}

PfPathPoint eval_pf_cubic(const PfPathCubic& c, double t) {
  const auto a = lerp_pf_path_point(c[0], c[1], t);
  const auto b = lerp_pf_path_point(c[1], c[2], t);
  const auto d = lerp_pf_path_point(c[2], c[3], t);
  return lerp_pf_path_point(lerp_pf_path_point(a, b, t),
                            lerp_pf_path_point(b, d, t), t);
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
  const auto p01 = lerp_pf_path_point(c[0], c[1], 0.5);
  const auto p12 = lerp_pf_path_point(c[1], c[2], 0.5);
  const auto p23 = lerp_pf_path_point(c[2], c[3], 0.5);
  const auto p012 = lerp_pf_path_point(p01, p12, 0.5);
  const auto p123 = lerp_pf_path_point(p12, p23, 0.5);
  const auto midpoint = lerp_pf_path_point(p012, p123, 0.5);
  const double tm = (t0 + t1) * 0.5;
  append_adaptive_pf_cubic({c[0], p01, p012, midpoint}, t0, tm,
                           tolerance * 0.5, depth + 1, parameters, points);
  append_adaptive_pf_cubic({midpoint, p123, p23, c[3]}, tm, t1,
                           tolerance * 0.5, depth + 1, parameters, points);
}
PfFixed color_to_fixed(double value) {
  const double scaled = value * 65536.0;
  if (scaled >= static_cast<double>(INT32_MAX)) return INT32_MAX;
  if (scaled <= static_cast<double>(INT32_MIN)) return INT32_MIN;
  // This follows the SDK macro; unobserved AE tie behavior is not asserted here.
  return static_cast<PfFixed>(scaled + (scaled < 0.0 ? -0.5 : 0.5));
}
double color_from_fixed(PfFixed value) { return static_cast<double>(value) / 65536.0; }

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
  // Coefficients published by the SDK color-conversion contract.
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
  static ColorValues read(const PfPixel8& p) { return {p.red / 255.0, p.green / 255.0, p.blue / 255.0}; }
  static void write(PfPixel8& p, const ColorValues& c) {
    auto channel = [](double v) { return static_cast<uint8_t>(std::lround((std::max)(0.0, (std::min)(1.0, v)) * 255.0)); };
    p.red = channel(c.r); p.green = channel(c.g); p.blue = channel(c.b);
  }
  static constexpr double scale = 255.0;
};
template <> struct ColorPixelTraits<PfPixel16> {
  static ColorValues read(const PfPixel16& p) { return {p.red / 32768.0, p.green / 32768.0, p.blue / 32768.0}; }
  static void write(PfPixel16& p, const ColorValues& c) {
    auto channel = [](double v) { return static_cast<uint16_t>(std::lround((std::max)(0.0, (std::min)(1.0, v)) * 32768.0)); };
    p.red = channel(c.r); p.green = channel(c.g); p.blue = channel(c.b);
  }
  static constexpr double scale = 32768.0;
};
template <> struct ColorPixelTraits<PfPixelFloat> {
  static ColorValues read(const PfPixelFloat& p) { return {p.red, p.green, p.blue}; }
  static void write(PfPixelFloat& p, const ColorValues& c) {
    p.red = static_cast<float>(c.r); p.green = static_cast<float>(c.g); p.blue = static_cast<float>(c.b);
  }
  static constexpr double scale = 1.0;
};

template <class Pixel> int32_t __cdecl color_rgb_to_hls(void*, Pixel* rgb, PfFixedTriple out) {
  if (!rgb || !out) return kPfBadCallbackParam;
  const ColorValues c = ColorPixelTraits<Pixel>::read(*rgb);
  if (!finite3(c.r, c.g, c.b)) return kPfBadCallbackParam;
  const HlsValues hls = rgb_to_hls_values(c);
  if (!finite3(hls.h, hls.l, hls.s)) return kPfBadCallbackParam;
  PfFixed result[3]{color_to_fixed(hls.h * 360.0), color_to_fixed(hls.l),
                    color_to_fixed(hls.s)};
  std::memcpy(out, result, sizeof(result)); return 0;
}
template <class Pixel> int32_t __cdecl color_hls_to_rgb(void*, PfFixedTriple in, Pixel* rgb) {
  if (!in || !rgb) return kPfBadCallbackParam;
  const HlsValues hls{color_from_fixed(in[0]) / 360.0, color_from_fixed(in[1]),
                      color_from_fixed(in[2])};
  const ColorValues c = hls_to_rgb_values(hls);
  if (!finite3(c.r, c.g, c.b)) return kPfBadCallbackParam;
  ColorPixelTraits<Pixel>::write(*rgb, c); return 0;
}
template <class Pixel> int32_t __cdecl color_rgb_to_yiq(void*, Pixel* rgb, PfFixedTriple out) {
  if (!rgb || !out) return kPfBadCallbackParam;
  const ColorValues c = ColorPixelTraits<Pixel>::read(*rgb);
  if (!finite3(c.r, c.g, c.b)) return kPfBadCallbackParam;
  const ColorValues yiq = rgb_to_yiq_values(c);
  PfFixed result[3]{color_to_fixed(yiq.r), color_to_fixed(yiq.g), color_to_fixed(yiq.b)};
  std::memcpy(out, result, sizeof(result)); return 0;
}
template <class Pixel> int32_t __cdecl color_yiq_to_rgb(void*, PfFixedTriple in, Pixel* rgb) {
  if (!in || !rgb) return kPfBadCallbackParam;
  const ColorValues yiq{color_from_fixed(in[0]), color_from_fixed(in[1]), color_from_fixed(in[2])};
  const ColorValues c = yiq_to_rgb_values(yiq);
  if (!finite3(c.r, c.g, c.b)) return kPfBadCallbackParam;
  ColorPixelTraits<Pixel>::write(*rgb, c); return 0;
}
template <class Pixel, class Scalar, int Which>
int32_t __cdecl color_scalar(void*, Pixel* rgb, Scalar* out) {
  if (!rgb || !out) return kPfBadCallbackParam;
  const ColorValues c = ColorPixelTraits<Pixel>::read(*rgb);
  if (!finite3(c.r, c.g, c.b)) return kPfBadCallbackParam;
  const HlsValues hls = rgb_to_hls_values(c);
  const double value = Which == 0 ? rgb_to_yiq_values(c).r
                     : Which == 1 ? hls.h : Which == 2 ? hls.l : hls.s;
  if (!std::isfinite(value)) return kPfBadCallbackParam;
  if constexpr (std::is_same_v<Scalar, float>) {
    *out = static_cast<float>(Which == 1 ? value * 360.0 : value);
  } else {
    const double scale = Which == 0 ? 100.0 * ColorPixelTraits<Pixel>::scale
                         : Which == 1 ? 255.0
                                      : ColorPixelTraits<Pixel>::scale;
    *out = static_cast<int32_t>(std::lround(value * scale));
  }
  return 0;
}

#define PF_COLOR_SUITE(P, S) {&color_rgb_to_hls<P>, &color_hls_to_rgb<P>, \
  &color_rgb_to_yiq<P>, &color_yiq_to_rgb<P>, &color_scalar<P, S, 0>, \
  &color_scalar<P, S, 1>, &color_scalar<P, S, 2>, &color_scalar<P, S, 3>}
PfColorCallbacks8 g_color_suite8 = PF_COLOR_SUITE(PfPixel8, int32_t);
PfColorCallbacks16 g_color_suite16 = PF_COLOR_SUITE(PfPixel16, int32_t);
PfColorCallbacksFloat g_color_suite_float = PF_COLOR_SUITE(PfPixelFloat, float);
#undef PF_COLOR_SUITE
struct Iterate16Suite2 {
  decltype(&iterate_world16) iterate;
  decltype(&iterate_origin16) iterate_origin;
  decltype(&iterate_origin_non_clip16) iterate_origin_non_clip_src;
};
struct IterateFloatSuite2 {
  decltype(&iterate_world_float) iterate;
  decltype(&iterate_origin_float) iterate_origin;
  decltype(&iterate_origin_non_clip_float) iterate_origin_non_clip_src;
};
static_assert(sizeof(Iterate8Suite2) == 5 * sizeof(void*));
static_assert(offsetof(Iterate8Suite2, iterate_lut) == 2 * sizeof(void*));
static_assert(offsetof(Iterate8Suite2, iterate_origin_non_clip_src) == 3 * sizeof(void*));
static_assert(offsetof(Iterate8Suite2, iterate_generic) == 4 * sizeof(void*));
static_assert(sizeof(Iterate16Suite2) == 3 * sizeof(void*));
static_assert(sizeof(IterateFloatSuite2) == 3 * sizeof(void*));
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
static_assert(offsetof(WorldTransformSuite1, blend) == 1 * sizeof(void*));
static_assert(offsetof(WorldTransformSuite1, convolve) == 2 * sizeof(void*));
static_assert(offsetof(WorldTransformSuite1, copy) == 3 * sizeof(void*));
static_assert(offsetof(WorldTransformSuite1, copy_hq) == 4 * sizeof(void*));
static_assert(offsetof(WorldTransformSuite1, transfer_rect) == 5 * sizeof(void*));
static_assert(offsetof(WorldTransformSuite1, transform_world) == 6 * sizeof(void*));
struct MaskSuite {
  decltype(&get_layer_num_masks) get_layer_num_masks;
  decltype(&get_layer_mask_by_index) get_layer_mask_by_index;
  decltype(&dispose_mask) dispose_mask;
  decltype(&get_mask_invert) get_invert;
  decltype(&set_mask_invert) set_invert;
  decltype(&get_mask_mode) get_mode;
  decltype(&set_mask_mode) set_mode;
  decltype(&get_mask_motion_blur) get_motion_blur;
  decltype(&set_mask_motion_blur) set_motion_blur;
  decltype(&get_mask_feather_falloff) get_feather_falloff;
  decltype(&set_mask_feather_falloff) set_feather_falloff;
  decltype(&get_mask_id) get_id;
  decltype(&create_new_mask) create_new;
  decltype(&delete_mask_from_layer) delete_from_layer;
  decltype(&get_mask_color) get_color;
  decltype(&set_mask_color) set_color;
  decltype(&get_mask_lock) get_lock;
  decltype(&set_mask_lock) set_lock;
  decltype(&get_mask_roto_bezier) get_roto_bezier;
  decltype(&set_mask_roto_bezier) set_roto_bezier;
  decltype(&duplicate_mask) duplicate;
};
struct MaskSuite5 {
  decltype(&get_layer_num_masks) get_layer_num_masks;
  decltype(&get_layer_mask_by_index) get_layer_mask_by_index;
  decltype(&dispose_mask) dispose_mask;
  decltype(&get_mask_invert) get_invert;
  decltype(&set_mask_invert) set_invert;
  decltype(&get_mask_mode) get_mode;
  decltype(&set_mask_mode) set_mode;
  decltype(&get_mask_motion_blur) get_motion_blur;
  decltype(&set_mask_motion_blur) set_motion_blur;
  decltype(&get_mask_id) get_id;
  decltype(&create_new_mask) create_new;
  decltype(&delete_mask_from_layer) delete_from_layer;
  decltype(&get_mask_color) get_color;
  decltype(&set_mask_color) set_color;
  decltype(&get_mask_lock) get_lock;
  decltype(&set_mask_lock) set_lock;
  decltype(&get_mask_roto_bezier) get_roto_bezier;
  decltype(&set_mask_roto_bezier) set_roto_bezier;
  decltype(&duplicate_mask) duplicate;
};
struct StreamSuite {
  decltype(&is_stream_legal) is_stream_legal;
  decltype(&can_vary_over_time) can_vary_over_time;
  decltype(&get_valid_interpolations) get_valid_interpolations;
  decltype(&unsupported_new_layer_stream) get_new_layer_stream;
  decltype(&unsupported_effect_stream_count) get_effect_num_param_streams;
  decltype(&unsupported_new_effect_stream) get_new_effect_stream_by_index;
  decltype(&get_new_mask_stream) get_new_mask_stream;
  decltype(&dispose_stream) dispose_stream;
  decltype(&unsupported_stream_name) get_stream_name;
  decltype(&get_stream_units_text) get_stream_units_text;
  decltype(&get_stream_properties) get_stream_properties;
  decltype(&is_stream_timevarying) is_stream_timevarying;
  decltype(&get_stream_type) get_stream_type;
  decltype(&get_new_stream_value) get_new_stream_value;
  decltype(&dispose_stream_value) dispose_stream_value;
  decltype(&set_stream_value) set_stream_value;
  decltype(&unsupported_layer_stream_value) get_layer_stream_value;
  decltype(&get_expression_state) get_expression_state;
  decltype(&reject_expression_state) set_expression_state;
  decltype(&unsupported_get_expression) get_expression;
  decltype(&unsupported_set_expression) set_expression;
  decltype(&duplicate_stream_ref) duplicate_stream_ref;
  decltype(&get_unique_stream_id) get_unique_stream_id;
};
static_assert(sizeof(StreamSuite) == 23 * sizeof(void*));
struct KeyframeSuite {
  decltype(&get_stream_num_keyframes) get_stream_num_keyframes;
  decltype(&get_keyframe_time) get_keyframe_time;
  decltype(&insert_keyframe) insert_keyframe;
  decltype(&delete_keyframe) delete_keyframe;
  decltype(&get_new_keyframe_value) get_new_keyframe_value;
  decltype(&set_keyframe_value) set_keyframe_value;
  decltype(&get_stream_value_dimensionality) get_stream_value_dimensionality;
  decltype(&get_stream_temporal_dimensionality) get_stream_temporal_dimensionality;
  decltype(&get_new_keyframe_spatial_tangents) get_new_keyframe_spatial_tangents;
  decltype(&set_keyframe_spatial_tangents) set_keyframe_spatial_tangents;
  decltype(&get_keyframe_temporal_ease) get_keyframe_temporal_ease;
  decltype(&set_keyframe_temporal_ease) set_keyframe_temporal_ease;
  decltype(&get_keyframe_flags) get_keyframe_flags;
  decltype(&set_keyframe_flag) set_keyframe_flag;
  decltype(&get_keyframe_interpolation) get_keyframe_interpolation;
  decltype(&set_keyframe_interpolation) set_keyframe_interpolation;
  decltype(&start_add_keyframes) start_add_keyframes;
  decltype(&add_keyframes) add_keyframes;
  decltype(&set_add_keyframe) set_add_keyframe;
  decltype(&end_add_keyframes) end_add_keyframes;
  decltype(&get_keyframe_label) get_keyframe_label_color_index;
  decltype(&set_keyframe_label) set_keyframe_label_color_index;
};
static_assert(sizeof(KeyframeSuite) == 22 * sizeof(void*));
struct DynamicStreamSuite {
  decltype(&get_new_dynamic_stream_for_layer) get_new_stream_ref_for_layer;
  decltype(&get_new_dynamic_stream_for_mask) get_new_stream_ref_for_mask;
  decltype(&get_dynamic_stream_depth) get_stream_depth;
  decltype(&get_dynamic_stream_grouping_type) get_stream_grouping_type;
  decltype(&get_num_streams_in_group) get_num_streams_in_group;
  decltype(&get_dynamic_stream_flags) get_dynamic_stream_flags;
  decltype(&set_dynamic_stream_flag) set_dynamic_stream_flag;
  decltype(&get_new_dynamic_stream_by_index) get_new_stream_ref_by_index;
  decltype(&get_new_dynamic_stream_by_match_name) get_new_stream_ref_by_match_name;
  decltype(&delete_dynamic_stream) delete_stream;
  decltype(&reorder_dynamic_stream) reorder_stream;
  decltype(&duplicate_dynamic_stream) duplicate_stream;
  decltype(&set_dynamic_stream_name) set_stream_name;
  decltype(&can_add_dynamic_stream) can_add_stream;
  decltype(&add_dynamic_stream) add_stream;
  decltype(&get_dynamic_match_name) get_match_name;
  decltype(&get_new_parent_dynamic_stream) get_new_parent_stream_ref;
  decltype(&get_dynamic_stream_modified) get_stream_is_modified;
  decltype(&get_dynamic_stream_index) get_stream_index_in_parent;
  decltype(&is_separation_leader) is_separation_leader;
  decltype(&are_dimensions_separated) are_dimensions_separated;
  decltype(&reject_set_dimensions_separated) set_dimensions_separated;
  decltype(&reject_get_separation_follower) get_separation_follower;
  decltype(&is_separation_follower) is_separation_follower;
  decltype(&reject_get_separation_leader) get_separation_leader;
  decltype(&reject_get_separation_dimension) get_separation_dimension;
};
static_assert(sizeof(DynamicStreamSuite) == 26 * sizeof(void*));
struct MaskOutlineSuite {
  decltype(&is_mask_outline_open) is_open;
  decltype(&set_mask_outline_open) set_open;
  decltype(&get_mask_outline_num_segments) get_num_segments;
  decltype(&get_mask_outline_vertex_info) get_vertex_info;
  decltype(&set_mask_outline_vertex_info) set_vertex_info;
  decltype(&create_mask_outline_vertex) create_vertex;
  decltype(&delete_mask_outline_vertex) delete_vertex;
  decltype(&get_mask_outline_num_feathers) get_num_feathers;
  decltype(&get_mask_outline_feather_info) get_feather_info;
  decltype(&set_mask_outline_feather_info) set_feather_info;
  decltype(&create_mask_outline_feather) create_feather;
  decltype(&delete_mask_outline_feather) delete_feather;
};

UtilitySuite g_utility_suite{{}, &register_with_aegp, &get_main_hwnd, {}};
UtilitySuite3 g_utility_suite3{{}, &register_with_aegp, &get_main_hwnd, {}};
PfInterfaceSuite g_pf_interface_suite{&get_effect_layer, &get_new_effect_for_effect,
    &convert_effect_to_comp_time, &get_effect_camera,
    &get_effect_camera_matrix};
std::array<void*, 14> g_aegp_dynamic_stream_suite2{};
// Stream Suite v2 has a private payload type shared with the AEGP scene
// runtime. Keep the declaration in its owning header so this ABI table never
// carries a conflicting local forward declaration.
#include "worker_aegp_scene_runtime.hpp"
using AegpStreamValue = aexcompat::scene_runtime::AegpStreamValue;
int32_t __cdecl aegp_get_new_effect_stream_by_index_v2(
    int32_t plugin_id, void* effect, int32_t index, void** stream);
int32_t __cdecl aegp_dispose_stream_v2(void* stream);
int32_t __cdecl aegp_get_stream_name_v2(void* stream, uint8_t force_english, char* name);
int32_t __cdecl aegp_get_stream_type_v2(void* stream, int32_t* type);
int32_t __cdecl aegp_get_new_stream_value_v2(
    int32_t plugin_id, void* stream, int32_t time_mode, const AegpTime* time,
    uint8_t pre_expression, AegpStreamValue* output);
int32_t __cdecl aegp_dispose_stream_value_v2(AegpStreamValue* output);
int32_t __cdecl aegp_set_stream_value_v2(
    int32_t plugin_id, void* stream, AegpStreamValue* input);
int32_t __cdecl aegp_set_dynamic_stream_flag_v2(
    void* stream, uint32_t one_flag, uint8_t undoable, uint8_t set);
int32_t __cdecl aegp_get_effect_param_union_by_index_v3(
    int32_t plugin_id, void* effect, int32_t index, int32_t* type, void* param_union);
PfMaskSuite1 g_pf_mask_suite1{&pf_mask_world_with_path};
std::array<void*, 4> g_pf_path_query_suite1{};
std::array<void*, 11> g_pf_path_data_suite1{};
Iterate8Suite2 g_iterate8_suite2{reinterpret_cast<void*>(&iterate_world8), &iterate_origin8, &iterate_lut8,
                                 &iterate_origin_non_clip8, &iterate_generic};
Iterate16Suite2 g_iterate16_suite2{&iterate_world16, &iterate_origin16,
                                   &iterate_origin_non_clip16};
IterateFloatSuite2 g_iterate_float_suite2{&iterate_world_float, &iterate_origin_float,
                                           &iterate_origin_non_clip_float};
std::array<void*, 3> g_sampling16_suite1{};
std::array<void*, 3> g_sampling_float_suite1{};
std::array<void*, 3> g_sampling8_suite1{};
using BatchSamplingBegin = int32_t(__cdecl*)(void*, int32_t, uint32_t, void*);
using BatchSamplingGetter = int32_t(__cdecl*)(void*, int32_t, uint32_t, const void*, void**);
struct PfBatchSamplingSuite1 {
  BatchSamplingBegin begin_sampling;
  BatchSamplingBegin end_sampling;
  BatchSamplingGetter get_batch_func;
  BatchSamplingGetter get_batch_func16;
};
static_assert(sizeof(PfBatchSamplingSuite1) == 4 * sizeof(void*));
static_assert(offsetof(PfBatchSamplingSuite1, begin_sampling) == 0);
static_assert(offsetof(PfBatchSamplingSuite1, end_sampling) == sizeof(void*));
static_assert(offsetof(PfBatchSamplingSuite1, get_batch_func) == 2 * sizeof(void*));
static_assert(offsetof(PfBatchSamplingSuite1, get_batch_func16) == 3 * sizeof(void*));
PfBatchSamplingSuite1 g_batch_sampling_suite1{};
using PfConstHandle = const void* const*;
using GetEffectSequenceData = int32_t(__cdecl*)(void*, PfConstHandle*);
struct PfEffectSequenceDataSuite1 {
  GetEffectSequenceData get_effect_sequence_data;
};
static_assert(sizeof(PfEffectSequenceDataSuite1) == sizeof(void*));

constexpr std::size_t kMaxLiveEffectSequences = 64;
struct LiveEffectSequence {
  void* effect_ref{};
  PfConstHandle sequence_handle{};
  uint64_t generation{};
};
std::mutex g_effect_sequence_mutex;
std::vector<LiveEffectSequence> g_live_effect_sequences;
uint64_t g_effect_sequence_generation{};
uint64_t g_effect_sequence_publications{};
uint64_t g_effect_sequence_invalidations{};

void invalidate_effect_sequence(void* effect_ref) {
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  const auto old_size = g_live_effect_sequences.size();
  g_live_effect_sequences.erase(
      std::remove_if(g_live_effect_sequences.begin(), g_live_effect_sequences.end(),
          [effect_ref](const auto& entry) { return entry.effect_ref == effect_ref; }),
      g_live_effect_sequences.end());
  if (g_live_effect_sequences.size() != old_size) ++g_effect_sequence_invalidations;
}

bool publish_effect_sequence(void* effect_ref, void* sequence_handle) {
  if (!effect_ref || !sequence_handle) return false;
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  auto found = std::find_if(g_live_effect_sequences.begin(), g_live_effect_sequences.end(),
      [effect_ref](const auto& entry) { return entry.effect_ref == effect_ref; });
  if (found == g_live_effect_sequences.end()) {
    if (g_live_effect_sequences.size() >= kMaxLiveEffectSequences) return false;
    g_live_effect_sequences.push_back({effect_ref,
        reinterpret_cast<PfConstHandle>(sequence_handle), ++g_effect_sequence_generation});
  } else {
    found->sequence_handle = reinterpret_cast<PfConstHandle>(sequence_handle);
    found->generation = ++g_effect_sequence_generation;
  }
  ++g_effect_sequence_publications;
  return true;
}

int32_t __cdecl get_effect_sequence_data(void* effect_ref, PfConstHandle* sequence_handle) {
  if (!sequence_handle) return kPfBadCallbackParam;
  *sequence_handle = nullptr;
  if (!effect_ref) return kPfBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  const auto found = std::find_if(g_live_effect_sequences.begin(), g_live_effect_sequences.end(),
      [effect_ref](const auto& entry) { return entry.effect_ref == effect_ref; });
  if (found == g_live_effect_sequences.end() || !found->sequence_handle)
    return kPfBadCallbackParam;
  *sequence_handle = found->sequence_handle;
  return 0;
}
PfEffectSequenceDataSuite1 g_effect_sequence_data_suite1{&get_effect_sequence_data};
std::array<void*, 7> g_fill_matte_suite2{};
WorldTransformSuite1 g_world_transform_suite1{};
std::array<void*, 19> g_ansi_suite1{};
std::array<void*, 1> g_effect_ui_suite1{};
std::array<void*, 3> g_pf_helper_suite2{
    reinterpret_cast<void*>(&pf_parse_clipboard),
    reinterpret_cast<void*>(&pf_set_current_extended_tool),
    reinterpret_cast<void*>(&pf_get_current_extended_tool)};
std::array<void*, 1> g_pf_helper_suite1{
    reinterpret_cast<void*>(&pf_get_current_tool)};
static_assert(sizeof(g_pf_helper_suite1) == sizeof(void*));
// PF_AdvAppSuite1 is frozen at ten callbacks; keep its storage independent
// from the eleven-slot v2 table so versioned suite identity cannot alias.
std::array<void*, 10> g_adv_app_suite1{};
std::array<void*, 11> g_adv_app_suite2{};
static_assert(sizeof(g_adv_app_suite1) == 10 * sizeof(void*));
