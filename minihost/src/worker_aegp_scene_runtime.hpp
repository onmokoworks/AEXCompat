#pragma once

#include <array>
#include <cstddef>
#include <cstdint>

namespace aexcompat::scene_runtime {

struct AegpSceneObject { uint32_t tag{}; };

inline constexpr std::size_t kAegpEffectInstanceCapacity = 8;
inline constexpr std::size_t kAegpEffectLeaseCapacity = 16;
inline constexpr std::size_t kAegpEffectParameterCapacity = 12;
inline constexpr std::size_t kAegpLegacyEffectStreamCapacity = 16;
inline constexpr int32_t kAegpInstalledEffectKeyNone = 0;
inline constexpr std::size_t kAegpMaxEffectCategoryNameSize = 128;

struct AegpEffectInstance {
  void* layer{};
  int32_t installed_key{};
  int32_t stack_order{};
  uint32_t flags{1};
  uint32_t generation{};
  bool occupied{};
  std::array<std::array<double, 4>, kAegpEffectParameterCapacity> parameter_values{{
      {{42.5, 0.0, 0.0, 0.0}}, {{160.0, 90.0, 0.0, 0.0}},
      {{1.0, 2.0, 3.0, 0.0}}, {{0.25, 0.5, 0.75, 1.0}}}};
};

struct AegpEffectLease {
  int32_t owner_plugin_id{};
  uint32_t instance_index{};
  uint32_t instance_generation{};
  uint32_t generation{};
  bool live{};
};

struct AegpInstalledEffectRecord {
  int32_t key{};
  const char* name{};
  const char* match_name{};
  const char* category{};
  int32_t parameter_count{};
};

struct AegpEffectParameterRecord {
  const char* name{};
  int32_t type{};
  std::array<double, 4> default_value{};
  bool writable{};
};

struct AegpStreamValue {
  void* stream{};
  std::array<std::byte, 32> value{};
};
static_assert(sizeof(AegpStreamValue) == 40);

struct AegpTransformStream {
  AegpSceneObject object{0x5354524d};
  int32_t selector{-1};
  void* layer{};
  bool effect_param{};
  bool live{};
  bool value_live{};
  uint32_t effect_instance_index{};
  uint32_t effect_instance_generation{};
  int32_t owner_plugin_id{};
};

struct AegpLegacyEffectStream {
  int32_t param_index{-1};
  bool live{};
  bool hidden{};
  bool value_live{};
  AegpStreamValue* checked_out_value{};
  uint32_t effect_instance_index{};
  uint32_t effect_instance_generation{};
  uint32_t generation{};
  int32_t owner_plugin_id{};
};

// All synthetic-scene identity and lease state has one owning translation
// unit. Callbacks obtain it through this accessor; no SDK-shaped handle is
// ever reconstituted from a second scene copy.
struct SceneRuntimeState {
  AegpSceneObject composition_item{0x4954454d};
  AegpSceneObject composition{0x434f4d50};
  std::array<AegpSceneObject, 3> layers{{
      {0x4c415930}, {0x4c415931}, {0x4c415932}}};
  AegpSceneObject effect{0x45464643};
  int32_t scene_frame{1};
  bool effect_live{};
  std::array<AegpEffectInstance, kAegpEffectInstanceCapacity> effect_instances{};
  std::array<AegpEffectLease, kAegpEffectLeaseCapacity> effect_leases{};
  uint32_t effect_lease_generation{};
  AegpTransformStream transform_stream{};
  std::array<AegpLegacyEffectStream, kAegpLegacyEffectStreamCapacity>
      legacy_effect_streams{};
  uint32_t legacy_effect_stream_generation{};

  SceneRuntimeState() noexcept;
};

SceneRuntimeState& scene_runtime_state() noexcept;
void* composition_item_handle() noexcept;
void* composition_handle() noexcept;

extern const std::array<AegpEffectParameterRecord, 5> kAegpProbeParameters;
extern const std::array<AegpEffectParameterRecord, 7> kAegpLevelsParameters;
extern const std::array<AegpInstalledEffectRecord, 3> kAegpInstalledEffects;

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

}  // namespace aexcompat::scene_runtime
