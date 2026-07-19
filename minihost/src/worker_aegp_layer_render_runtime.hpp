#pragma once

#include "worker_aegp_async_layer_runtime.hpp"
#include "worker_aegp_render_options.hpp"
#include "worker_selector_dispatch.hpp"

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace aexcompat::aegp_layer_render_runtime {

struct Context {
  worker_runtime::EffectEntry entry{};
  void* input{};
  void* output{};
  int32_t current_time{};
  int32_t time_scale{1};
  std::string case_id{"default"};
  const void* requested{};
  const std::vector<unsigned char>* external_rgba{};
  const void* external_layers{};
  int32_t external_width{};
  int32_t external_height{};
  int32_t time_step{1};
  int32_t total_time{1};
  int32_t pixel_bytes{4};
  const std::vector<unsigned char>* source_argb{};
  int32_t source_width{};
  int32_t source_height{};
  const std::vector<unsigned char>* downstream_argb{};
  int32_t downstream_width{};
  int32_t downstream_height{};
  int32_t downstream_pixel_bytes{};
  bool downstream_finalized{};
  const std::vector<unsigned char>* all_effects_argb{};
  int32_t all_effects_width{};
  int32_t all_effects_height{};
  int32_t all_effects_pixel_bytes{};
  bool all_effects_finalized{};
  int32_t active_item_time{};
  bool active_item_time_valid{};
};

struct Hooks {
  bool (*is_render_worker)();
  bool (*effect_boundary_live)(const render_options::LayerValue& options);
};

void configure(Hooks hooks) noexcept;
Context& context() noexcept;
bool active() noexcept;
Context replace_context(Context next);
void clear_context() noexcept;

int32_t publish_from_context(const Context& context,
                             const render_options::LayerValue& options, void** receipt);
int32_t publish(const render_options::LayerValue& options, void** receipt);
bool capture_async_source(const render_options::LayerValue& options,
                          aegp_async_layer::SourceSnapshot& output);
int32_t publish_async_source(const aegp_async_layer::SourceSnapshot& source,
                             const render_options::LayerValue& options, void** receipt);

}  // namespace aexcompat::aegp_layer_render_runtime
