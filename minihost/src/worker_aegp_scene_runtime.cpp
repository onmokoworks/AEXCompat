#include "worker_aegp_scene_runtime.hpp"

namespace {
SceneRuntimeContext g_context{};
bool g_configured{};
}

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
