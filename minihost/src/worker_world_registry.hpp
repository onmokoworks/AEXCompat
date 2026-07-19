#pragma once

#include "worker_world_safety.hpp"

#include <cstddef>
#include <cstdint>

namespace aexcompat::world_registry {

inline constexpr int32_t kPixelFormatArgb32 = 1650946657;
inline constexpr int32_t kPixelFormatArgb64 = 909206881;
inline constexpr int32_t kPixelFormatArgb128 = 842229089;
inline constexpr int32_t kPixelFormatGpuBgra128 = 1094992704;

struct Statistics {
  uint64_t created{};
  uint64_t disposed{};
  uint64_t invalid_operations{};
  std::size_t live_count{};
  uint64_t live_bytes{};
};

int32_t __cdecl new_world(void*, int32_t width, int32_t height,
                          int32_t clear_pixels, int32_t pixel_format,
                          void* world);
int32_t __cdecl legacy_new_world(void*, int32_t width, int32_t height,
                                 int32_t flags, void* world);
int32_t __cdecl dispose_world(void*, void* world);
int32_t __cdecl get_pixel_format(const void* world, int32_t* pixel_format);

world_safety::OwnedWorldResolution resolve_owned_world(
    const void* world, void* data, int32_t rowbytes, int32_t width,
    int32_t height, world_safety::DispatchWorldFormat& result);
bool resolve_dispatch_world_format(
    const void* world, world_safety::DispatchWorldFormat& result);

bool owns_world(void* world);
bool owned_world_matches(void* world, int32_t pixel_format);
bool lifetimes_balanced();
Statistics statistics();

using RecognizesSmartWorld = bool (*)(void*);
void configure_gpu_fallback_bridge(RecognizesSmartWorld recognizes_smart_world);

}  // namespace aexcompat::world_registry
