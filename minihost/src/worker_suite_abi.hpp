#pragma once

#include <cstddef>
#include <cstdint>
#include <type_traits>

namespace aexcompat::suite_abi {

// Clean-room value ABI used by the render-options callbacks. These mirror the
// SDK's two-word rational time and four-coordinate long rectangle without
// importing proprietary SDK headers into the worker.
struct AegpTime {
  int32_t value{};
  uint32_t scale{1};
};
struct AegpRect {
  int32_t left;
  int32_t top;
  int32_t right;
  int32_t bottom;
};

static_assert(std::is_standard_layout_v<AegpTime>);
static_assert(sizeof(AegpTime) == 8);
static_assert(alignof(AegpTime) == alignof(uint32_t));
static_assert(offsetof(AegpTime, value) == 0);
static_assert(offsetof(AegpTime, scale) == 4);
static_assert(std::is_standard_layout_v<AegpRect>);
static_assert(sizeof(AegpRect) == 16);
static_assert(alignof(AegpRect) == alignof(int32_t));
static_assert(offsetof(AegpRect, left) == 0);
static_assert(offsetof(AegpRect, top) == 4);
static_assert(offsetof(AegpRect, right) == 8);
static_assert(offsetof(AegpRect, bottom) == 12);

using AegpLayerOptionsNew = int32_t(__cdecl*)(int32_t, void*, void**);
using AegpLayerOptionsDuplicate = int32_t(__cdecl*)(int32_t, void*, void**);
using AegpLayerOptionsDispose = int32_t(__cdecl*)(void*);
using AegpLayerOptionsSetTime = int32_t(__cdecl*)(void*, AegpTime);
using AegpLayerOptionsGetTime = int32_t(__cdecl*)(void*, AegpTime*);
using AegpLayerOptionsSetWorldType = int32_t(__cdecl*)(void*, int32_t);
using AegpLayerOptionsGetWorldType = int32_t(__cdecl*)(void*, int32_t*);
using AegpLayerOptionsSetDownsample = int32_t(__cdecl*)(void*, int16_t, int16_t);
using AegpLayerOptionsGetDownsample = int32_t(__cdecl*)(void*, int16_t*, int16_t*);
using AegpLayerOptionsSetMatte = int32_t(__cdecl*)(void*, int32_t);
using AegpLayerOptionsGetMatte = int32_t(__cdecl*)(void*, int32_t*);

struct AegpLayerRenderOptionsSuite1 {
  AegpLayerOptionsNew new_from_layer;
  AegpLayerOptionsNew new_from_upstream_of_effect;
  AegpLayerOptionsDuplicate duplicate;
  AegpLayerOptionsDispose dispose;
  AegpLayerOptionsSetTime set_time;
  AegpLayerOptionsGetTime get_time;
  AegpLayerOptionsSetTime set_time_step;
  AegpLayerOptionsGetTime get_time_step;
  AegpLayerOptionsSetWorldType set_world_type;
  AegpLayerOptionsGetWorldType get_world_type;
  AegpLayerOptionsSetDownsample set_downsample;
  AegpLayerOptionsGetDownsample get_downsample;
  AegpLayerOptionsSetMatte set_matte;
  AegpLayerOptionsGetMatte get_matte;
};

struct AegpLayerRenderOptionsSuite2 {
  AegpLayerOptionsNew new_from_layer;
  AegpLayerOptionsNew new_from_upstream_of_effect;
  AegpLayerOptionsNew new_from_downstream_of_effect;
  AegpLayerOptionsDuplicate duplicate;
  AegpLayerOptionsDispose dispose;
  AegpLayerOptionsSetTime set_time;
  AegpLayerOptionsGetTime get_time;
  AegpLayerOptionsSetTime set_time_step;
  AegpLayerOptionsGetTime get_time_step;
  AegpLayerOptionsSetWorldType set_world_type;
  AegpLayerOptionsGetWorldType get_world_type;
  AegpLayerOptionsSetDownsample set_downsample;
  AegpLayerOptionsGetDownsample get_downsample;
  AegpLayerOptionsSetMatte set_matte;
  AegpLayerOptionsGetMatte get_matte;
};

using AegpRenderOptionsNew = int32_t(__cdecl*)(int32_t, void*, void**);
using AegpRenderOptionsDuplicate = int32_t(__cdecl*)(int32_t, void*, void**);
using AegpRenderOptionsDispose = int32_t(__cdecl*)(void*);
using AegpRenderOptionsSetTime = int32_t(__cdecl*)(void*, AegpTime);
using AegpRenderOptionsGetTime = int32_t(__cdecl*)(void*, AegpTime*);
using AegpRenderOptionsSetI32 = int32_t(__cdecl*)(void*, int32_t);
using AegpRenderOptionsGetI32 = int32_t(__cdecl*)(void*, int32_t*);
using AegpRenderOptionsSetDownsample = int32_t(__cdecl*)(void*, int16_t, int16_t);
using AegpRenderOptionsGetDownsample = int32_t(__cdecl*)(void*, int16_t*, int16_t*);
using AegpRenderOptionsSetRoi = int32_t(__cdecl*)(void*, const AegpRect*);
using AegpRenderOptionsGetRoi = int32_t(__cdecl*)(void*, AegpRect*);
using AegpRenderOptionsSetI8 = int32_t(__cdecl*)(void*, int8_t);
using AegpRenderOptionsGetI8 = int32_t(__cdecl*)(void*, int8_t*);
using AegpRenderOptionsSetU8 = int32_t(__cdecl*)(void*, uint8_t);
using AegpRenderOptionsGetU8 = int32_t(__cdecl*)(void*, uint8_t*);

#define AEXCOMPAT_RENDER_OPTIONS_BASE_MEMBERS \
  AegpRenderOptionsNew new_from_item; \
  AegpRenderOptionsDuplicate duplicate; \
  AegpRenderOptionsDispose dispose; \
  AegpRenderOptionsSetTime set_time; \
  AegpRenderOptionsGetTime get_time; \
  AegpRenderOptionsSetTime set_time_step; \
  AegpRenderOptionsGetTime get_time_step; \
  AegpRenderOptionsSetI32 set_field; \
  AegpRenderOptionsGetI32 get_field; \
  AegpRenderOptionsSetI32 set_world_type; \
  AegpRenderOptionsGetI32 get_world_type; \
  AegpRenderOptionsSetDownsample set_downsample; \
  AegpRenderOptionsGetDownsample get_downsample; \
  AegpRenderOptionsSetRoi set_roi; \
  AegpRenderOptionsGetRoi get_roi; \
  AegpRenderOptionsSetI32 set_matte; \
  AegpRenderOptionsGetI32 get_matte

struct AegpRenderOptionsSuite1 { AEXCOMPAT_RENDER_OPTIONS_BASE_MEMBERS; };
struct AegpRenderOptionsSuite4 {
  AEXCOMPAT_RENDER_OPTIONS_BASE_MEMBERS;
  AegpRenderOptionsSetI8 set_channel_order;
  AegpRenderOptionsGetI8 get_channel_order;
  AegpRenderOptionsGetU8 get_guide_layers;
  AegpRenderOptionsSetU8 set_guide_layers;
  AegpRenderOptionsGetI8 get_quality;
  AegpRenderOptionsSetI8 set_quality;
};
#undef AEXCOMPAT_RENDER_OPTIONS_BASE_MEMBERS

#define AEXCOMPAT_ASSERT_SUITE_SLOT(Suite, Member, Slot) \
  static_assert(offsetof(Suite, Member) == (Slot) * sizeof(void*))
#define AEXCOMPAT_ASSERT_LAYER1_SLOT(Member, Slot) \
  AEXCOMPAT_ASSERT_SUITE_SLOT(AegpLayerRenderOptionsSuite1, Member, Slot)
#define AEXCOMPAT_ASSERT_LAYER2_SLOT(Member, Slot) \
  AEXCOMPAT_ASSERT_SUITE_SLOT(AegpLayerRenderOptionsSuite2, Member, Slot)
#define AEXCOMPAT_ASSERT_RENDER1_SLOT(Member, Slot) \
  AEXCOMPAT_ASSERT_SUITE_SLOT(AegpRenderOptionsSuite1, Member, Slot)
#define AEXCOMPAT_ASSERT_RENDER4_SLOT(Member, Slot) \
  AEXCOMPAT_ASSERT_SUITE_SLOT(AegpRenderOptionsSuite4, Member, Slot)

static_assert(std::is_standard_layout_v<AegpLayerRenderOptionsSuite1>);
static_assert(sizeof(AegpLayerRenderOptionsSuite1) == 14 * sizeof(void*));
static_assert(alignof(AegpLayerRenderOptionsSuite1) == alignof(void*));
AEXCOMPAT_ASSERT_LAYER1_SLOT(new_from_layer, 0); AEXCOMPAT_ASSERT_LAYER1_SLOT(new_from_upstream_of_effect, 1);
AEXCOMPAT_ASSERT_LAYER1_SLOT(duplicate, 2); AEXCOMPAT_ASSERT_LAYER1_SLOT(dispose, 3);
AEXCOMPAT_ASSERT_LAYER1_SLOT(set_time, 4); AEXCOMPAT_ASSERT_LAYER1_SLOT(get_time, 5);
AEXCOMPAT_ASSERT_LAYER1_SLOT(set_time_step, 6); AEXCOMPAT_ASSERT_LAYER1_SLOT(get_time_step, 7);
AEXCOMPAT_ASSERT_LAYER1_SLOT(set_world_type, 8); AEXCOMPAT_ASSERT_LAYER1_SLOT(get_world_type, 9);
AEXCOMPAT_ASSERT_LAYER1_SLOT(set_downsample, 10); AEXCOMPAT_ASSERT_LAYER1_SLOT(get_downsample, 11);
AEXCOMPAT_ASSERT_LAYER1_SLOT(set_matte, 12); AEXCOMPAT_ASSERT_LAYER1_SLOT(get_matte, 13);

static_assert(std::is_standard_layout_v<AegpLayerRenderOptionsSuite2>);
static_assert(sizeof(AegpLayerRenderOptionsSuite2) == 15 * sizeof(void*));
static_assert(alignof(AegpLayerRenderOptionsSuite2) == alignof(void*));
AEXCOMPAT_ASSERT_LAYER2_SLOT(new_from_layer, 0); AEXCOMPAT_ASSERT_LAYER2_SLOT(new_from_upstream_of_effect, 1);
AEXCOMPAT_ASSERT_LAYER2_SLOT(new_from_downstream_of_effect, 2); AEXCOMPAT_ASSERT_LAYER2_SLOT(duplicate, 3);
AEXCOMPAT_ASSERT_LAYER2_SLOT(dispose, 4); AEXCOMPAT_ASSERT_LAYER2_SLOT(set_time, 5);
AEXCOMPAT_ASSERT_LAYER2_SLOT(get_time, 6); AEXCOMPAT_ASSERT_LAYER2_SLOT(set_time_step, 7);
AEXCOMPAT_ASSERT_LAYER2_SLOT(get_time_step, 8); AEXCOMPAT_ASSERT_LAYER2_SLOT(set_world_type, 9);
AEXCOMPAT_ASSERT_LAYER2_SLOT(get_world_type, 10); AEXCOMPAT_ASSERT_LAYER2_SLOT(set_downsample, 11);
AEXCOMPAT_ASSERT_LAYER2_SLOT(get_downsample, 12); AEXCOMPAT_ASSERT_LAYER2_SLOT(set_matte, 13);
AEXCOMPAT_ASSERT_LAYER2_SLOT(get_matte, 14);

static_assert(std::is_standard_layout_v<AegpRenderOptionsSuite1>);
static_assert(sizeof(AegpRenderOptionsSuite1) == 17 * sizeof(void*));
static_assert(alignof(AegpRenderOptionsSuite1) == alignof(void*));
AEXCOMPAT_ASSERT_RENDER1_SLOT(new_from_item, 0); AEXCOMPAT_ASSERT_RENDER1_SLOT(duplicate, 1);
AEXCOMPAT_ASSERT_RENDER1_SLOT(dispose, 2); AEXCOMPAT_ASSERT_RENDER1_SLOT(set_time, 3);
AEXCOMPAT_ASSERT_RENDER1_SLOT(get_time, 4); AEXCOMPAT_ASSERT_RENDER1_SLOT(set_time_step, 5);
AEXCOMPAT_ASSERT_RENDER1_SLOT(get_time_step, 6); AEXCOMPAT_ASSERT_RENDER1_SLOT(set_field, 7);
AEXCOMPAT_ASSERT_RENDER1_SLOT(get_field, 8); AEXCOMPAT_ASSERT_RENDER1_SLOT(set_world_type, 9);
AEXCOMPAT_ASSERT_RENDER1_SLOT(get_world_type, 10); AEXCOMPAT_ASSERT_RENDER1_SLOT(set_downsample, 11);
AEXCOMPAT_ASSERT_RENDER1_SLOT(get_downsample, 12); AEXCOMPAT_ASSERT_RENDER1_SLOT(set_roi, 13);
AEXCOMPAT_ASSERT_RENDER1_SLOT(get_roi, 14); AEXCOMPAT_ASSERT_RENDER1_SLOT(set_matte, 15);
AEXCOMPAT_ASSERT_RENDER1_SLOT(get_matte, 16);

static_assert(std::is_standard_layout_v<AegpRenderOptionsSuite4>);
static_assert(sizeof(AegpRenderOptionsSuite4) == 23 * sizeof(void*));
static_assert(alignof(AegpRenderOptionsSuite4) == alignof(void*));
#define AEXCOMPAT_ASSERT_RENDER4_BASE(Member, Slot) AEXCOMPAT_ASSERT_RENDER4_SLOT(Member, Slot)
AEXCOMPAT_ASSERT_RENDER4_BASE(new_from_item, 0); AEXCOMPAT_ASSERT_RENDER4_BASE(duplicate, 1);
AEXCOMPAT_ASSERT_RENDER4_BASE(dispose, 2); AEXCOMPAT_ASSERT_RENDER4_BASE(set_time, 3);
AEXCOMPAT_ASSERT_RENDER4_BASE(get_time, 4); AEXCOMPAT_ASSERT_RENDER4_BASE(set_time_step, 5);
AEXCOMPAT_ASSERT_RENDER4_BASE(get_time_step, 6); AEXCOMPAT_ASSERT_RENDER4_BASE(set_field, 7);
AEXCOMPAT_ASSERT_RENDER4_BASE(get_field, 8); AEXCOMPAT_ASSERT_RENDER4_BASE(set_world_type, 9);
AEXCOMPAT_ASSERT_RENDER4_BASE(get_world_type, 10); AEXCOMPAT_ASSERT_RENDER4_BASE(set_downsample, 11);
AEXCOMPAT_ASSERT_RENDER4_BASE(get_downsample, 12); AEXCOMPAT_ASSERT_RENDER4_BASE(set_roi, 13);
AEXCOMPAT_ASSERT_RENDER4_BASE(get_roi, 14); AEXCOMPAT_ASSERT_RENDER4_BASE(set_matte, 15);
AEXCOMPAT_ASSERT_RENDER4_BASE(get_matte, 16);
AEXCOMPAT_ASSERT_RENDER4_SLOT(set_channel_order, 17); AEXCOMPAT_ASSERT_RENDER4_SLOT(get_channel_order, 18);
AEXCOMPAT_ASSERT_RENDER4_SLOT(get_guide_layers, 19); AEXCOMPAT_ASSERT_RENDER4_SLOT(set_guide_layers, 20);
AEXCOMPAT_ASSERT_RENDER4_SLOT(get_quality, 21); AEXCOMPAT_ASSERT_RENDER4_SLOT(set_quality, 22);
#undef AEXCOMPAT_ASSERT_RENDER4_BASE
#undef AEXCOMPAT_ASSERT_RENDER4_SLOT
#undef AEXCOMPAT_ASSERT_RENDER1_SLOT
#undef AEXCOMPAT_ASSERT_LAYER2_SLOT
#undef AEXCOMPAT_ASSERT_LAYER1_SLOT
#undef AEXCOMPAT_ASSERT_SUITE_SLOT

AegpLayerRenderOptionsSuite1& aegp_layer_render_options_suite1_table() noexcept;
AegpLayerRenderOptionsSuite2& aegp_layer_render_options_suite2_table() noexcept;
AegpRenderOptionsSuite1& aegp_render_options_suite1_table() noexcept;
AegpRenderOptionsSuite4& aegp_render_options_suite4_table() noexcept;

using AegpWorldNew = int32_t(__cdecl*)(int32_t, int32_t, int32_t, int32_t, void***);
using AegpWorldDispose = int32_t(__cdecl*)(void**);
using AegpWorldGetType = int32_t(__cdecl*)(void**, int32_t*);
using AegpWorldGetSize = int32_t(__cdecl*)(void**, int32_t*, int32_t*);
using AegpWorldGetRowbytes = int32_t(__cdecl*)(void**, uint32_t*);
using AegpWorldGetBaseAddress8 = int32_t(__cdecl*)(void**, void**);
using AegpWorldGetBaseAddress16 = int32_t(__cdecl*)(void**, void**);
using AegpWorldGetBaseAddress32 = int32_t(__cdecl*)(void**, void**);
using AegpWorldFillPfWorld = int32_t(__cdecl*)(void**, void*);
using AegpWorldFastBlur = int32_t(__cdecl*)(double, uint32_t, int32_t, void**);
using AegpWorldNewPlatform = int32_t(__cdecl*)(int32_t, int32_t, int32_t, int32_t, void**);
using AegpWorldDisposePlatform = int32_t(__cdecl*)(void*);
using AegpWorldReferencePlatform = int32_t(__cdecl*)(int32_t, void*, void***);

struct AegpWorldSuite3 {
  AegpWorldNew new_world;
  AegpWorldDispose dispose;
  AegpWorldGetType get_type;
  AegpWorldGetSize get_size;
  AegpWorldGetRowbytes get_rowbytes;
  AegpWorldGetBaseAddress8 get_base_addr8;
  AegpWorldGetBaseAddress16 get_base_addr16;
  AegpWorldGetBaseAddress32 get_base_addr32;
  AegpWorldFillPfWorld fill_pf_world;
  AegpWorldFastBlur fast_blur;
  AegpWorldNewPlatform new_platform_world;
  AegpWorldDisposePlatform dispose_platform_world;
  AegpWorldReferencePlatform reference_platform_world;
};

static_assert(std::is_standard_layout_v<AegpWorldSuite3>);
static_assert(sizeof(AegpWorldSuite3) == 13 * sizeof(void*));
static_assert(alignof(AegpWorldSuite3) == alignof(void*));
static_assert(offsetof(AegpWorldSuite3, new_world) == 0 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, dispose) == 1 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, get_type) == 2 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, get_size) == 3 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, get_rowbytes) == 4 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, get_base_addr8) == 5 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, get_base_addr16) == 6 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, get_base_addr32) == 7 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, fill_pf_world) == 8 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, fast_blur) == 9 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, new_platform_world) == 10 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, dispose_platform_world) == 11 * sizeof(void*));
static_assert(offsetof(AegpWorldSuite3, reference_platform_world) == 12 * sizeof(void*));

// The table storage lives outside the worker entry translation unit so its ABI
// layout is compiled and owned independently of selector and suite dispatch.
AegpWorldSuite3& aegp_world_suite3_table() noexcept;

}  // namespace aexcompat::suite_abi
