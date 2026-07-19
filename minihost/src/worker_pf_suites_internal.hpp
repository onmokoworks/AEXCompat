#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <vector>

#include "worker_world_safety.hpp"
#include "worker_pf_world_transform_runtime.hpp"

constexpr int32_t kPfInvalidIndex = 513;
constexpr int32_t kPfUnrecognizedParamType = 514;

// The PF suite implementation is deliberately compiled separately from the
// worker.  It only receives host state through this table; callbacks must
// fail closed until the worker installs a context for the current process.
struct LocalEffectWorld {
  void* reserved0{};
  void* reserved1{};
  int32_t world_flags{};
  void* data{};
  int32_t rowbytes{};
  int32_t width{};
  int32_t height{};
  LegacyRect extent_hint{};
  void* platform_ref{};
  int32_t reserved_long1{};
  void* reserved_long4{};
  struct { int32_t num{}; uint32_t den{}; } pix_aspect_ratio{};
  void* reserved_long2{};
  int32_t origin_x{};
  int32_t origin_y{};
  int32_t reserved_long3{};
  int32_t dephault{};
};
static_assert(sizeof(LocalEffectWorld) == 120);

struct PfHostHooks {
  bool (__cdecl *resolve_world)(void* world, int32_t pixel_bytes,
                                unsigned char*& pixels, int32_t& rowbytes,
                                int32_t& width, int32_t& height){};
  bool (__cdecl *resolve_dispatch_world_format)(
      const void* world, aexcompat::world_safety::DispatchWorldFormat& result){};
  const char* (__cdecl *pixel_format)(){};
  bool (__cdecl *set_pixel_format)(const char* value){};
  int32_t (__cdecl *acquire_suite)(const char* name, int32_t version,
                                   const void** suite){};
  int32_t (__cdecl *release_suite)(const char* name, int32_t version){};
};

struct PfHostContext {
  PfHostHooks hooks{};
  void* effect_ref{};
  void* batch_sampling_suite{};
};

void configure_pf_host_context(const PfHostContext& context);
bool pf_host_context_configured();

using IteratePixel8 = int32_t(__cdecl*)(void*, int32_t, int32_t,
                                        unsigned char*, unsigned char*);
using IteratePixelRaw = int32_t(__cdecl*)(void*, int32_t, int32_t, void*, void*);
using IterateGenericCallback = int32_t(__cdecl*)(void*, int32_t, int32_t, int32_t);
using IterateAbortCallback = int32_t(__cdecl*)(void*);
using IterateProgressCallback = int32_t(__cdecl*)(void*, int32_t, int32_t);

extern "C" {
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
bool verify_iterate_suites();
bool verify_pf_batch_sampling_suite();
}

using aexcompat::pf_world_transform::blend_world;
using aexcompat::pf_world_transform::composite_rect8;
using aexcompat::pf_world_transform::convolve_world;
using aexcompat::pf_world_transform::copy_world8;
using aexcompat::pf_world_transform::copy_world_hq;
using aexcompat::pf_world_transform::fill_world8;
using aexcompat::pf_world_transform::fill_world16;
using aexcompat::pf_world_transform::fill_world_float;
using aexcompat::pf_world_transform::premultiply_world8;
using aexcompat::pf_world_transform::premultiply_color8;
using aexcompat::pf_world_transform::premultiply_color16;
using aexcompat::pf_world_transform::premultiply_color_float;
using aexcompat::pf_world_transform::transfer_rect;
using aexcompat::pf_world_transform::transform_world;
using aexcompat::pf_world_transform::verify_legacy_fill_matte_callbacks;
using aexcompat::pf_world_transform::verify_world_transform_affine;
using aexcompat::pf_world_transform::verify_world_transform_blend;
using aexcompat::pf_world_transform::verify_world_transform_composite_rect;
using aexcompat::pf_world_transform::verify_world_transform_transfer_mask;


using PfPathPoint = std::array<double, 2>;
using PfPathCubic = std::array<PfPathPoint, 4>;
PfPathPoint eval_pf_cubic(const PfPathCubic& cubic, double t);
PfPathPoint deriv_pf_cubic(const PfPathCubic& cubic, double t);
double pf_path_point_distance(const PfPathPoint& a, const PfPathPoint& b);
void append_adaptive_pf_cubic(const PfPathCubic& cubic, double t0, double t1,
                              double tolerance, int depth,
                              std::vector<double>& parameters,
                              std::vector<PfPathPoint>& points);

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
extern PfColorCallbacks8 g_color_suite8;
extern PfColorCallbacks16 g_color_suite16;
extern PfColorCallbacksFloat g_color_suite_float;
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
PfFixed pf_color_to_fixed(double value);

struct Iterate8Suite2 {
  void* iterate;
  decltype(&iterate_origin8) iterate_origin;
  decltype(&iterate_lut8) iterate_lut;
  decltype(&iterate_origin_non_clip8) iterate_origin_non_clip_src;
  decltype(&iterate_generic) iterate_generic;
};
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
extern Iterate8Suite2 g_iterate8_suite2;
extern Iterate16Suite2 g_iterate16_suite2;
extern IterateFloatSuite2 g_iterate_float_suite2;
extern std::array<void*, 3> g_sampling8_suite1;
extern std::array<void*, 3> g_sampling16_suite1;
extern std::array<void*, 3> g_sampling_float_suite1;

using BatchSamplingBegin = int32_t(__cdecl*)(void*, int32_t, uint32_t, void*);
using BatchSamplingGetter = int32_t(__cdecl*)(void*, int32_t, uint32_t,
                                             const void*, void**);
struct PfBatchSamplingSuite1 {
  BatchSamplingBegin begin_sampling;
  BatchSamplingBegin end_sampling;
  BatchSamplingGetter get_batch_func;
  BatchSamplingGetter get_batch_func16;
};
extern PfBatchSamplingSuite1 g_batch_sampling_suite1;

