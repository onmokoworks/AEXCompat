#pragma once

#include "worker_world_safety.hpp"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <mutex>
#include <vector>

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

struct OwnedWorldSnapshot {
  world_safety::LocalEffectWorld world{};
  int32_t pixel_format{};
};

struct PlatformWorldBacking {
  world_safety::LocalEffectWorld world{};
  std::vector<std::byte> pixels;
  int32_t pixel_format{};
  uint64_t accounted_bytes{};
  bool counted_owned{};
  std::mutex pixels_mutex;
};

struct AegpStatistics {
  uint64_t platform_created{};
  uint64_t platform_disposed{};
  uint64_t platform_adopted{};
  uint64_t references_created{};
  uint64_t references_disposed{};
  uint64_t owned_created{};
  uint64_t owned_disposed{};
  std::size_t live_references{};
  std::size_t live_owned{};
  std::size_t live_owned_backings{};
  std::size_t live_platforms{};
  uint64_t live_bytes{};
};

struct AegpWorldSnapshot {
  world_safety::LocalEffectWorld world{};
  int32_t pixel_format{};
  bool disposable{};
  std::shared_ptr<PlatformWorldBacking> backing_pin;
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
// Base-pointer membership across every host-issued pixel allocation this
// registry tracks (PF_NEW_WORLD worlds plus AEGP platform/owned backings);
// the copy callbacks' foreign-operand fallback gate.
bool hosts_world_pixels(void* world);
bool owned_world_matches(void* world, int32_t pixel_format);
bool snapshot_owned_world(void* world, OwnedWorldSnapshot& snapshot);
bool lifetimes_balanced();
Statistics statistics();

using RecognizesSmartWorld = bool (*)(void*);
void configure_gpu_fallback_bridge(RecognizesSmartWorld recognizes_smart_world);

int32_t aegp_world_type_from_format(int32_t pixel_format);
bool register_borrowed_view(void** handle, void* pf_world, int32_t pixel_format,
                            bool borrowed = true);
enum class UnregisterBorrowedViewResult {
  removed,
  already_absent,
  ownership_mismatch,
};
UnregisterBorrowedViewResult unregister_borrowed_view(void** handle);
bool snapshot_aegp_world(void** handle, AegpWorldSnapshot& snapshot);
bool snapshot_platform_world(void* handle,
                             std::shared_ptr<PlatformWorldBacking>& backing);
bool adopt_platform_world(void* handle,
                          std::shared_ptr<PlatformWorldBacking>& backing);
AegpStatistics aegp_statistics();
bool aegp_lifetimes_balanced();

int32_t __cdecl aegp_world_new_owned(int32_t, int32_t, int32_t, int32_t,
                                     void***);
int32_t __cdecl aegp_world_dispose(void**);
int32_t __cdecl aegp_world_get_type(void**, int32_t*);
int32_t __cdecl aegp_world_get_size(void**, int32_t*, int32_t*);
int32_t __cdecl aegp_world_get_rowbytes(void**, uint32_t*);
int32_t __cdecl aegp_world_get_base_addr8(void**, void**);
int32_t __cdecl aegp_world_get_base_addr16(void**, void**);
int32_t __cdecl aegp_world_get_base_addr32(void**, void**);
int32_t __cdecl aegp_world_fill_pf_world(void**, void*);
int32_t __cdecl aegp_world_fast_blur(double, uint32_t, int32_t, void**);
int32_t __cdecl aegp_world_new_platform(int32_t, int32_t, int32_t, int32_t,
                                        void**);
int32_t __cdecl aegp_world_dispose_platform(void*);
int32_t __cdecl aegp_world_reference_platform(int32_t, void*, void***);

}  // namespace aexcompat::world_registry
