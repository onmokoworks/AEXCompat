#pragma once

#include "worker_world_safety.hpp"

#include <cstdint>

struct LegacyRect { int32_t left, top, right, bottom; };

namespace aexcompat::pf_world_transform {

struct Hooks {
  bool (__cdecl *resolve_world)(void*, int32_t, unsigned char*&, int32_t&,
                                int32_t&, int32_t&){};
  bool (__cdecl *resolve_dispatch_world_format)(
      const void*, world_safety::DispatchWorldFormat&){};
  const char* (__cdecl *pixel_format)(){};
  bool (__cdecl *set_pixel_format)(const char*){};
  bool (*bounded_argb8_world)(void*, unsigned char*&, int32_t&, int32_t&, int32_t&){};
};

struct Telemetry {
  uint32_t* calls{};
  int32_t* last_x{};
  int32_t* last_y{};
  uint8_t* last_opacity{};
};

struct Context {
  Hooks hooks{};
  Telemetry telemetry{};
};

void configure(const Context& context);
bool configured() noexcept;

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
int32_t __cdecl copy_world8(void*, void*, void*, const LegacyRect*, const LegacyRect*);
int32_t __cdecl copy_world_hq(void*, void*, void*, const LegacyRect*, const LegacyRect*);
int32_t __cdecl transform_world(void*, int32_t, uint32_t, int32_t, const void*,
                                const void*, const void*, const void*, int32_t, uint8_t,
                                const LegacyRect*, void*);
int32_t __cdecl transfer_rect(void*, int32_t, uint32_t, int32_t, const LegacyRect*,
                              const void*, const void*, const void*, int32_t, int32_t, void*);

bool verify_legacy_fill_matte_callbacks();
bool verify_world_transform_blend();
bool verify_bad_callback_param_contract();
bool verify_copy_world_clipping();
bool verify_world_transform_affine();
bool verify_world_transform_transfer_mask();
int32_t __cdecl composite_rect8(void*, LegacyRect*, int32_t, void*, int32_t,
                                int32_t, int32_t, int32_t, void*);
bool verify_world_transform_composite_rect();

const void* provide_world_transform1(void*);
const void* provide_fill_matte2(void*);

}  // namespace aexcompat::pf_world_transform
