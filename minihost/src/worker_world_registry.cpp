#include "worker_world_registry.hpp"

#include "gpu_memory_world_transport.hpp"

#include <array>
#include <cstring>
#include <limits>
#include <mutex>
#include <new>
#include <unordered_map>

namespace aexcompat::world_registry {
namespace {

constexpr uint64_t kMaxWorldBytes = 256ULL * 1024 * 1024;
constexpr std::size_t kMaxWorldCount = 64;

struct OwnedWorld {
  void* pixels{};
  uint64_t size{};
  int32_t pixel_format{};
};

std::mutex g_mutex;
std::unordered_map<void*, OwnedWorld> g_worlds;
uint64_t g_created{};
uint64_t g_disposed{};
uint64_t g_invalid_operations{};
uint64_t g_live_bytes{};

int32_t bytes_per_pixel(int32_t pixel_format) {
  if (pixel_format == kPixelFormatArgb32) return 4;
  if (pixel_format == kPixelFormatArgb64) return 8;
  if (pixel_format == kPixelFormatArgb128 ||
      pixel_format == kPixelFormatGpuBgra128) return 16;
  return 0;
}

}  // namespace

world_safety::OwnedWorldResolution resolve_owned_world(
    const void* world, void* data, int32_t rowbytes, int32_t width,
    int32_t height, world_safety::DispatchWorldFormat& result) {
  using world_safety::OwnedWorldResolution;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto exact = g_worlds.find(const_cast<void*>(world));
  if (exact != g_worlds.end()) {
    if (exact->second.pixels != data) return OwnedWorldResolution::rejected;
    result = {world, data, width, height, rowbytes, exact->second.pixel_format, 0};
    return OwnedWorldResolution::resolved;
  }

  const OwnedWorld* unique_owned = nullptr;
  const void* unique_world = nullptr;
  for (const auto& candidate : g_worlds) {
    if (candidate.second.pixels != data) continue;
    const auto* candidate_bytes = static_cast<const std::byte*>(candidate.first);
    int32_t candidate_rowbytes{}, candidate_width{}, candidate_height{};
    std::memcpy(&candidate_rowbytes, candidate_bytes + 32,
                sizeof(candidate_rowbytes));
    std::memcpy(&candidate_width, candidate_bytes + 36,
                sizeof(candidate_width));
    std::memcpy(&candidate_height, candidate_bytes + 40,
                sizeof(candidate_height));
    if (candidate_rowbytes != rowbytes || candidate_width != width ||
        candidate_height != height) continue;
    if (unique_owned) return OwnedWorldResolution::rejected;
    unique_owned = &candidate.second;
    unique_world = candidate.first;
  }
  if (!unique_owned) return OwnedWorldResolution::not_owned;
  result = {unique_world, data, width, height, rowbytes,
            unique_owned->pixel_format, 0};
  return OwnedWorldResolution::resolved;
}

bool resolve_dispatch_world_format(
    const void* world, world_safety::DispatchWorldFormat& result) {
  return world_safety::resolve_dispatch_world_format(
      world, &resolve_owned_world, result);
}

int32_t __cdecl new_world(void*, int32_t width, int32_t height,
                          int32_t clear_pixels, int32_t pixel_format,
                          void* world) {
  std::lock_guard<std::mutex> lock(g_mutex);
  const int32_t pixel_bytes = bytes_per_pixel(pixel_format);
  if (!world || width <= 0 || height <= 0 || pixel_bytes == 0 ||
      g_worlds.count(world) || g_worlds.size() >= kMaxWorldCount) {
    ++g_invalid_operations;
    return 4;
  }
  const uint64_t rowbytes64 = static_cast<uint64_t>(width) * pixel_bytes;
  const uint64_t size = rowbytes64 * static_cast<uint64_t>(height);
  if (rowbytes64 > static_cast<uint64_t>((std::numeric_limits<int32_t>::max)()) ||
      size > kMaxWorldBytes || g_live_bytes > kMaxWorldBytes - size) {
    ++g_invalid_operations;
    return 4;
  }
  void* pixels = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!pixels) return 1;
  std::memset(pixels, clear_pixels ? 0 : 0xcd, static_cast<std::size_t>(size));
  std::memset(world, 0, world_safety::kEffectWorldSize);
  auto* bytes = static_cast<std::byte*>(world);
  const int32_t flags = 2 | (pixel_format == kPixelFormatArgb32 ? 0 : 1);
  const int32_t rowbytes = static_cast<int32_t>(rowbytes64);
  const std::array<int32_t, 4> extent{0, 0, width, height};
  const int32_t aspect_num = 1;
  const uint32_t aspect_den = 1;
  std::memcpy(bytes + 16, &flags, sizeof(flags));
  std::memcpy(bytes + 24, &pixels, sizeof(pixels));
  std::memcpy(bytes + 32, &rowbytes, sizeof(rowbytes));
  std::memcpy(bytes + 36, &width, sizeof(width));
  std::memcpy(bytes + 40, &height, sizeof(height));
  std::memcpy(bytes + 44, extent.data(), sizeof(extent));
  std::memcpy(bytes + 88, &aspect_num, sizeof(aspect_num));
  std::memcpy(bytes + 92, &aspect_den, sizeof(aspect_den));
  g_worlds.emplace(world, OwnedWorld{pixels, size, pixel_format});
  ++g_created;
  g_live_bytes += size;
  return 0;
}

