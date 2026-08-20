#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace aexcompat::world_safety {

inline constexpr std::size_t kEffectWorldSize = 120;

// Storage of a world the host hands to a plug-in, in AE's shape: the 120-byte
// PF_LayerDef preceded by the 8-byte vtable slot of the PF_World object AE
// embeds it in (worker_pf_world_facade, issue #1276). `data()` / `size()` /
// `fill()` are the LayerDef, exactly what the std::array<std::byte, 120> this
// replaces offered, so a caller that hands `.data()` to the plug-in or copies
// `.size()` bytes into a PF_ParamDef keeps doing that; `pf_world_vtable` is
// what a plug-in computing `world - 8` finds, and the LayerDef's
// reserved_long4 (+0x50) points at it once `pf_world_facade::embed` (called by
// render::prepare_world_layout) has run. Copying the storage copies both and
// re-points the copy's reserved_long4 at its own prefix, so a copy the host
// makes (checkout views, layer worlds) is a self-consistent object too.
struct alignas(16) EffectWorldStorage {
  const void* const* pf_world_vtable{};  // data() - 8
  std::array<std::byte, kEffectWorldSize> layer_def{};
  // AE's PF_WorldX<T> is 0x90 bytes: 8 (vtable) + 120 (LayerDef) + two words
  // its constructor writes (`this+0x88 = 0`). Without this tail a PF.dll path
  // that constructs or assigns through a `world - 8` pointer would write into
  // the *next* world of an array or a neighbouring local.
  std::array<std::byte, 0x10> pf_world_tail{};

  EffectWorldStorage() = default;
  EffectWorldStorage(const EffectWorldStorage& other) noexcept { assign(other); }
  EffectWorldStorage& operator=(const EffectWorldStorage& other) noexcept {
    if (this != &other) assign(other);
    return *this;
  }

  std::byte* data() noexcept { return layer_def.data(); }
  const std::byte* data() const noexcept { return layer_def.data(); }
  static constexpr std::size_t size() noexcept { return kEffectWorldSize; }
  void fill(std::byte value) noexcept { layer_def.fill(value); }
  std::byte& operator[](std::size_t index) noexcept { return layer_def[index]; }
  const std::byte& operator[](std::size_t index) const noexcept { return layer_def[index]; }
  auto begin() noexcept { return layer_def.begin(); }
  auto end() noexcept { return layer_def.end(); }
  auto begin() const noexcept { return layer_def.begin(); }
  auto end() const noexcept { return layer_def.end(); }
  bool operator==(const EffectWorldStorage& other) const noexcept {
    return layer_def == other.layer_def;
  }
  bool operator!=(const EffectWorldStorage& other) const noexcept { return !(*this == other); }

 private:
  void assign(const EffectWorldStorage& other) noexcept {
    pf_world_vtable = other.pf_world_vtable;
    layer_def = other.layer_def;
    pf_world_tail = other.pf_world_tail;
    // reserved_long4 (+0x50) pointing at the source's prefix follows the copy
    // to this one's prefix; any other value (null, a plug-in's own pointer,
    // a pool object) is carried as it is.
    const void* reserved_long4{};
    std::memcpy(&reserved_long4, layer_def.data() + 0x50, sizeof(reserved_long4));
    if (reserved_long4 == &other.pf_world_vtable) {
      const void* self = &pf_world_vtable;
      std::memcpy(layer_def.data() + 0x50, &self, sizeof(self));
    }
  }
};
static_assert(sizeof(EffectWorldStorage) == 0x90,
              "the embedded object must cover AE's whole PF_WorldX footprint");
static_assert(offsetof(EffectWorldStorage, layer_def) == 8);
static_assert(offsetof(EffectWorldStorage, pf_world_tail) == 8 + kEffectWorldSize);

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

  // Registers a world the host is handing to the plug-in. Also gives it AE's
  // PF_World identity behind reserved_long4 (worker_pf_world_facade), which is
  // why the world is not const.
  bool register_world(void* world, int32_t pixel_format);
  bool register_gpu_world(void* world, int32_t pixel_format);
};

bool resolve_dispatch_world_format(const void* world,
                                   OwnedWorldResolver owned_resolver,
                                   DispatchWorldFormat& result);
bool resolve_registered_dispatch_world(const void* world,
                                       DispatchWorldFormat& result);
// True when the dispatch-format registry knows this reference at all: the
// struct pointer was registered (a host-handed world - reaching a failed
// resolve with one means its fields no longer match what was registered), or
// its pixel pointer is some registered world's base under a different
// geometry. Both are the registry's fail-closed mismatch refusal, and the
// copy callbacks' foreign-operand fallback consults this so that refusal
// stays a refusal instead of degrading into foreign admission.
bool dispatch_world_reference_known(const void* world);

bool bounded_typed_world(void* world, int32_t pixel_bytes,
                         unsigned char*& pixels, int32_t& rowbytes,
                         int32_t& width, int32_t& height);
bool bounded_argb8_world(void* world, unsigned char*& pixels,
                        int32_t& rowbytes, int32_t& width, int32_t& height);

}  // namespace aexcompat::world_safety
