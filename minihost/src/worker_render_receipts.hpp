#pragma once

#include "worker_aegp_render_options.hpp"
#include "worker_aegp_scene_model.hpp"
#include "worker_world_safety.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <vector>

namespace aexcompat::render_receipts {

inline constexpr std::size_t kMaxReceiptCount = 32;
inline constexpr uint64_t kMaxReceiptBytes = 64ULL * 1024 * 1024;

// Receipt test-mode state (issue #126 Phase D): the synthetic-receipt
// toggle and the async-manager identity anchor handed to the AEGP render
// suites. worker_main and the AEGP render selftests share this state.
// Lifetime: process-lifetime, defaults off/false, never torn down.
struct ReceiptTestState {
  bool synthetic_test_mode{};
  int async_manager{};
};
ReceiptTestState& receipt_test_state();

struct ReceiptDraft {
  world_safety::LocalEffectWorld world{};
  std::vector<std::byte> pixels;
  std::shared_ptr<const std::vector<std::byte>> staged_source_pin;
  int32_t pixel_format{};
  bool has_render_options{};
  render_options::ItemValue render_options{};
  suite_abi::AegpRect rendered_region{};
  uint32_t render_timestamp{};
  bool has_stage_evidence{};
  uint64_t stage_identity_hash{};
  uint64_t item_identity{};
  uint64_t effect_instance{};
  uint64_t trace_hash{};
  suite_abi::AegpTime requested_time{};
  suite_abi::AegpTime source_time{};
  uint32_t project_generation{};
  uint32_t resolved_stage_count{};
  uint32_t resolved_depth{};
  uint8_t stage_kind{};
  uint8_t sampling_policy{};
  bool scene_bound{};
  scene_model::Identity scene_item{};
  scene_model::Identity scene_effect{};
  scene_model::Identity scene_project{};
  uint32_t effect_order{};
  uint64_t dependency_identity_hash{};
  uint64_t effect_order_hash{};
};

struct ReceiptSnapshot {
  bool has_render_options{};
  render_options::ItemValue render_options{};
  suite_abi::AegpRect rendered_region{};
  uint32_t render_timestamp{};
  std::array<uint8_t, 16> guid{};
  bool has_stage_evidence{};
  uint64_t stage_identity_hash{};
  uint64_t item_identity{};
  uint64_t effect_instance{};
  uint64_t trace_hash{};
  suite_abi::AegpTime requested_time{};
  suite_abi::AegpTime source_time{};
  uint32_t project_generation{};
  uint32_t resolved_stage_count{};
  uint32_t resolved_depth{};
  uint8_t stage_kind{};
  uint8_t sampling_policy{};
  bool scene_bound{};
  scene_model::Identity scene_item{};
  scene_model::Identity scene_effect{};
  scene_model::Identity scene_project{};
  uint32_t effect_order{};
  uint64_t dependency_identity_hash{};
  uint64_t effect_order_hash{};
};

struct Statistics {
  uint64_t created{};
  uint64_t checked_in{};
  uint64_t invalid_operations{};
  uint64_t stale_invalidations{};
  uint64_t invalid_handle_operations{};
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
std::size_t invalidate_scene_generation(uint64_t project_id,
                                        uint32_t valid_generation);
std::size_t invalidate_all_scene_generations(uint32_t valid_generation);
Statistics statistics();
bool lifetimes_balanced();

}  // namespace aexcompat::render_receipts
