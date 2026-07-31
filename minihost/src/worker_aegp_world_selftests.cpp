#include "worker_aegp_world_selftests.hpp"

#include "worker_render_receipts.hpp"
#include "worker_world_registry.hpp"
#include "worker_world_safety.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cstddef>
#include <cstring>
#include <thread>
#include <unordered_set>
#include <vector>

namespace aexcompat::aegp_world_selftests {
namespace {

using world_registry::aegp_world_dispose;
using world_registry::aegp_world_dispose_platform;
using world_registry::aegp_world_fast_blur;
using world_registry::aegp_world_fill_pf_world;
using world_registry::aegp_world_get_base_addr16;
using world_registry::aegp_world_get_base_addr32;
using world_registry::aegp_world_get_base_addr8;
using world_registry::aegp_world_get_rowbytes;
using world_registry::aegp_world_get_size;
using world_registry::aegp_world_get_type;
using world_registry::aegp_world_new_owned;
using world_registry::aegp_world_new_platform;
using world_registry::aegp_world_reference_platform;
using world_registry::dispose_world;
using world_registry::new_world;
using world_safety::LocalEffectWorld;

constexpr int32_t kSyntheticCompWidth = 17;
constexpr int32_t kSyntheticCompHeight = 9;

bool valid(const Hooks& h) {
  return h.world_lifetimes_balanced && h.comp_item_handle && h.new_item_options &&
      h.timestamp && h.checkin_rendered && h.worthwhile && h.checkout_frame &&
      h.get_receipt_world && h.checkin_frame && h.bump_project_timestamp &&
      h.dispose_item_options && h.external_render_cache_empty &&
      h.set_synthetic_receipt_mode && h.receipt_lifetimes_balanced &&
      h.publish_receipt && h.insert_default_layer_options &&
      h.checkout_layer_frame && h.dispose_layer_options;
}

bool reject(const Hooks& h, const char* stage) {
  if (h.diagnostic) h.diagnostic(stage);
  return false;
}

class SyntheticReceiptScope {
 public:
  explicit SyntheticReceiptScope(const Hooks& hooks) : hooks_(hooks) {
    hooks_.set_synthetic_receipt_mode(true);
  }
  SyntheticReceiptScope(const SyntheticReceiptScope&) = delete;
  SyntheticReceiptScope& operator=(const SyntheticReceiptScope&) = delete;
  ~SyntheticReceiptScope() { hooks_.set_synthetic_receipt_mode(false); }
 private:
  const Hooks& hooks_;
};

}  // namespace

bool verify_world_suite3(const Hooks& h) {
  if (!valid(h)) return reject(h, "hooks");
  int32_t null_value = 0;
  if (aegp_world_get_type(nullptr, &null_value) == 0 ||
      aegp_world_get_type(reinterpret_cast<void**>(1), nullptr) == 0 ||
      aegp_world_get_size(nullptr, &null_value, &null_value) == 0 ||
      aegp_world_get_rowbytes(nullptr, nullptr) == 0 ||
      aegp_world_fill_pf_world(nullptr, nullptr) == 0) return reject(h, "null-contract");
  const std::array<int32_t, 3> formats{world_registry::kPixelFormatArgb32,
      world_registry::kPixelFormatArgb64, world_registry::kPixelFormatArgb128};
  for (std::size_t index = 0; index < formats.size(); ++index) {
    std::array<std::byte, world_safety::kEffectWorldSize> storage{};
    if (new_world(nullptr, 7, 5, 1, formats[index], storage.data()) != 0) return reject(h, "new-world");
    void* token = storage.data();
    void** handle = &token;
    if (!world_registry::register_borrowed_view(handle, storage.data(), formats[index], false)) return reject(h, "borrowed-register");
    int32_t type = 0, width = 0, height = 0;
    uint32_t rowbytes = 0;
    void* pixels = nullptr;
    std::array<std::byte, world_safety::kEffectWorldSize> projection{};
    const bool metadata_ok = aegp_world_get_type(handle, &type) == 0 &&
        type == static_cast<int32_t>(index + 1) &&
        aegp_world_get_size(handle, &width, &height) == 0 && width == 7 && height == 5 &&
        aegp_world_get_rowbytes(handle, &rowbytes) == 0 &&
        rowbytes == 7U * static_cast<uint32_t>(4U << index) &&
        aegp_world_fill_pf_world(handle, projection.data()) == 0 &&
        std::memcmp(projection.data(), storage.data(), world_safety::kEffectWorldSize) == 0;
    const int32_t correct_addr = index == 0 ? aegp_world_get_base_addr8(handle, &pixels) :
        (index == 1 ? aegp_world_get_base_addr16(handle, &pixels) : aegp_world_get_base_addr32(handle, &pixels));
    void* wrong_pixels = reinterpret_cast<void*>(1);
    const int32_t wrong_addr = index == 0 ? aegp_world_get_base_addr16(handle, &wrong_pixels) :
        aegp_world_get_base_addr8(handle, &wrong_pixels);
    LocalEffectWorld world{};
    std::memcpy(&world, storage.data(), sizeof(world));
    const bool addresses_ok = correct_addr == 0 && pixels == world.data && wrong_addr != 0 &&
        wrong_pixels == reinterpret_cast<void*>(1);
    if (world_registry::unregister_borrowed_view(handle) !=
        world_registry::UnregisterBorrowedViewResult::removed) return reject(h, "borrowed-unregister");
    const bool stale_rejected = aegp_world_get_type(handle, &type) != 0;
    if (dispose_world(nullptr, storage.data()) != 0 || !metadata_ok || !addresses_ok ||
        !stale_rejected) return reject(h, "borrowed-contract");
  }

  std::array<std::byte, world_safety::kEffectWorldSize> borrowed{};
  std::array<std::byte, 16> borrowed_pixels{};
  auto* borrowed_world = reinterpret_cast<LocalEffectWorld*>(borrowed.data());
  borrowed_world->world_flags = 2; borrowed_world->data = borrowed_pixels.data();
  borrowed_world->rowbytes = 8; borrowed_world->width = 2; borrowed_world->height = 2;
  void* token = borrowed.data();
  void** handle = &token;
  if (!world_registry::register_borrowed_view(handle, borrowed.data(), world_registry::kPixelFormatArgb32)) return reject(h, "borrowed-live-register");
  int32_t width = 0, height = 0;
  const bool borrowed_live = aegp_world_get_size(handle, &width, &height) == 0 &&
      width == 2 && height == 2 && aegp_world_dispose(handle) != 0;
  if (world_registry::unregister_borrowed_view(handle) !=
      world_registry::UnregisterBorrowedViewResult::removed) return reject(h, "borrowed-live-unregister");
  if (!borrowed_live || aegp_world_get_size(handle, &width, &height) == 0) return reject(h, "borrowed-live-contract");

  for (int32_t type = 1; type <= 3; ++type) {
    void** owned = nullptr; void* base = nullptr; LocalEffectWorld projection{};
    int32_t actual_type = 0; uint32_t rowbytes = 0;
    if (aegp_world_new_owned(1, type, 4, 3, &owned) != 0 || !owned ||
        aegp_world_get_type(owned, &actual_type) != 0 || actual_type != type ||
        aegp_world_get_size(owned, &width, &height) != 0 || width != 4 || height != 3 ||
        aegp_world_get_rowbytes(owned, &rowbytes) != 0 || rowbytes != static_cast<uint32_t>(4 * (4 << (type - 1))) ||
        aegp_world_fill_pf_world(owned, &projection) != 0 || projection.width != 4 || projection.height != 3 ||
        (type == 1 ? aegp_world_get_base_addr8(owned, &base) :
         (type == 2 ? aegp_world_get_base_addr16(owned, &base) : aegp_world_get_base_addr32(owned, &base))) != 0 ||
        !base || aegp_world_dispose(owned) != 0 || aegp_world_get_type(owned, &actual_type) == 0 ||
        aegp_world_dispose(owned) == 0) return reject(h, "owned-contract");
  }
  void** blur_world = nullptr; void* blur_base = nullptr;
  if (aegp_world_new_owned(1, 1, 5, 5, &blur_world) != 0 ||
      aegp_world_get_base_addr8(blur_world, &blur_base) != 0 || !blur_base) return reject(h, "blur-new");
  auto* blur_pixels = static_cast<uint8_t*>(blur_base);
  std::fill(blur_pixels, blur_pixels + 5 * 5 * 4, 0);
  for (int channel = 0; channel < 4; ++channel) blur_pixels[(2 * 5 + 2) * 4 + channel] = 255;
  std::array<uint8_t, 5 * 5 * 4> unchanged{};
  std::memcpy(unchanged.data(), blur_pixels, unchanged.size());
  if (aegp_world_fast_blur(0.0, 0, 1, blur_world) != 0 ||
      std::memcmp(unchanged.data(), blur_pixels, unchanged.size()) != 0 ||
      aegp_world_fast_blur(1.0, 0, 1, blur_world) != 0 || blur_pixels[(2 * 5 + 2) * 4] >= 255 ||
      blur_pixels[(2 * 5 + 1) * 4] == 0 || aegp_world_fast_blur(-1.0, 0, 1, blur_world) == 0 ||
      aegp_world_fast_blur(1.0, 2, 1, blur_world) == 0 || aegp_world_dispose(blur_world) != 0 ||
      aegp_world_fast_blur(1.0, 0, 1, blur_world) == 0) return reject(h, "blur-contract");

  void* platform = nullptr; void** reference = nullptr; void* pixels = nullptr;
  if (aegp_world_new_platform(1, 1, 3, 2, &platform) != 0 || !platform ||
      aegp_world_reference_platform(1, platform, &reference) != 0 || !reference ||
      aegp_world_get_base_addr8(reference, &pixels) != 0 || !pixels ||
      aegp_world_dispose_platform(platform) != 0 ||
      aegp_world_get_size(reference, &width, &height) != 0 || width != 3 || height != 2 ||
      aegp_world_dispose_platform(platform) == 0 || aegp_world_dispose(reference) != 0 ||
      aegp_world_get_size(reference, &width, &height) == 0 || aegp_world_dispose(reference) == 0)
    return reject(h, "platform-contract");

  void* options = nullptr; std::array<uint8_t, 4> timestamp{};
  if (h.new_item_options(h.comp_item_handle(), &options) != 0 || h.timestamp(timestamp.data()) != 0 ||
      aegp_world_new_platform(1, 1, kSyntheticCompWidth, kSyntheticCompHeight, &platform) != 0 ||
      aegp_world_reference_platform(1, platform, &reference) != 0 ||
      aegp_world_get_base_addr8(reference, &pixels) != 0 || !pixels) return reject(h, "cache-setup");
  uint32_t cached_generation = 0;
  std::memcpy(&cached_generation, timestamp.data(), sizeof(cached_generation));
  const std::array<uint8_t, 4> external_sentinel{{231, 17, 91, 203}};
  std::memcpy(pixels, external_sentinel.data(), external_sentinel.size());
  if (h.checkin_rendered(options, timestamp.data(), 1, platform) != 0 ||
      aegp_world_dispose_platform(platform) == 0) return reject(h, "cache-checkin");
  uint8_t worthwhile = 1;
  if (h.worthwhile(options, timestamp.data(), &worthwhile) != 0 || worthwhile != 0) return reject(h, "cache-worthwhile");
  void* cached_receipt = nullptr; void** cached_world = nullptr; void* cached_pixels = nullptr;
  render_receipts::ReceiptSnapshot cached_snapshot{};
  if (h.checkout_frame(options, &cached_receipt) != 0 || !cached_receipt ||
      h.get_receipt_world(cached_receipt, &cached_world) != 0 ||
      aegp_world_get_base_addr8(cached_world, &cached_pixels) != 0 || !cached_pixels ||
      std::memcmp(cached_pixels, external_sentinel.data(), external_sentinel.size()) != 0 ||
      !render_receipts::snapshot(cached_receipt, cached_snapshot) ||
      !cached_snapshot.scene_bound ||
      cached_snapshot.project_generation != cached_generation ||
      aegp_world_dispose(reference) != 0) return reject(h, "cache-checkout");
  h.bump_project_timestamp();
  render_receipts::ReceiptSnapshot stale_cached_snapshot{};
  void** stale_cached_world =
      reinterpret_cast<void**>(static_cast<uintptr_t>(1));
  if (render_receipts::snapshot(cached_receipt, stale_cached_snapshot) ||
      h.get_receipt_world(cached_receipt, &stale_cached_world) == 0 ||
      stale_cached_world != nullptr)
    return reject(h, "cache-generation-binding");
  if (aegp_world_new_platform(1, 1, 1, 1, &platform) != 0 ||
      h.checkin_rendered(options, timestamp.data(), 1, platform) == 0 ||
      aegp_world_dispose_platform(platform) != 0 || h.dispose_item_options(options) != 0) return reject(h, "cache-stale");
  return h.world_lifetimes_balanced() && h.external_render_cache_empty() &&
      world_registry::aegp_lifetimes_balanced();
}

bool verify_world_mfr_safety(const Hooks& h) {
  if (!valid(h)) return reject(h, "hooks");
  void** world = nullptr;
  if (aegp_world_new_owned(1, 1, 16, 16, &world) != 0 || !world) return reject(h, "mfr-new");
  std::atomic_bool start{false}, invalid{false};
  std::vector<std::thread> readers;
  try {
    for (int worker = 0; worker < 4; ++worker) readers.emplace_back([&] {
      while (!start.load(std::memory_order_acquire)) std::this_thread::yield();
      for (int iteration = 0; iteration < 256; ++iteration) {
        int32_t type = 0, width = 0, height = 0; uint32_t rowbytes = 0;
        if (aegp_world_get_type(world, &type) != 0 || type != 1 ||
            aegp_world_get_size(world, &width, &height) != 0 || width != 16 || height != 16 ||
            aegp_world_get_rowbytes(world, &rowbytes) != 0 || rowbytes != 64)
          invalid.store(true, std::memory_order_release);
      }
    });
  } catch (...) {
    start.store(true, std::memory_order_release);
    for (auto& reader : readers) reader.join();
    aegp_world_dispose(world);
    return reject(h, "mfr-thread-create");
  }
  start.store(true, std::memory_order_release);
  for (int iteration = 0; iteration < 16; ++iteration)
    if (aegp_world_fast_blur(0.25, 0, iteration & 1, world) != 0) invalid.store(true, std::memory_order_release);
  for (auto& reader : readers) reader.join();
  if (invalid.load(std::memory_order_acquire) || aegp_world_dispose(world) != 0 ||
      !world_registry::aegp_lifetimes_balanced()) return reject(h, "mfr-access");

  std::array<world_registry::AegpWorldSnapshot, 64> pins{};
  for (auto& pin : pins) {
    void** pinned_world = nullptr;
    if (aegp_world_new_owned(1, 1, 1, 1, &pinned_world) != 0 ||
        !world_registry::snapshot_aegp_world(pinned_world, pin) || !pin.backing_pin ||
        aegp_world_dispose(pinned_world) != 0) return reject(h, "mfr-pin");
  }
  void** rejected_world = reinterpret_cast<void**>(1);
  if (aegp_world_new_owned(1, 1, 1, 1, &rejected_world) == 0 || rejected_world != nullptr) return reject(h, "mfr-cap");
  pins[0] = {};
  void** admitted = nullptr;
  if (aegp_world_new_owned(1, 1, 1, 1, &admitted) != 0 || !admitted ||
      aegp_world_dispose(admitted) != 0) return reject(h, "mfr-readmit");
  pins = {};
  return world_registry::aegp_lifetimes_balanced();
}

bool verify_async_receipts(const Hooks& h) {
  if (!valid(h)) return reject(h, "hooks");
  SyntheticReceiptScope synthetic(h);
  if (!h.receipt_lifetimes_balanced()) return reject(h, "receipt-initial-balance");
  const std::array<int32_t, 3> formats{world_registry::kPixelFormatArgb32,
      world_registry::kPixelFormatArgb64, world_registry::kPixelFormatArgb128};
  std::unordered_set<void*> receipt_handles;
  std::unordered_set<void**> world_handles;
  for (std::size_t index = 0; index < formats.size(); ++index) {
    void* receipt = reinterpret_cast<void*>(1);
    if (h.publish_receipt(formats[index], &receipt) != 0 || !receipt) return reject(h, "receipt-publish");
    void** world = nullptr;
    if (h.get_receipt_world(receipt, &world) != 0 || !world) return reject(h, "receipt-world");
    if (!receipt_handles.insert(receipt).second || !world_handles.insert(world).second) return reject(h, "receipt-identity");
    int32_t type = 0, width = 0, height = 0; void* pixels = nullptr;
    void* wrong_pixels = reinterpret_cast<void*>(1);
    const int32_t correct = index == 0 ? aegp_world_get_base_addr8(world, &pixels) :
        (index == 1 ? aegp_world_get_base_addr16(world, &pixels) : aegp_world_get_base_addr32(world, &pixels));
    const int32_t wrong = index == 0 ? aegp_world_get_base_addr16(world, &wrong_pixels) : aegp_world_get_base_addr8(world, &wrong_pixels);
    if (aegp_world_get_type(world, &type) != 0 || type != static_cast<int32_t>(index + 1) ||
        aegp_world_get_size(world, &width, &height) != 0 || width != 8 || height != 4 ||
        correct != 0 || !pixels || wrong == 0 || wrong_pixels != reinterpret_cast<void*>(1) ||
        aegp_world_dispose(world) == 0) return reject(h, "receipt-world-contract");
    void** stale_world = world;
    if (h.checkin_frame(receipt) != 0 || h.get_receipt_world(receipt, &world) == 0 ||
        aegp_world_get_type(stale_world, &type) == 0 || h.checkin_frame(receipt) == 0) return reject(h, "receipt-stale");
  }
  void* options = nullptr;
  if (h.insert_default_layer_options(&options) != 0 || !options) return reject(h, "layer-options");
  void* receipt = nullptr;
  if (h.checkout_layer_frame(options, &receipt) != 0 || !receipt ||
      h.dispose_layer_options(options) != 0) return reject(h, "layer-checkout");
  void** world = nullptr; int32_t width = 0, height = 0;
  const bool survives_options = h.get_receipt_world(receipt, &world) == 0 && world &&
      aegp_world_get_size(world, &width, &height) == 0 && width == 8 && height == 4;
  return survives_options && h.checkin_frame(receipt) == 0 && h.receipt_lifetimes_balanced();
}

}  // namespace aexcompat::aegp_world_selftests
