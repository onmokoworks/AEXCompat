#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::world_safety {

inline constexpr std::size_t kEffectWorldSize = 120;

struct LocalRect { int32_t left, top, right, bottom; };
struct LocalRationalScale { int32_t num; uint32_t den; };
struct LocalEffectWorld {
  void* reserved0;
  void* reserved1;
  int32_t world_flags;
  void* data;
  int32_t rowbytes;
  int32_t width;
  int32_t height;
  LocalRect extent_hint;
  void* platform_ref;
  int32_t reserved_long1;
  void* reserved_long4;
  LocalRationalScale pix_aspect_ratio;
  void* reserved_long2;
  int32_t origin_x;
  int32_t origin_y;
  int32_t reserved_long3;
  int32_t dephault;
};

static_assert(sizeof(LocalEffectWorld) == kEffectWorldSize);
static_assert(offsetof(LocalEffectWorld, world_flags) == 16);
static_assert(offsetof(LocalEffectWorld, data) == 24);
static_assert(offsetof(LocalEffectWorld, rowbytes) == 32);
static_assert(offsetof(LocalEffectWorld, extent_hint) == 44);
static_assert(offsetof(LocalEffectWorld, pix_aspect_ratio) == 88);

struct DispatchWorldFormat {
  const void* world{};
  void* data{};
  int32_t width{};
  int32_t height{};
  int32_t rowbytes{};
  int32_t pixel_format{};
  uint64_t generation{};
};

enum class OwnedWorldResolution { not_owned, resolved, rejected };
using OwnedWorldResolver = OwnedWorldResolution (*)(
    const void* world, void* data, int32_t rowbytes, int32_t width,
    int32_t height, DispatchWorldFormat& result);

class DispatchWorldFormatScope {
 public:
  DispatchWorldFormatScope();
  DispatchWorldFormatScope(const DispatchWorldFormatScope&) = delete;
  DispatchWorldFormatScope& operator=(const DispatchWorldFormatScope&) = delete;
  ~DispatchWorldFormatScope();

  bool register_world(const void* world, int32_t pixel_format);
  bool register_gpu_world(const void* world, int32_t pixel_format);
};

bool resolve_dispatch_world_format(const void* world,
                                   OwnedWorldResolver owned_resolver,
                                   DispatchWorldFormat& result);
bool resolve_registered_dispatch_world(const void* world,
                                       DispatchWorldFormat& result);

bool bounded_typed_world(void* world, int32_t pixel_bytes,
                         unsigned char*& pixels, int32_t& rowbytes,
                         int32_t& width, int32_t& height);
bool bounded_argb8_world(void* world, unsigned char*& pixels,
                        int32_t& rowbytes, int32_t& width, int32_t& height);

}  // namespace aexcompat::world_safety
