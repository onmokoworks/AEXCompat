// Copyright (c) AEXCompat contributors.
// Explicit host-context boundary compiled into every worker target.

#include "worker_aegp_scene.hpp"

namespace {
SceneContext g_scene_context{};
bool g_scene_context_configured{};
}

bool configure_scene_context(const SceneContext& context) noexcept {
  if (!context.hooks.bump_project_timestamp ||
      !context.hooks.validate_render_options_item ||
      !context.hooks.initialize_layer_render_options ||
      !context.hooks.suite_lease_balanced ||
      !context.composition_item || !context.composition ||
      !context.full_resolution_width || !context.full_resolution_height ||
      !context.smart_width || !context.smart_height) return false;
  g_scene_context = context;
  g_scene_context_configured = true;
  return true;
}

const SceneContext* scene_context() noexcept {
  return g_scene_context_configured ? &g_scene_context : nullptr;
}

bool scene_translation_unit_linked() noexcept {
  return scene_context() != nullptr;
}
