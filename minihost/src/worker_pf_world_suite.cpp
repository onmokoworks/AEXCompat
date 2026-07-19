#include "worker_pf_world_suite.hpp"

#include "worker_mask_runtime_internal.hpp"
#include "worker_world_safety.hpp"

#include <algorithm>
#include <atomic>
#include <cstddef>
#include <cstring>
#include <thread>

namespace aexcompat::l2_detail {

using world_registry::dispose_world;
using world_registry::get_pixel_format;
using world_registry::kPixelFormatArgb32;
using world_registry::kPixelFormatArgb64;
using world_registry::kPixelFormatArgb128;
using world_registry::legacy_new_world;
using world_registry::new_world;
using world_safety::kEffectWorldSize;

extern OpaqueHostObject g_effect;

namespace {
bool world_lifetimes_balanced() {
  return aexcompat::world_registry::lifetimes_balanced();
}
}  // namespace

WorldSuite g_world_suite{&new_world, &dispose_world, &get_pixel_format};
std::array<void*, 2> g_world_suite1{
    reinterpret_cast<void*>(&legacy_new_world), reinterpret_cast<void*>(&dispose_world)};

bool verify_world_double_dispose_rejected() {
  alignas(8) std::array<std::byte, kEffectWorldSize> world{};
  const uint64_t invalid_before =
      aexcompat::world_registry::statistics().invalid_operations;
  if (new_world(&g_effect, 7, 5, 1, kPixelFormatArgb128, world.data()) != 0) return false;
  void* pixels{};
  int32_t flags{}, rowbytes{}, width{}, height{}, format{};
  std::memcpy(&flags, world.data() + 16, sizeof(flags));
  std::memcpy(&pixels, world.data() + 24, sizeof(pixels));
  std::memcpy(&rowbytes, world.data() + 32, sizeof(rowbytes));
  std::memcpy(&width, world.data() + 36, sizeof(width));
  std::memcpy(&height, world.data() + 40, sizeof(height));
  const bool layout_valid = pixels && flags == 3 && rowbytes == 112 && width == 7 && height == 5 &&
      get_pixel_format(world.data(), &format) == 0 && format == kPixelFormatArgb128 &&
      std::all_of(static_cast<const unsigned char*>(pixels),
                  static_cast<const unsigned char*>(pixels) + 560,
                  [](unsigned char value) { return value == 0; });
  const int32_t first = dispose_world(&g_effect, world.data());
  const int32_t second = dispose_world(&g_effect, world.data());
  return layout_valid && first == 0 && second != 0 &&
      aexcompat::world_registry::statistics().invalid_operations ==
          invalid_before + 1 && world_lifetimes_balanced();
}

bool verify_world_allocation_limit_rejected() {
  alignas(8) std::array<std::byte, kEffectWorldSize> world{};
  world.fill(std::byte{0x5a});
  const auto before = world;
  const uint64_t invalid_before =
      aexcompat::world_registry::statistics().invalid_operations;
  const int32_t error = new_world(&g_effect, 32768, 32768, 1,
                                  kPixelFormatArgb128, world.data());
  return error != 0 && world == before &&
      aexcompat::world_registry::statistics().invalid_operations ==
          invalid_before + 1 && world_lifetimes_balanced();
}

bool verify_owned_world_snapshot_is_atomic() {
  alignas(8) std::array<std::byte, kEffectWorldSize> world{};
  if (new_world(&g_effect, 2, 2, 1, kPixelFormatArgb32, world.data()) != 0)
    return false;
  aexcompat::world_registry::OwnedWorldSnapshot snapshot{};
  if (!aexcompat::world_registry::snapshot_owned_world(world.data(), snapshot) ||
      !snapshot.world.data || snapshot.pixel_format != kPixelFormatArgb32 ||
      snapshot.world.width != 2 || snapshot.world.height != 2 ||
      snapshot.world.rowbytes != 8) {
    dispose_world(&g_effect, world.data());
    return false;
  }
  const void* captured_data = snapshot.world.data;
  if (dispose_world(&g_effect, world.data()) != 0 || !world_lifetimes_balanced())
    return false;
  aexcompat::world_registry::OwnedWorldSnapshot stale{};
  return captured_data && snapshot.world.data == captured_data &&
      snapshot.world.width == 2 && snapshot.pixel_format == kPixelFormatArgb32 &&
      !aexcompat::world_registry::snapshot_owned_world(world.data(), stale);
}

bool verify_owned_world_snapshot_concurrent_dispose() {
  for (int iteration = 0; iteration < 64; ++iteration) {
    alignas(8) std::array<std::byte, kEffectWorldSize> world{};
    if (new_world(&g_effect, 3, 2, 1, kPixelFormatArgb64, world.data()) != 0)
      return false;
    std::atomic_bool ready{false};
    std::atomic_bool go{false};
    bool resolved = false;
    aexcompat::world_registry::OwnedWorldSnapshot snapshot{};
    std::thread reader([&] {
      ready.store(true, std::memory_order_release);
      while (!go.load(std::memory_order_acquire)) std::this_thread::yield();
      resolved = aexcompat::world_registry::snapshot_owned_world(
          world.data(), snapshot);
    });
    while (!ready.load(std::memory_order_acquire)) std::this_thread::yield();
    go.store(true, std::memory_order_release);
    const int32_t dispose_error = dispose_world(&g_effect, world.data());
    reader.join();
    if (dispose_error != 0 || !world_lifetimes_balanced()) return false;
    if (resolved && (!snapshot.world.data || snapshot.world.width != 3 ||
        snapshot.world.height != 2 || snapshot.world.rowbytes != 24 ||
        snapshot.pixel_format != kPixelFormatArgb64)) return false;
  }
  return true;
}

}  // namespace aexcompat::l2_detail
