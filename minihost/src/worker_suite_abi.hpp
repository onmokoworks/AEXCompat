#pragma once

#include <cstddef>
#include <cstdint>
#include <type_traits>

namespace aexcompat::suite_abi {

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
