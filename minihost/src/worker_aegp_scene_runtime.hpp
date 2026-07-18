#pragma once

#include <cstdint>

// Staging boundary for the AEGP scene runtime.  It intentionally owns no
// scene state yet: the first extraction step records the complete dependency
// set before callbacks are moved out of l2_main.cpp.
//
// The following groups move together in the next steps because their lifetime
// checks are coupled: effect instance/lease state, legacy Stream Suite v2
// wrappers, installed-effect catalog metadata, suite tables, and native
// scene verifiers.  Splitting an individual group would reintroduce stale
// handles or duplicate render-options ownership.
struct SceneRuntimeHostHooks {
  bool (__cdecl *suite_lease_balanced)(){};
};

struct SceneRuntimeContext {
  SceneRuntimeHostHooks hooks{};
  void* composition_item{};
  void* composition{};
  int32_t* full_resolution_width{};
  int32_t* full_resolution_height{};
  int32_t* smart_width{};
  int32_t* smart_height{};
};

bool configure_scene_runtime_context(const SceneRuntimeContext& context) noexcept;
const SceneRuntimeContext* scene_runtime_context() noexcept;
bool scene_runtime_translation_unit_linked() noexcept;
