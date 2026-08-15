#pragma once

#include <array>
#include <cstddef>
#include <cstdint>

#include "worker_aegp_scene_model.hpp"
#include "worker_suite_abi.hpp"

namespace aexcompat::scene_runtime {

struct AegpSceneObject { uint32_t tag{}; };

inline constexpr std::size_t kAegpEffectInstanceCapacity = 8;
inline constexpr std::size_t kAegpEffectLeaseCapacity = 16;
inline constexpr std::size_t kAegpEffectParameterCapacity = 12;
inline constexpr std::size_t kAegpLegacyEffectStreamCapacity = 16;
inline constexpr int32_t kAegpInstalledEffectKeyNone = 0;
inline constexpr std::size_t kAegpMaxEffectCategoryNameSize = 128;

enum class AegpItemSamplingPolicy : uint8_t {
  exact,
  hold,
  nearest,
};

struct AegpEffectInstance {
  void* layer{};
  int32_t installed_key{};
  int32_t stack_order{};
  uint32_t flags{1};
  uint32_t generation{};
  bool occupied{};
  // This instance stands for the plug-in this worker loaded, not for one of
  // the fixtures `installed_key` names. Slot 0 is seeded with the probe's key
  // at scene start and `AEGP_GetNewEffectForEffect` hands that same slot back,
  // so the key alone cannot tell the two apart - and answering a real
  // plug-in's stream questions out of a five-parameter fixture table is what
  // kept a 159-parameter effect from reaching its own parameters (issue #909).
  bool loaded_plugin{};
  void* render_ref{};
  std::array<std::array<double, 4>, kAegpEffectParameterCapacity> parameter_values{{
      {{42.5, 0.0, 0.0, 0.0}}, {{160.0, 90.0, 0.0, 0.0}},
      {{1.0, 2.0, 3.0, 0.0}}, {{0.25, 0.5, 0.75, 1.0}}}};
  scene_model::Identity identity{};
};

struct AegpEffectLease {
  int32_t owner_plugin_id{};
  uint32_t instance_index{};
  uint32_t instance_generation{};
  uint32_t generation{};
  bool live{};
  void* handle{};
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
  scene_model::Identity identity{};
  scene_model::Identity value_identity{};
  std::array<scene_model::Identity, 2> keyframe_identities{};
  void* handle{};
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
  scene_model::Identity identity{};
  scene_model::Identity value_identity{};
  void* handle{};
};

struct AegpSelectionCollection {
  uint32_t tag{0x434f4c4c};
  bool live{};
};

// Authored transform data used by the bounded single-layer scene contract.
// Position and anchor are composition pixels; scale is an AE-style percent.
struct AegpLayerTransform {
  std::array<double, 3> anchor{};
  std::array<double, 3> position{};
  std::array<double, 3> scale{{100.0, 100.0, 100.0}};
  std::array<double, 3> rotation_degrees{};
  bool is_3d{};
};

struct AegpLayerTransformKeyframe {
  aexcompat::suite_abi::AegpTime time{};
  AegpLayerTransform transform{};
  bool valid{};
};

struct AegpCameraZoomKeyframe {
  aexcompat::suite_abi::AegpTime time{};
  double zoom{};
  bool valid{};
};

// All synthetic-scene identity and lease state has one owning translation
// unit. Callbacks obtain it through this accessor; no SDK-shaped handle is
// ever reconstituted from a second scene copy.
struct SceneRuntimeState {
  AegpSceneObject composition_item{0x4954454d};
  uint64_t composition_item_identity{1001};
  AegpItemSamplingPolicy composition_item_sampling_policy{
      AegpItemSamplingPolicy::exact};
  std::array<void*, 3> composition_item_dependencies{};
  std::size_t composition_item_dependency_count{};
  AegpSceneObject composition{0x434f4d50};
  std::array<AegpSceneObject, 3> layers{{
      {0x4c415930}, {0x4c415931}, {0x4c415932}}};
  AegpSceneObject dynamic_camera{0x43414d52};
  bool dynamic_camera_live{};
  bool scene_registry_initialized{};
  scene_model::Identity dynamic_camera_identity{};
  double dynamic_camera_zoom{800.0};
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

