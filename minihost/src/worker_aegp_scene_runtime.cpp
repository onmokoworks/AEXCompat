#include "worker_aegp_scene_runtime.hpp"

namespace aexcompat::scene_runtime {
namespace {
SceneRuntimeContext g_context{};
bool g_configured{};
}

SceneRuntimeState::SceneRuntimeState() noexcept {
  composition_item_identity = 1001;
  composition_item_sampling_policy = AegpItemSamplingPolicy::exact;
  composition_item_dependencies = {};
  composition_item_dependency_count = 0;
  for (auto& transform : layer_transforms)
    transform.scale = {{100.0, 100.0, 100.0}};
  layer_parent_indices = {{-1, -1, -1}};
  effect_instances[0] = {&layers[0], 3001, 0, 1, 1, true};
}

SceneRuntimeState& scene_runtime_state() noexcept {
  static SceneRuntimeState state{};
  return state;
}

void* composition_item_handle() noexcept {
  return &scene_runtime_state().composition_item;
}

void* composition_handle() noexcept {
  return &scene_runtime_state().composition;
}

bool update_composition_item_render_metadata(
    void* item, uint64_t stable_identity, AegpItemSamplingPolicy policy,
    void* const* dependencies, std::size_t dependency_count) noexcept {
  auto& state = scene_runtime_state();
  if (item != &state.composition_item || stable_identity == 0 ||
      static_cast<uint8_t>(policy) >
          static_cast<uint8_t>(AegpItemSamplingPolicy::nearest) ||
      dependency_count > state.composition_item_dependencies.size() ||
      (dependency_count != 0 && !dependencies))
    return false;
  for (std::size_t index = 0; index < dependency_count; ++index)
    if (!dependencies[index]) return false;
  state.composition_item_identity = stable_identity;
  state.composition_item_sampling_policy = policy;
  state.composition_item_dependencies = {};
  state.composition_item_dependency_count = dependency_count;
  for (std::size_t index = 0; index < dependency_count; ++index)
    state.composition_item_dependencies[index] = dependencies[index];
  return true;
}

const std::array<AegpEffectParameterRecord, 5> kAegpProbeParameters{{
    {"Input", 9, {}, false}, {"Amount", 5, {{42.5, 0.0, 0.0, 0.0}}, true},
    {"Center", 4, {{160.0, 90.0, 0.0, 0.0}}, true},
    {"Vector", 2, {{1.0, 2.0, 3.0, 0.0}}, true},
    {"Tint", 6, {{0.25, 0.5, 0.75, 1.0}}, true}}};
const std::array<AegpEffectParameterRecord, 7> kAegpLevelsParameters{{
    {"Input", 9, {}, false}, {"Channel", 5, {}, true},
    {"Histogram", 5, {}, false}, {"Reset", 5, {}, false},
    {"Input Black", 5, {{0.0, 0.0, 0.0, 0.0}}, true},
    {"Input White", 5, {{1.0, 0.0, 0.0, 0.0}}, true},
    {"Gamma", 5, {{1.0, 0.0, 0.0, 0.0}}, true}}};
const std::array<AegpInstalledEffectRecord, 3> kAegpInstalledEffects{{
    {3001, "AEXCompat Probe", "AEXCompat.Probe", "AEXCompat", 5},
    {3002, "Levels", "ADBE Easy Levels", "Color Correction", 7},
    {3003, "Levels (Individual Controls)", "ADBE Pro Levels", "Color Correction", 7}}};
static_assert(sizeof("AEXCompat") <= kAegpMaxEffectCategoryNameSize);

bool configure_scene_runtime_context(const SceneRuntimeContext& context) noexcept {
  if (!context.hooks.suite_lease_balanced || !context.composition_item ||
      !context.composition || !context.full_resolution_width ||
      !context.full_resolution_height || !context.smart_width ||
      !context.smart_height)
    return false;
  g_context = context;
  g_configured = true;
  return true;
}

const SceneRuntimeContext* scene_runtime_context() noexcept {
  return g_configured ? &g_context : nullptr;
}

bool scene_runtime_translation_unit_linked() noexcept {
  return scene_runtime_context() != nullptr;
}

}  // namespace aexcompat::scene_runtime