int32_t __cdecl legacy_new_world(void* effect_ref, int32_t width,
                                 int32_t height, int32_t flags, void* world) {
  if ((flags & ~3) != 0) return 4;
  return new_world(effect_ref, width, height, flags & 1,
                   (flags & 2) ? kPixelFormatArgb64 : kPixelFormatArgb32,
                   world);
}

int32_t __cdecl dispose_world(void*, void* world) {
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_worlds.find(world);
  if (!world || found == g_worlds.end()) {
    ++g_invalid_operations;
    return 4;
  }
  ::operator delete(found->second.pixels);
  g_live_bytes -= found->second.size;
  g_worlds.erase(found);
  ++g_disposed;
  std::memset(world, 0, world_safety::kEffectWorldSize);
  return 0;
}

int32_t __cdecl get_pixel_format(const void* world, int32_t* pixel_format) {
  if (!world || !pixel_format) return 4;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    const auto found = g_worlds.find(const_cast<void*>(world));
    if (found != g_worlds.end()) {
      *pixel_format = found->second.pixel_format;
      return 0;
    }
  }
  world_safety::DispatchWorldFormat resolved{};
  if (!resolve_dispatch_world_format(world, resolved)) return 4;
  *pixel_format = resolved.pixel_format;
  return 0;
}

bool owns_world(void* world) {
  std::lock_guard<std::mutex> lock(g_mutex);
  return world && g_worlds.count(world) != 0;
}

bool owned_world_matches(void* world, int32_t pixel_format) {
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_worlds.find(world);
  return found != g_worlds.end() && found->second.pixel_format == pixel_format;
}

bool snapshot_owned_world(void* world, OwnedWorldSnapshot& snapshot) {
  snapshot = {};
  if (!world) return false;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_worlds.find(world);
  if (found == g_worlds.end() || !found->second.pixels) return false;
  world_safety::LocalEffectWorld descriptor{};
  std::memcpy(&descriptor, world, sizeof(descriptor));
  if (descriptor.data != found->second.pixels) return false;
  snapshot.world = descriptor;
  snapshot.pixel_format = found->second.pixel_format;
  return true;
}

bool lifetimes_balanced() {
  std::lock_guard<std::mutex> lock(g_mutex);
  return g_worlds.empty() && g_created == g_disposed && g_live_bytes == 0;
}

Statistics statistics() {
  std::lock_guard<std::mutex> lock(g_mutex);
  return {g_created, g_disposed, g_invalid_operations, g_worlds.size(),
          g_live_bytes};
}

void configure_gpu_fallback_bridge(
    RecognizesSmartWorld recognizes_smart_world) {
  gpu_runtime::memory_world_transport::configure_host_world_fallback(
      &new_world, &dispose_world, &owns_world, recognizes_smart_world);
}

}  // namespace aexcompat::world_registry