  bool update_menu_mode{};
  bool command_roundtrip_mode{};
  bool active_idle_roundtrip_mode{};
  bool comp_idle_roundtrip_mode{};
  uint32_t item_current_time_calls{};
  uint32_t item_set_current_time_calls{};
  int32_t item_last_set_time_value{-1};
  uint32_t item_last_set_time_scale{};
  uint32_t item_name_calls{};
  uint32_t item_duration_calls{};
  uint32_t item_type_calls{};
  uint32_t comp_from_item_calls{};
  uint32_t comp_framerate_calls{};
  uint32_t layer_count_calls{};
  uint32_t layer_by_index_calls{};
  uint32_t layer_source_item_calls{};
  uint32_t layer_id_calls{};
  uint32_t layer_attribute_calls{};
  uint32_t layer_trim_set_calls{};
  uint32_t layer_flag_set_calls{};
  std::array<uint32_t, 3> layer_flags{{0x00000005u, 0x00000005u, 0x00000005u}};
  uint32_t layer_name_calls{};
  uint32_t effect_count_calls{};
  uint32_t effect_acquires{};
  uint32_t effect_disposes{};
  uint32_t effect_metadata_calls{};
  uint32_t stream_acquires{};
  uint32_t stream_disposes{};
  uint32_t stream_value_acquires{};
  uint32_t stream_value_disposes{};
  uint32_t stream_sampled_selector_mask{};
  uint32_t effect_param_name_calls{};
  uint32_t effect_param_value_calls{};
  uint32_t effect_param_union_calls{};
  uint32_t keyframe_count_calls{};
  uint32_t keyframed_stream_reports{};
  uint32_t keyframe_time_calls{};
  uint32_t keyframe_value_calls{};
  uint32_t keyframe_interpolation_calls{};
  uint32_t collection_creates{};
  uint32_t collection_disposes{};
  uint32_t collection_item_reads{};
  int32_t first_observed_frame{-1};
  int32_t last_observed_frame{-1};
  std::array<aexcompat::suite_abi::AegpTime, 3> layer_in_points{{
      {0, 30}, {0, 30}, {0, 30}}};
  std::array<aexcompat::suite_abi::AegpTime, 3> layer_durations{{
      {300, 30}, {300, 30}, {300, 30}}};
  std::array<AegpLayerTransform, 3> layer_transforms{};
  std::array<std::array<AegpLayerTransformKeyframe, 2>, 3> layer_transform_keyframes{};
  // Zero keeps the historical composition-width fallback until a camera
  // supplies an authored zoom value.
  std::array<double, 3> layer_camera_zoom{};
  std::array<std::array<AegpCameraZoomKeyframe, 2>, 3> layer_camera_zoom_keyframes{};
  std::array<int32_t, 3> layer_parent_indices{{-1, -1, -1}};
  int32_t active_camera_layer_index{-1};
  AegpSelectionCollection selection{};

  SceneRuntimeState() noexcept;
};

SceneRuntimeState& scene_runtime_state() noexcept;
void* composition_item_handle() noexcept;
void* composition_handle() noexcept;
bool update_composition_item_render_metadata(
    void* item, uint64_t stable_identity, AegpItemSamplingPolicy policy,
    void* const* dependencies, std::size_t dependency_count) noexcept;

extern const std::array<AegpEffectParameterRecord, 5> kAegpProbeParameters;
extern const std::array<AegpEffectParameterRecord, 7> kAegpLevelsParameters;
extern const std::array<AegpInstalledEffectRecord, 3> kAegpInstalledEffects;

// Host services retained by the independently compiled AEGP scene runtime.
// Mutable scene state and suite tables are owned by SceneRuntimeState and the
// scene translation unit; the host supplies only lifecycle checks and stable
// object identities through this context.
struct SceneRuntimeHostHooks {
  bool (__cdecl *suite_lease_balanced)(){};
};

struct SceneRuntimeContext {
  SceneRuntimeHostHooks hooks{};
  void* composition_item{};
  void* composition{};
  int32_t* full_resolution_width{};
  int32_t* full_resolution_height{};
  int32_t (__cdecl *smart_width)(){};
  int32_t (__cdecl *smart_height)(){};
};

bool configure_scene_runtime_context(const SceneRuntimeContext& context) noexcept;
const SceneRuntimeContext* scene_runtime_context() noexcept;
bool scene_runtime_translation_unit_linked() noexcept;

}  // namespace aexcompat::scene_runtime
