#pragma once

#include "worker_aegp_render_options.hpp"
#include "worker_world_safety.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <vector>

namespace aexcompat::render_receipts {

inline constexpr std::size_t kMaxReceiptCount = 32;
inline constexpr uint64_t kMaxReceiptBytes = 64ULL * 1024 * 1024;

struct ReceiptDraft {
  world_safety::LocalEffectWorld world{};
  std::vector<std::byte> pixels;
  std::shared_ptr<const std::vector<std::byte>> staged_source_pin;
  int32_t pixel_format{};
  bool has_render_options{};
  render_options::ItemValue render_options{};
  suite_abi::AegpRect rendered_region{};
  uint32_t render_timestamp{};
};

struct ReceiptSnapshot {
  bool has_render_options{};
  render_options::ItemValue render_options{};
  suite_abi::AegpRect rendered_region{};
  uint32_t render_timestamp{};
  std::array<uint8_t, 16> guid{};
};

struct Statistics {
  uint64_t created{};
  uint64_t checked_in{};
  uint64_t invalid_operations{};
  std::size_t live_count{};
  uint64_t live_bytes{};
  std::size_t reserved_count{};
  uint64_t reserved_bytes{};
};

int32_t register_receipt(std::unique_ptr<ReceiptDraft> draft, void** output);
int32_t get_world(void* receipt, void*** world);
int32_t checkin(void* receipt);
bool checkin_if_live(void* receipt);
bool snapshot(void* receipt, ReceiptSnapshot& output);
Statistics statistics();
bool lifetimes_balanced();

}  // namespace aexcompat::render_receipts
