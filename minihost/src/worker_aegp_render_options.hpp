#pragma once

#include "worker_suite_abi.hpp"

#include <cstddef>
#include <cstdint>

namespace aexcompat::render_options {

using suite_abi::AegpRect;
using suite_abi::AegpTime;

enum class LayerEffectBoundary : uint8_t { all, upstream, downstream };

struct LayerValue {
  int32_t owner_plugin_id{1};
  void* layer{};
  void* upstream_effect{};
  LayerEffectBoundary effect_boundary{LayerEffectBoundary::all};
  AegpTime time{0, 1};
  AegpTime time_step{1, 30};
  int32_t world_type{1};
  int16_t downsample_x{1};
  int16_t downsample_y{1};
  int32_t matte{};
};

struct ItemValue {
  int32_t owner_plugin_id{1};
  void* item{};
  AegpTime time{0, 1};
  AegpTime time_step{1, 30};
  int32_t field{};
  int32_t world_type{1};
  int16_t downsample_x{1};
  int16_t downsample_y{1};
  AegpRect roi{};
  int32_t matte{};
  int8_t channel_order{};
  uint8_t render_guide_layers{};
  int8_t render_quality{1};
};

using ItemValidator = bool(__cdecl*)(int32_t plugin_id, void* item);
using LayerInitializer = bool(__cdecl*)(int32_t plugin_id, void* source,
                                        LayerEffectBoundary boundary, LayerValue* value);

void configure_validators(ItemValidator item_validator,
                          LayerInitializer layer_initializer) noexcept;

bool snapshot_item(void* handle, ItemValue& value) noexcept;
bool snapshot_layer(void* handle, LayerValue& value) noexcept;
int32_t insert_layer_value(const LayerValue& value, void** output);
std::size_t item_live_count() noexcept;
std::size_t layer_live_count() noexcept;
uint32_t item_created_count() noexcept;
uint32_t item_disposed_count() noexcept;
uint32_t item_invalid_count() noexcept;
uint32_t layer_created_count() noexcept;
uint32_t layer_disposed_count() noexcept;
uint32_t layer_invalid_count() noexcept;

int32_t __cdecl render_options_new_from_item(int32_t, void*, void**);
int32_t __cdecl render_options_duplicate(int32_t, void*, void**);
int32_t __cdecl render_options_dispose(void*);
int32_t __cdecl render_options_set_time(void*, AegpTime);
int32_t __cdecl render_options_get_time(void*, AegpTime*);
int32_t __cdecl render_options_set_time_step(void*, AegpTime);
int32_t __cdecl render_options_get_time_step(void*, AegpTime*);
int32_t __cdecl render_options_set_field(void*, int32_t);
int32_t __cdecl render_options_get_field(void*, int32_t*);
int32_t __cdecl render_options_set_world_type(void*, int32_t);
int32_t __cdecl render_options_get_world_type(void*, int32_t*);
int32_t __cdecl render_options_set_downsample(void*, int16_t, int16_t);
int32_t __cdecl render_options_get_downsample(void*, int16_t*, int16_t*);
int32_t __cdecl render_options_set_roi(void*, const AegpRect*);
int32_t __cdecl render_options_get_roi(void*, AegpRect*);
int32_t __cdecl render_options_set_matte(void*, int32_t);
int32_t __cdecl render_options_get_matte(void*, int32_t*);
int32_t __cdecl render_options_set_channel_order(void*, int8_t);
int32_t __cdecl render_options_get_channel_order(void*, int8_t*);
int32_t __cdecl render_options_get_guide_layers(void*, uint8_t*);
int32_t __cdecl render_options_set_guide_layers(void*, uint8_t);
int32_t __cdecl render_options_get_quality(void*, int8_t*);
int32_t __cdecl render_options_set_quality(void*, int8_t);

int32_t __cdecl new_layer_render_options(int32_t, void*, void**);
int32_t __cdecl new_from_upstream_of_effect(int32_t, void*, void**);
int32_t __cdecl new_from_downstream_of_effect(int32_t, void*, void**);
int32_t __cdecl duplicate_layer_render_options(int32_t, void*, void**);
int32_t __cdecl dispose_layer_render_options(void*);
int32_t __cdecl set_layer_render_time(void*, AegpTime);
int32_t __cdecl get_layer_render_time(void*, AegpTime*);
int32_t __cdecl set_layer_render_time_step(void*, AegpTime);
int32_t __cdecl get_layer_render_time_step(void*, AegpTime*);
int32_t __cdecl set_layer_render_world_type(void*, int32_t);
int32_t __cdecl get_layer_render_world_type(void*, int32_t*);
int32_t __cdecl set_layer_render_downsample(void*, int16_t, int16_t);
int32_t __cdecl get_layer_render_downsample(void*, int16_t*, int16_t*);
int32_t __cdecl set_layer_render_matte(void*, int32_t);
int32_t __cdecl get_layer_render_matte(void*, int32_t*);

}  // namespace aexcompat::render_options
