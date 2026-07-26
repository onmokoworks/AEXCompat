#pragma once

#include "worker_aegp_render_options.hpp"
#include "worker_aegp_scene_model.hpp"

#include <cstddef>
#include <cstdint>

namespace aexcompat::aegp_staged_item_runtime {

enum class SamplingPolicy : uint8_t { exact, hold, nearest };
enum class StageKind : uint8_t { upstream, all_effects, downstream, final_item };

using PublishSynthetic = int32_t(*)(int32_t pixel_format, void** output,
                                    const render_options::ItemValue* options);
using Cancel = int32_t(__cdecl*)(void* refcon, uint8_t* canceled);
using Checkout = int32_t(__cdecl*)(void* options, Cancel cancel, void* refcon, void** output);

struct Hooks {
  uint32_t (*project_generation)();
  bool (*synthetic_receipts_enabled)();
  PublishSynthetic publish_synthetic;
};

struct Diagnostics {
  uint32_t published{};
  uint32_t cache_hits{};
  uint32_t cache_misses{};
  uint32_t cycles_rejected{};
  uint32_t generation_invalidations{};
  uint32_t evictions{};
  uint32_t exact_hits{};
  uint32_t hold_hits{};
  uint32_t nearest_hits{};
  uint32_t unavailable_frames{};
  uint32_t direct_cycles_rejected{};
  uint32_t indirect_cycles_rejected{};
  uint32_t depth_limit_rejections{};
  uint32_t stage_limit_rejections{};
  uint32_t time_limit_rejections{};
  uint32_t effect_boundary_rejections{};
  uint32_t partial_failures{};
  uint32_t cleanup_count{};
  uint32_t in_flight{};
  uint32_t max_in_flight{};
  uint32_t registered_items{};
  uint32_t cached_stages{};
  uint64_t cached_bytes{};
  uint64_t last_trace_hash{};
  uint64_t last_stage_identity_hash{};
  uint32_t last_resolved_stages{};
  uint32_t max_resolved_depth{};
  uint32_t typed_registrations{};
  uint32_t identity_mismatch_rejections{};
  uint32_t duplicate_identity_rejections{};
  uint32_t cross_project_rejections{};
  uint32_t registration_cycle_rejections{};
  uint32_t stale_stage_invalidations{};
  uint32_t stale_receipt_invalidations{};
  uint32_t invalid_handle_rejections{};
};

struct OrderedSceneEffect {
  scene_model::Identity identity{};
  uint32_t order{};
};

void configure(Hooks hooks) noexcept;
void clear() noexcept;
bool has_item_registration(void* item) noexcept;
bool register_item(void* item, uint64_t stable_identity, SamplingPolicy policy,
                   void* const* dependencies, std::size_t dependency_count,
                   const uint64_t* effect_instances, std::size_t effect_count);
bool register_scene_item(
    scene_model::Registry& registry, scene_model::Identity item,
    SamplingPolicy policy, const scene_model::Identity* dependencies,
    std::size_t dependency_count, const OrderedSceneEffect* effects,
    std::size_t effect_count);
bool publish_stage_world(void* item, StageKind stage_kind, uint64_t effect_instance,
                         suite_abi::AegpTime time, suite_abi::AegpTime time_step,
                         int8_t quality, uint8_t guide_layers, int32_t pixel_format,
                         int32_t width, int32_t height, int32_t rowbytes,
                         const void* pixels, uint64_t* stage_identity_hash = nullptr);
bool publish_scene_stage_world(
    scene_model::Registry& registry, scene_model::Identity item,
    StageKind stage_kind, scene_model::Identity effect,
    suite_abi::AegpTime time, suite_abi::AegpTime time_step,
    int8_t quality, uint8_t guide_layers, int32_t pixel_format,
    int32_t width, int32_t height, int32_t rowbytes, const void* pixels,
    uint64_t* stage_identity_hash = nullptr);
bool publish_world(void* item, suite_abi::AegpTime time, suite_abi::AegpTime time_step,
                   int8_t quality, uint8_t guide_layers, int32_t pixel_format,
                   int32_t width, int32_t height, int32_t rowbytes, const void* pixels);
int32_t publish_registered_receipt(const render_options::ItemValue& options,
                                   void** receipt);
int32_t publish_receipt(void* options, void** receipt);
bool verify_recursion_guard(void* item, suite_abi::AegpTime time,
                            void* options, Checkout checkout);
void invalidate_scene_generation(uint64_t project_id,
                                 uint32_t valid_generation) noexcept;
Diagnostics diagnostics() noexcept;

}  // namespace aexcompat::aegp_staged_item_runtime
