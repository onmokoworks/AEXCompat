#include "worker_world_safety.hpp"

#include <algorithm>
#include <cstring>
#include <vector>

namespace aexcompat::world_safety {
namespace {

thread_local std::vector<std::vector<DispatchWorldFormat>> g_dispatch_world_formats;
thread_local uint64_t g_dispatch_world_generation{};

bool read_world_layout(const void* world, void*& data, int32_t& rowbytes,
                       int32_t& width, int32_t& height) {
  if (!world) return false;
  const auto* bytes = static_cast<const std::byte*>(world);
  std::memcpy(&data, bytes + 24, sizeof(data));
  std::memcpy(&rowbytes, bytes + 32, sizeof(rowbytes));
  std::memcpy(&width, bytes + 36, sizeof(width));
  std::memcpy(&height, bytes + 40, sizeof(height));
  return true;
}

}  // namespace

DispatchWorldFormatScope::DispatchWorldFormatScope() {
  g_dispatch_world_formats.emplace_back();
}

DispatchWorldFormatScope::~DispatchWorldFormatScope() {
  g_dispatch_world_formats.pop_back();
}

bool DispatchWorldFormatScope::register_world(const void* world,
                                               int32_t pixel_format) {
  if (!world || g_dispatch_world_formats.empty()) return false;
  DispatchWorldFormat entry{};
  entry.world = world;
  entry.pixel_format = pixel_format;
  entry.generation = ++g_dispatch_world_generation;
  if (!read_world_layout(world, entry.data, entry.rowbytes, entry.width, entry.height) ||
      !entry.data || entry.width <= 0 || entry.height <= 0 || entry.rowbytes <= 0)
    return false;
  auto& entries = g_dispatch_world_formats.back();
  entries.erase(std::remove_if(entries.begin(), entries.end(),
                               [&](const auto& old) { return old.world == world; }),
                entries.end());
  entries.push_back(entry);
  return true;
}

bool DispatchWorldFormatScope::register_gpu_world(const void* world,
                                                  int32_t pixel_format) {
  if (!world || g_dispatch_world_formats.empty()) return false;
  DispatchWorldFormat entry{};
  entry.world = world;
  entry.pixel_format = pixel_format;
  entry.generation = ++g_dispatch_world_generation;
  void* platform_ref{};
  std::memcpy(&platform_ref, static_cast<const std::byte*>(world) + 64,
              sizeof(platform_ref));
  if (!read_world_layout(world, entry.data, entry.rowbytes, entry.width,
                         entry.height) || entry.data || !platform_ref ||
      entry.width <= 0 || entry.height <= 0 || entry.rowbytes < 0)
    return false;
  auto& entries = g_dispatch_world_formats.back();
  entries.erase(std::remove_if(entries.begin(), entries.end(),
                               [&](const auto& old) { return old.world == world; }),
                entries.end());
  entries.push_back(entry);
  return true;
}

bool resolve_dispatch_world_format(const void* world,
                                   OwnedWorldResolver owned_resolver,
                                   DispatchWorldFormat& result) {
  void* data{};
  int32_t rowbytes{}, width{}, height{};
  if (!read_world_layout(world, data, rowbytes, width, height) || !owned_resolver)
    return false;

  const auto owned = owned_resolver(world, data, rowbytes, width, height, result);
  if (owned == OwnedWorldResolution::resolved) {
    result.generation = g_dispatch_world_generation;
    return true;
  }
  if (owned == OwnedWorldResolution::rejected) return false;

  for (auto scope = g_dispatch_world_formats.rbegin();
       scope != g_dispatch_world_formats.rend(); ++scope) {
    for (auto entry = scope->rbegin(); entry != scope->rend(); ++entry) {
      if (entry->world == world) {
        if (entry->data != data || entry->rowbytes != rowbytes ||
            entry->width != width || entry->height != height)
          return false;
        result = *entry;
        return true;
      }
    }
  }
  const DispatchWorldFormat* unique = nullptr;
  for (auto scope = g_dispatch_world_formats.rbegin();
       scope != g_dispatch_world_formats.rend(); ++scope) {
    for (const auto& entry : *scope) {
      if (entry.data == data && entry.rowbytes == rowbytes &&
          entry.width == width && entry.height == height) {
        if (unique && (unique->world != entry.world ||
                       unique->pixel_format != entry.pixel_format))
          return false;
        unique = &entry;
      }
    }
  }
  if (!unique) return false;
  result = *unique;
  return true;
}

bool resolve_registered_dispatch_world(const void* world,
                                       DispatchWorldFormat& result) {
  void* data{};
  int32_t rowbytes{}, width{}, height{};
  if (!read_world_layout(world, data, rowbytes, width, height)) return false;
  for (auto scope = g_dispatch_world_formats.rbegin();
       scope != g_dispatch_world_formats.rend(); ++scope) {
    const auto match = std::find_if(scope->begin(), scope->end(),
                                    [&](const auto& entry) { return entry.world == world; });
    if (match != scope->end()) {
      if (data != match->data || rowbytes != match->rowbytes ||
          width != match->width || height != match->height)
        return false;
      result = *match;
      return true;
    }
  }
  return false;
}

bool bounded_typed_world(void* world, int32_t pixel_bytes,
                         unsigned char*& pixels, int32_t& rowbytes,
                         int32_t& width, int32_t& height) {
  if (!world) return false;
  auto* bytes = static_cast<std::byte*>(world);
  std::memcpy(&pixels, bytes + 24, sizeof(pixels));
  std::memcpy(&rowbytes, bytes + 32, sizeof(rowbytes));
  std::memcpy(&width, bytes + 36, sizeof(width));
  std::memcpy(&height, bytes + 40, sizeof(height));
  int32_t flags{};
  std::memcpy(&flags, bytes + 16, sizeof(flags));
  const bool depth_matches = pixel_bytes == 4 ? (flags & 1) == 0 : (flags & 1) != 0;
  return pixels && (pixel_bytes == 4 || pixel_bytes == 8 || pixel_bytes == 16) &&
      width > 0 && height > 0 && width <= 4096 && height <= 4096 &&
      static_cast<int64_t>(width) * height <= 16'777'216 &&
      rowbytes >= width * pixel_bytes && rowbytes <= 4096 * 16 && depth_matches;
}

bool bounded_argb8_world(void* world, unsigned char*& pixels, int32_t& rowbytes,
                         int32_t& width, int32_t& height) {
  return bounded_typed_world(world, 4, pixels, rowbytes, width, height);
}

}  // namespace aexcompat::world_safety
