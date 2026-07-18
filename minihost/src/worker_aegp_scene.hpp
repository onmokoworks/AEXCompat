#pragma once

#include <cstdint>

// Private boundary for the clean-room scene worker.  Callback addresses keep
// their exact __cdecl ABI; worker-owned state is supplied explicitly.
struct SceneHostHooks {
  void (__cdecl *bump_project_timestamp)(){};
  bool (__cdecl *validate_render_options_item)(int32_t, void*){};
  bool (__cdecl *initialize_layer_render_options)(int32_t, void*, int32_t,
                                                  void*){};
  bool (__cdecl *suite_lease_balanced)(){};
};

struct SceneContext {
  SceneHostHooks hooks{};
  void* composition_item{};
  void* composition{};
  int32_t* full_resolution_width{};
  int32_t* full_resolution_height{};
  int32_t* smart_width{};
  int32_t* smart_height{};
};

bool configure_scene_context(const SceneContext& context) noexcept;
const SceneContext* scene_context() noexcept;
bool scene_translation_unit_linked() noexcept;
bool scene_selftests_translation_unit_linked() noexcept;

// Ownership boundary for the clean-room AEGP scene family.
//
// The implementation deliberately has no public SDK-shaped API. Suite tables
// remain private to the worker and are leased through the existing BasicSuite
// adapter in l2_main.cpp. This header is therefore a boundary marker rather
// than an ABI surface; adding declarations here requires an explicit review of
// calling convention, handle provenance, and lifetime semantics.
inline constexpr unsigned kAegpSceneEffectInstanceLimit = 8;
inline constexpr unsigned kAegpSceneEffectLeaseLimit = 16;
inline constexpr unsigned kAegpSceneLegacyEffectStreamLimit = 16;
