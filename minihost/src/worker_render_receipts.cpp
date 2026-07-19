#include "worker_render_receipts.hpp"

#include "worker_world_registry.hpp"

#include <algorithm>
#include <atomic>
#include <cstring>
#include <limits>
#include <mutex>
#include <unordered_map>
#include <utility>

namespace aexcompat::render_receipts {
namespace {

struct Receipt {
  std::unique_ptr<ReceiptDraft> draft;
  void* handle{};
  void** world_handle{};
  std::array<uint8_t, 16> guid{};
};

std::mutex g_mutex;
std::unordered_map<void*, std::unique_ptr<Receipt>> g_receipts;
std::atomic<uint64_t> g_receipt_generation{1};
std::atomic<uint64_t> g_world_generation{1};
uint64_t g_live_bytes{};
uint64_t g_created{};
uint64_t g_checked_in{};
uint64_t g_invalid_operations{};

bool claim_generation(std::atomic<uint64_t>& counter, uint64_t& generation) {
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

bool assign_handles(Receipt& receipt) {
  uint64_t receipt_generation = 0;
  uint64_t world_generation = 0;
  if (!claim_generation(g_receipt_generation, receipt_generation) ||
      !claim_generation(g_world_generation, world_generation)) return false;
  receipt.handle = reinterpret_cast<void*>(
      static_cast<uintptr_t>((receipt_generation << 3) | 4));
  receipt.world_handle = reinterpret_cast<void**>(
      static_cast<uintptr_t>((world_generation << 3) | 6));
  std::memcpy(receipt.guid.data(), &receipt_generation,
              (std::min)(sizeof(receipt_generation), receipt.guid.size()));
  std::memcpy(receipt.guid.data() + 8, &world_generation,
              (std::min)(sizeof(world_generation), receipt.guid.size() - 8));
  receipt.guid[6] = static_cast<uint8_t>((receipt.guid[6] & 0x0f) | 0x40);
  receipt.guid[8] = static_cast<uint8_t>((receipt.guid[8] & 0x3f) | 0x80);
  return true;
}

bool has_capacity(uint64_t bytes) {
  return bytes <= kMaxReceiptBytes && g_receipts.size() < kMaxReceiptCount &&
      g_live_bytes <= kMaxReceiptBytes - bytes;
}

}  // namespace

int32_t register_receipt(std::unique_ptr<ReceiptDraft> draft, void** output) {
  if (output) *output = nullptr;
  if (!output || !draft || draft->pixels.empty() ||
      draft->world.data != draft->pixels.data()) return 4;
  const uint64_t bytes = draft->pixels.size();
  std::unique_ptr<Receipt> receipt;
  try {
    receipt = std::make_unique<Receipt>();
  } catch (...) {
    return 4;
  }
  receipt->draft = std::move(draft);
  if (!assign_handles(*receipt)) return 4;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    if (!has_capacity(bytes)) return 4;
  }
  if (!world_registry::register_borrowed_view(
          receipt->world_handle, &receipt->draft->world,
          receipt->draft->pixel_format)) return 4;
  void** const registered_world_handle = receipt->world_handle;
  bool committed = false;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    if (has_capacity(bytes)) {
      try {
        void* const key = receipt->handle;
        committed = g_receipts.emplace(receipt->handle, std::move(receipt)).second;
        if (committed) {
          g_live_bytes += bytes;
          ++g_created;
          *output = key;
        }
      } catch (...) {
        committed = false;
      }
    }
  }
  if (!committed) {
    world_registry::unregister_borrowed_view(registered_world_handle);
    return 4;
  }
  return 0;
}

int32_t get_world(void* receipt, void*** world) {
  if (world) *world = nullptr;
  if (!receipt || !world) return 4;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_receipts.find(receipt);
  if (found == g_receipts.end() || !found->second->world_handle) {
    ++g_invalid_operations;
    return 4;
  }
  *world = found->second->world_handle;
  return 0;
}

int32_t checkin(void* handle) {
  if (!handle) return 4;
  decltype(g_receipts)::node_type receipt;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    const auto found = g_receipts.find(handle);
    if (found == g_receipts.end()) {
      ++g_invalid_operations;
      return 4;
    }
    receipt = g_receipts.extract(found);
    g_live_bytes -= receipt.mapped()->draft->pixels.size();
  }
  if (!world_registry::unregister_borrowed_view(receipt.mapped()->world_handle)) {
    std::lock_guard<std::mutex> lock(g_mutex);
    ++g_invalid_operations;
    g_live_bytes += receipt.mapped()->draft->pixels.size();
    g_receipts.insert(std::move(receipt));
    return 4;
  }
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    ++g_checked_in;
  }
  return 0;
}

bool checkin_if_live(void* handle) {
  if (!handle) return false;
  decltype(g_receipts)::node_type receipt;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    const auto found = g_receipts.find(handle);
    if (found == g_receipts.end()) return false;
    receipt = g_receipts.extract(found);
    g_live_bytes -= receipt.mapped()->draft->pixels.size();
  }
  if (!world_registry::unregister_borrowed_view(receipt.mapped()->world_handle)) {
    std::lock_guard<std::mutex> lock(g_mutex);
    g_live_bytes += receipt.mapped()->draft->pixels.size();
    g_receipts.insert(std::move(receipt));
    return false;
  }
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    ++g_checked_in;
  }
  return true;
}

bool snapshot(void* handle, ReceiptSnapshot& output) {
  output = {};
  if (!handle) return false;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_receipts.find(handle);
  if (found == g_receipts.end()) return false;
  const auto& receipt = *found->second;
  output.has_render_options = receipt.draft->has_render_options;
  output.render_options = receipt.draft->render_options;
  output.rendered_region = receipt.draft->rendered_region;
  output.render_timestamp = receipt.draft->render_timestamp;
  output.guid = receipt.guid;
  return true;
}

Statistics statistics() {
  std::lock_guard<std::mutex> lock(g_mutex);
  return {g_created, g_checked_in, g_invalid_operations, g_receipts.size(),
          g_live_bytes};
}

bool lifetimes_balanced() {
  const auto stats = statistics();
  return stats.live_count == 0 && stats.live_bytes == 0 &&
      stats.created == stats.checked_in;
}

}  // namespace aexcompat::render_receipts
