#include "worker_world_registry.hpp"

#include "gpu_memory_world_transport.hpp"
#include "trace_writer.hpp"

#include <array>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstring>
#include <limits>
#include <mutex>
#include <new>
#include <unordered_map>
#include <utility>
#include <vector>

namespace aexcompat::l2_detail {
// Installed by WorkerSession for the session lifetime (defined in l2_main.cpp).
// World descriptors are a high-frequency detail event, so they are gated on the
// writer's verbose opt-in to protect the bounded event budget (issue #17).
extern aexcompat::TraceWriter* g_trace_writer;
}  // namespace aexcompat::l2_detail

namespace aexcompat::world_registry {
namespace {

// Map the internal pixel-format tag to a trace-contract pixel_format string
// (contracts/trace/host_trace_event.schema.json). Unrecognized tags fall back
// to "unknown" rather than emitting an out-of-contract value.
const char* trace_pixel_format(int32_t pixel_format) {
  if (pixel_format == kPixelFormatArgb32) return "argb8";
  if (pixel_format == kPixelFormatArgb64) return "argb16";
  if (pixel_format == kPixelFormatArgb128) return "argb32f";
  return "unknown";
}

constexpr uint64_t kMaxWorldBytes = 256ULL * 1024 * 1024;
constexpr std::size_t kMaxWorldCount = 64;

struct OwnedWorld {
  void* pixels{};
  uint64_t size{};
  int32_t pixel_format{};
};

struct AegpWorldView {
  void* pf_world{};
  int32_t pixel_format{};
  bool borrowed{};
  bool disposable{};
  bool owned_aegp{};
  std::shared_ptr<PlatformWorldBacking> platform_backing;
};

struct PlatformWorldEntry {
  std::shared_ptr<PlatformWorldBacking> backing;
};

std::mutex g_mutex;
std::unordered_map<void*, OwnedWorld> g_worlds;
uint64_t g_created{};
uint64_t g_disposed{};
uint64_t g_invalid_operations{};
uint64_t g_live_bytes{};
std::unordered_map<void**, AegpWorldView> g_aegp_views;
std::unordered_map<void*, PlatformWorldEntry> g_platform_worlds;
std::atomic<uint64_t> g_platform_world_generation{1};
std::atomic<uint64_t> g_platform_reference_generation{1};
std::atomic<uint64_t> g_owned_aegp_world_generation{1};
std::atomic<uint64_t> g_platform_world_bytes{};
uint64_t g_platform_worlds_created{};
uint64_t g_platform_worlds_disposed{};
uint64_t g_platform_worlds_adopted{};
uint64_t g_platform_references_created{};
uint64_t g_platform_references_disposed{};
std::size_t g_live_platform_references{};
uint64_t g_owned_aegp_worlds_created{};
uint64_t g_owned_aegp_worlds_disposed{};
std::size_t g_live_owned_aegp_worlds{};
std::atomic<std::size_t> g_live_owned_aegp_backings{};
constexpr std::size_t kMaxPlatformWorlds = 32;
constexpr std::size_t kMaxPlatformReferences = 64;
constexpr std::size_t kMaxOwnedAegpWorlds = 64;
constexpr uint64_t kMaxPlatformWorldBytes = 64ULL * 1024 * 1024;

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
  if (aexcompat::l2_detail::g_trace_writer &&
      aexcompat::l2_detail::g_trace_writer->verbose()) {
    aexcompat::l2_detail::g_trace_writer->world_descriptor(
        width, height, rowbytes, trace_pixel_format(pixel_format));
  }
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

bool claim_opaque_generation(std::atomic<uint64_t>& counter,
                             uint64_t& generation) {
  const uint64_t limit = (std::numeric_limits<uintptr_t>::max)() / 8;
  uint64_t current = counter.load();
  for (;;) {
    if (current == 0 || current > limit) return false;
    const uint64_t next = current == limit ? limit + 1 : current + 1;
    if (counter.compare_exchange_weak(current, next)) {
      generation = current;
      return true;
    }
  }
}

std::shared_ptr<PlatformWorldBacking> allocate_platform_backing(
    int32_t type, int32_t pixel_format, int32_t width, int32_t height,
    uint64_t rowbytes, uint64_t size, bool counted_owned) {
  std::shared_ptr<PlatformWorldBacking> backing;
  try {
    auto* raw = new PlatformWorldBacking();
    backing = std::shared_ptr<PlatformWorldBacking>(
        raw, [](PlatformWorldBacking* value) {
          if (value->accounted_bytes)
            g_platform_world_bytes.fetch_sub(value->accounted_bytes);
          if (value->counted_owned)
            g_live_owned_aegp_backings.fetch_sub(1);
          delete value;
        });
    backing->pixels.resize(static_cast<std::size_t>(size));
  } catch (...) {
    return {};
  }
  std::memset(backing->pixels.data(), 0, backing->pixels.size());
  backing->pixel_format = pixel_format;
  backing->world.world_flags = 2 | (type == 1 ? 0 : 1);
  backing->world.data = backing->pixels.data();
  backing->world.rowbytes = static_cast<int32_t>(rowbytes);
  backing->world.width = width;
  backing->world.height = height;
  backing->world.extent_hint = {0, 0, width, height};
  backing->world.pix_aspect_ratio = {1, 1};
  g_platform_world_bytes.fetch_add(size);
  backing->accounted_bytes = size;
  backing->counted_owned = counted_owned;
  if (counted_owned) g_live_owned_aegp_backings.fetch_add(1);
  return backing;
}

bool snapshot_aegp_view(void** handle, AegpWorldView& view,
                        world_safety::LocalEffectWorld& world) {
  if (!handle) return false;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_aegp_views.find(handle);
  if (found == g_aegp_views.end() || !found->second.pf_world) return false;
  if (!found->second.borrowed &&
      (!*handle || *handle != found->second.pf_world)) return false;
  if (!found->second.borrowed) {
    const auto owned = g_worlds.find(found->second.pf_world);
    if (owned == g_worlds.end() ||
        owned->second.pixel_format != found->second.pixel_format) return false;
    std::memcpy(&world, found->second.pf_world, sizeof(world));
    if (world.data != owned->second.pixels) return false;
  } else {
    std::memcpy(&world, found->second.pf_world, sizeof(world));
  }
  if (!world.data || world.width <= 0 || world.height <= 0 ||
      world.rowbytes <= 0) return false;
  const int32_t type = aegp_world_type_from_format(found->second.pixel_format);
  const int32_t pixel_bytes = type == 1 ? 4 : (type == 2 ? 8 :
      (type == 3 ? 16 : 0));
  const int64_t minimum_rowbytes =
      static_cast<int64_t>(world.width) * pixel_bytes;
  if (!pixel_bytes || minimum_rowbytes >
          (std::numeric_limits<int32_t>::max)() ||
      world.rowbytes < minimum_rowbytes) return false;
  view = found->second;
  return true;
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

int32_t aegp_world_type_from_format(int32_t pixel_format) {
  if (pixel_format == kPixelFormatArgb32) return 1;
  if (pixel_format == kPixelFormatArgb64) return 2;
  if (pixel_format == kPixelFormatArgb128 ||
      pixel_format == kPixelFormatGpuBgra128) return 3;
  return 0;
}

bool register_borrowed_view(void** handle, void* pf_world,
                            int32_t pixel_format, bool borrowed) {
  if (!handle || !pf_world || !aegp_world_type_from_format(pixel_format))
    return false;
  std::lock_guard<std::mutex> lock(g_mutex);
  try {
    return g_aegp_views.emplace(handle, AegpWorldView{
        pf_world, pixel_format, borrowed}).second;
  } catch (...) {
    return false;
  }
}

UnregisterBorrowedViewResult unregister_borrowed_view(void** handle) {
  if (!handle) return UnregisterBorrowedViewResult::already_absent;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_aegp_views.find(handle);
  if (found == g_aegp_views.end())
    return UnregisterBorrowedViewResult::already_absent;
  if (found->second.disposable)
    return UnregisterBorrowedViewResult::ownership_mismatch;
  g_aegp_views.erase(found);
  return UnregisterBorrowedViewResult::removed;
}

bool snapshot_aegp_world(void** handle, AegpWorldSnapshot& snapshot) {
  snapshot = {};
  AegpWorldView view{};
  if (!snapshot_aegp_view(handle, view, snapshot.world)) return false;
  snapshot.pixel_format = view.pixel_format;
  snapshot.disposable = view.disposable;
  snapshot.backing_pin = std::move(view.platform_backing);
  return true;
}

bool snapshot_platform_world(
    void* handle, std::shared_ptr<PlatformWorldBacking>& backing) {
  backing.reset();
  if (!handle) return false;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_platform_worlds.find(handle);
  if (found == g_platform_worlds.end() || !found->second.backing) return false;
  backing = found->second.backing;
  return true;
}

bool adopt_platform_world(
    void* handle, std::shared_ptr<PlatformWorldBacking>& backing) {
  backing.reset();
  if (!handle) return false;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_platform_worlds.find(handle);
  if (found == g_platform_worlds.end() || !found->second.backing) return false;
  backing = found->second.backing;
  g_platform_worlds.erase(found);
  ++g_platform_worlds_adopted;
  return true;
}

AegpStatistics aegp_statistics() {
  std::lock_guard<std::mutex> lock(g_mutex);
  return {g_platform_worlds_created, g_platform_worlds_disposed,
          g_platform_worlds_adopted, g_platform_references_created,
          g_platform_references_disposed, g_owned_aegp_worlds_created,
          g_owned_aegp_worlds_disposed, g_live_platform_references,
          g_live_owned_aegp_worlds, g_live_owned_aegp_backings.load(),
          g_platform_worlds.size(),
          g_platform_world_bytes.load()};
}

bool aegp_lifetimes_balanced() {
  const auto stats = aegp_statistics();
  return stats.live_platforms == 0 && stats.live_bytes == 0 &&
      stats.platform_created == stats.platform_disposed + stats.platform_adopted &&
      stats.references_created == stats.references_disposed &&
      stats.owned_created == stats.owned_disposed &&
      stats.live_references == 0 && stats.live_owned == 0 &&
      stats.live_owned_backings == 0;
}

int32_t __cdecl aegp_world_new_owned(int32_t plugin_id, int32_t type,
                                     int32_t width, int32_t height,
                                     void*** output) {
  if (output) *output = nullptr;
  const int32_t pixel_format = type == 1 ? kPixelFormatArgb32 :
      (type == 2 ? kPixelFormatArgb64 :
       (type == 3 ? kPixelFormatArgb128 : 0));
  const uint64_t pixel_bytes = type == 1 ? 4 : (type == 2 ? 8 :
      (type == 3 ? 16 : 0));
  if (plugin_id != 1 || !output || !pixel_format || width <= 0 || height <= 0)
    return 4;
  const uint64_t rowbytes = static_cast<uint64_t>(width) * pixel_bytes;
  const uint64_t size = rowbytes * static_cast<uint64_t>(height);
  if (rowbytes > static_cast<uint64_t>((std::numeric_limits<int32_t>::max)()) ||
      size == 0 || size > kMaxPlatformWorldBytes) return 4;
  uint64_t generation = 0;
  if (!claim_opaque_generation(g_owned_aegp_world_generation, generation)) return 4;
  auto* handle = reinterpret_cast<void**>(
      static_cast<uintptr_t>((generation << 3) | 5));
  std::lock_guard<std::mutex> lock(g_mutex);
  if (g_live_owned_aegp_worlds >= kMaxOwnedAegpWorlds ||
      g_live_owned_aegp_backings.load() >= kMaxOwnedAegpWorlds ||
      g_platform_world_bytes.load() > kMaxPlatformWorldBytes - size) return 4;
  auto backing = allocate_platform_backing(
      type, pixel_format, width, height, rowbytes, size, true);
  if (!backing) return 4;
  try {
    if (!g_aegp_views.emplace(handle, AegpWorldView{
        &backing->world, pixel_format, true, true, true, backing}).second)
      return 4;
  } catch (...) {
    return 4;
  }
  ++g_live_owned_aegp_worlds;
  ++g_owned_aegp_worlds_created;
  *output = handle;
  return 0;
}

int32_t __cdecl aegp_world_dispose(void** handle) {
  if (!handle) return 4;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_aegp_views.find(handle);
  if (found == g_aegp_views.end() || !found->second.disposable) return 4;
  const bool owned = found->second.owned_aegp;
  g_aegp_views.erase(found);
  if (owned) {
    --g_live_owned_aegp_worlds;
    ++g_owned_aegp_worlds_disposed;
  } else {
    --g_live_platform_references;
    ++g_platform_references_disposed;
  }
  return 0;
}

int32_t __cdecl aegp_world_get_type(void** handle, int32_t* type) {
  AegpWorldView view{};
  world_safety::LocalEffectWorld world{};
  if (!type || !snapshot_aegp_view(handle, view, world)) return 4;
  *type = aegp_world_type_from_format(view.pixel_format);
  return *type ? 0 : 4;
}

int32_t __cdecl aegp_world_get_size(void** handle, int32_t* width,
                                    int32_t* height) {
  AegpWorldView view{};
  world_safety::LocalEffectWorld world{};
  if (!width || !height || !snapshot_aegp_view(handle, view, world)) return 4;
  *width = world.width;
  *height = world.height;
  return 0;
}

int32_t __cdecl aegp_world_get_rowbytes(void** handle, uint32_t* rowbytes) {
  AegpWorldView view{};
  world_safety::LocalEffectWorld world{};
  if (!rowbytes || !snapshot_aegp_view(handle, view, world)) return 4;
  *rowbytes = static_cast<uint32_t>(world.rowbytes);
  return 0;
}

int32_t aegp_world_get_base_addr(void** handle, int32_t required_type,
                                 void** base) {
  AegpWorldView view{};
  world_safety::LocalEffectWorld world{};
  if (!base || !snapshot_aegp_view(handle, view, world) ||
      aegp_world_type_from_format(view.pixel_format) != required_type) return 4;
  *base = world.data;
  return 0;
}

int32_t __cdecl aegp_world_get_base_addr8(void** handle, void** base) {
  return aegp_world_get_base_addr(handle, 1, base);
}
int32_t __cdecl aegp_world_get_base_addr16(void** handle, void** base) {
  return aegp_world_get_base_addr(handle, 2, base);
}
int32_t __cdecl aegp_world_get_base_addr32(void** handle, void** base) {
  return aegp_world_get_base_addr(handle, 3, base);
}

int32_t __cdecl aegp_world_fill_pf_world(void** handle, void* output) {
  AegpWorldView view{};
  world_safety::LocalEffectWorld world{};
  if (!output || !snapshot_aegp_view(handle, view, world)) return 4;
  std::memcpy(output, &world, sizeof(world));
  return 0;
}

int32_t __cdecl aegp_world_fast_blur(double radius, uint32_t mode_flags,
                                     int32_t quality, void** handle) {
  AegpWorldView view{};
  world_safety::LocalEffectWorld world{};
  if (!std::isfinite(radius) || radius < 0.0 || radius > 1024.0 ||
      (mode_flags & ~1U) != 0 || (quality != 0 && quality != 1) ||
      !snapshot_aegp_view(handle, view, world) || !view.disposable ||
      !view.platform_backing) return 4;
  if (radius == 0.0) return 0;
  // The registry pin is copied before its mutex is released. Never hold the
  // registry mutex while waiting for mutable pixel access.
  std::lock_guard<std::mutex> pixels_lock(view.platform_backing->pixels_mutex);
  const int32_t type = aegp_world_type_from_format(view.pixel_format);
  const uint64_t pixel_count = static_cast<uint64_t>(world.width) * world.height;
  constexpr uint64_t kMaxBlurScratchBytes = 128ULL * 1024 * 1024;
  if (!type || pixel_count == 0 ||
      pixel_count > kMaxBlurScratchBytes / (4 * sizeof(float))) return 4;
  const int32_t kernel_radius = static_cast<int32_t>(std::ceil(radius));
  std::vector<float> weights;
  std::vector<float> horizontal;
  try {
    weights.resize(static_cast<std::size_t>(kernel_radius) + 1);
    horizontal.resize(static_cast<std::size_t>(pixel_count) * 4);
  } catch (...) {
    return 4;
  }
  float total_weight = 0.0f;
  for (int32_t offset = 0; offset <= kernel_radius; ++offset) {
    const float weight = quality == 0 ? 1.0f :
        static_cast<float>((std::max)(0.0, radius + 1.0 - offset));
    weights[static_cast<std::size_t>(offset)] = weight;
    total_weight += offset == 0 ? weight : 2.0f * weight;
  }
  if (!(total_weight > 0.0f)) return 4;
  const bool straight_alpha = (mode_flags & 1U) != 0;
  const auto read_pixel = [&](int32_t x, int32_t y, float* channels) {
    const auto* row = static_cast<const std::byte*>(world.data) +
        static_cast<std::size_t>(y) * world.rowbytes;
    if (type == 1) {
      const auto* pixel = reinterpret_cast<const uint8_t*>(row) + x * 4;
      for (int channel = 0; channel < 4; ++channel)
        channels[channel] = pixel[channel] / 255.0f;
    } else if (type == 2) {
      const auto* pixel = reinterpret_cast<const uint16_t*>(row) + x * 4;
      for (int channel = 0; channel < 4; ++channel)
        channels[channel] = pixel[channel] / 32768.0f;
    } else {
      std::memcpy(channels, row + static_cast<std::size_t>(x) * 16, 16);
    }
    if (straight_alpha) {
      channels[1] *= channels[0];
      channels[2] *= channels[0];
      channels[3] *= channels[0];
    }
  };
  for (int32_t y = 0; y < world.height; ++y) {
    for (int32_t x = 0; x < world.width; ++x) {
      float sum[4]{};
      for (int32_t offset = -kernel_radius; offset <= kernel_radius; ++offset) {
        const float weight = weights[static_cast<std::size_t>(std::abs(offset))];
        const int32_t sample_x = (std::clamp)(x + offset, 0, world.width - 1);
        float sample[4]{};
        read_pixel(sample_x, y, sample);
        for (int channel = 0; channel < 4; ++channel)
          sum[channel] += sample[channel] * weight;
      }
      float* output = horizontal.data() +
          (static_cast<std::size_t>(y) * world.width + x) * 4;
      for (int channel = 0; channel < 4; ++channel)
        output[channel] = sum[channel] / total_weight;
    }
  }
  for (int32_t y = 0; y < world.height; ++y) {
    auto* row = static_cast<std::byte*>(world.data) +
        static_cast<std::size_t>(y) * world.rowbytes;
    for (int32_t x = 0; x < world.width; ++x) {
      float sum[4]{};
      for (int32_t offset = -kernel_radius; offset <= kernel_radius; ++offset) {
        const float weight = weights[static_cast<std::size_t>(std::abs(offset))];
        const int32_t sample_y = (std::clamp)(y + offset, 0, world.height - 1);
        const float* sample = horizontal.data() +
            (static_cast<std::size_t>(sample_y) * world.width + x) * 4;
        for (int channel = 0; channel < 4; ++channel)
          sum[channel] += sample[channel] * weight;
      }
      for (float& channel : sum) channel /= total_weight;
      if (straight_alpha) {
        if (sum[0] > 0.0f) {
          sum[1] /= sum[0];
          sum[2] /= sum[0];
          sum[3] /= sum[0];
        } else {
          sum[1] = sum[2] = sum[3] = 0.0f;
        }
      }
      if (type == 1) {
        auto* pixel = reinterpret_cast<uint8_t*>(row) + x * 4;
        for (int channel = 0; channel < 4; ++channel)
          pixel[channel] = static_cast<uint8_t>(std::lround(
              (std::clamp)(sum[channel], 0.0f, 1.0f) * 255.0f));
      } else if (type == 2) {
        auto* pixel = reinterpret_cast<uint16_t*>(row) + x * 4;
        for (int channel = 0; channel < 4; ++channel)
          pixel[channel] = static_cast<uint16_t>(std::lround(
              (std::clamp)(sum[channel], 0.0f, 1.0f) * 32768.0f));
      } else {
        std::memcpy(row + static_cast<std::size_t>(x) * 16, sum, 16);
      }
    }
  }
  return 0;
}

int32_t __cdecl aegp_world_new_platform(int32_t plugin_id, int32_t type,
                                        int32_t width, int32_t height,
                                        void** output) {
  if (output) *output = nullptr;
  const int32_t pixel_format = type == 1 ? kPixelFormatArgb32 :
      (type == 2 ? kPixelFormatArgb64 :
       (type == 3 ? kPixelFormatArgb128 : 0));
  const uint64_t pixel_bytes = type == 1 ? 4 : (type == 2 ? 8 :
      (type == 3 ? 16 : 0));
  if (plugin_id != 1 || !output || !pixel_format || width <= 0 || height <= 0)
    return 4;
  const uint64_t rowbytes = static_cast<uint64_t>(width) * pixel_bytes;
  const uint64_t size = rowbytes * static_cast<uint64_t>(height);
  if (rowbytes > static_cast<uint64_t>((std::numeric_limits<int32_t>::max)()) ||
      size == 0 || size > kMaxPlatformWorldBytes) return 4;
  uint64_t generation = 0;
  if (!claim_opaque_generation(g_platform_world_generation, generation)) return 4;
  auto* handle = reinterpret_cast<void*>(
      static_cast<uintptr_t>((generation << 3) | 2));
  std::lock_guard<std::mutex> lock(g_mutex);
  if (g_platform_worlds.size() >= kMaxPlatformWorlds ||
      g_platform_world_bytes.load() > kMaxPlatformWorldBytes - size) return 4;
  auto backing = allocate_platform_backing(
      type, pixel_format, width, height, rowbytes, size, false);
  if (!backing) return 1;
  try {
    if (!g_platform_worlds.emplace(handle, PlatformWorldEntry{backing}).second)
      return 4;
  } catch (...) {
    return 4;
  }
  ++g_platform_worlds_created;
  *output = handle;
  return 0;
}

int32_t __cdecl aegp_world_dispose_platform(void* handle) {
  if (!handle) return 4;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_platform_worlds.find(handle);
  if (found == g_platform_worlds.end()) return 4;
  g_platform_worlds.erase(found);
  ++g_platform_worlds_disposed;
  return 0;
}

int32_t __cdecl aegp_world_reference_platform(int32_t plugin_id,
                                              void* platform,
                                              void*** output) {
  if (output) *output = nullptr;
  if (plugin_id != 1 || !output) return 4;
  uint64_t generation = 0;
  if (!claim_opaque_generation(g_platform_reference_generation, generation))
    return 4;
  auto* handle = reinterpret_cast<void**>(
      static_cast<uintptr_t>((generation << 3) | 7));
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_platform_worlds.find(platform);
  if (!platform || found == g_platform_worlds.end() ||
      g_live_platform_references >= kMaxPlatformReferences) return 4;
  const auto& backing = found->second.backing;
  try {
    if (!g_aegp_views.emplace(handle, AegpWorldView{
        &backing->world, backing->pixel_format, true, true, false,
        backing}).second) return 4;
  } catch (...) {
    return 4;
  }
  ++g_live_platform_references;
  ++g_platform_references_created;
  *output = handle;
  return 0;
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
