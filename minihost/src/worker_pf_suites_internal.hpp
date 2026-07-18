#pragma once

#include <array>
#include <cstddef>
#include <cstdint>

#include "worker_world_safety.hpp"

// The PF suite implementation is deliberately compiled separately from the
// worker.  It only receives host state through this table; callbacks must
// fail closed until the worker installs a context for the current process.
struct LegacyRect { int32_t left, top, right, bottom; };

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

struct PfTransformTelemetry {
  uint32_t* calls{};
  int32_t* last_x{};
  int32_t* last_y{};
  uint8_t* last_opacity{};
};

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
  PfTransformTelemetry transform_telemetry{};
};

void configure_pf_host_context(const PfHostContext& context);
bool pf_host_context_configured();

#ifdef AEXCOMPAT_PF_SUITE_IMPLEMENTATION
using IteratePixel8 = int32_t(__cdecl*)(void*, int32_t, int32_t,
                                        unsigned char*, unsigned char*);
using IteratePixelRaw = int32_t(__cdecl*)(void*, int32_t, int32_t, void*, void*);
using IterateGenericCallback = int32_t(__cdecl*)(void*, int32_t, int32_t, int32_t);
using IterateAbortCallback = int32_t(__cdecl*)(void*);
using IterateProgressCallback = int32_t(__cdecl*)(void*, int32_t, int32_t);

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
int32_t __cdecl fill_world16(void*, const void*, const LegacyRect*, void*);
int32_t __cdecl fill_world_float(void*, const void*, const LegacyRect*, void*);
int32_t __cdecl premultiply_world8(void*, int32_t, void*);
int32_t __cdecl premultiply_color8(void*, void*, const void*, int32_t, void*);
int32_t __cdecl premultiply_color16(void*, void*, const void*, int32_t, void*);
int32_t __cdecl premultiply_color_float(void*, void*, const void*, int32_t, void*);
int32_t __cdecl convolve_world(void*, void*, const LegacyRect*, uint32_t, int32_t,
                               void*, void*, void*, void*, void*);
int32_t __cdecl blend_world(void*, const void*, const void*, int32_t, void*);
int32_t __cdecl transform_world(void*, int32_t, uint32_t, int32_t, const void*,
                                const void*, const void*, const void*, int32_t, uint8_t,
                                const LegacyRect*, void*);
int32_t __cdecl transfer_rect(void*, int32_t, uint32_t, int32_t, const LegacyRect*,
                              const void*, const void*, const void*, int32_t, int32_t, void*);
bool verify_legacy_fill_matte_callbacks();
bool verify_world_transform_blend();
bool verify_world_transform_affine();
bool verify_world_transform_transfer_mask();
bool verify_iterate_suites();
bool verify_pf_batch_sampling_suite();
}
#endif
