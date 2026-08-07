// Copyright (c) AEXCompat contributors.
// Independent compiled implementation for the AEGP scene family.

#include "worker_aegp_scene.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_extended_diag.hpp"
#include <iostream>
#include "worker_aegp_external_render_runtime.hpp"
#include "worker_aegp_scene_model.hpp"
#include "worker_aegp_scene_transaction.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_suite_registry.hpp"

#include <algorithm>
#include <climits>
#include <cmath>
#include <cstring>
#include <iterator>
#include <limits>
#include <numeric>
#include <utility>

using aexcompat::scene_runtime::scene_runtime_state;
using aexcompat::scene_model::Identity;
using aexcompat::scene_model::ItemKind;
using aexcompat::scene_model::ObjectKind;
using aexcompat::scene_model::ObjectSnapshot;
using aexcompat::worker_runtime::UnsupportedSuiteId;
using aexcompat::worker_runtime::unsupported_suite_slots;

namespace {
SceneContext g_scene_context{};
bool g_scene_context_configured{};

SceneRuntimeState& state() noexcept { return scene_runtime_state(); }
AegpSceneObject& comp_item() noexcept { return state().composition_item; }
AegpSceneObject& comp() noexcept { return state().composition; }
auto& layers() noexcept { return state().layers; }
AegpSceneObject& effect_object() noexcept { return state().effect; }

bool valid_comp_time(const AegpTime& time) {
  if (time.scale == 0) return false;
  constexpr int64_t kCompDurationValue = 300;
  constexpr uint32_t kCompDurationScale = 30;
  const int64_t scaled_time = static_cast<int64_t>(time.value) * kCompDurationScale;
  const int64_t scaled_duration = kCompDurationValue * static_cast<int64_t>(time.scale);
  return scaled_time >= 0 && scaled_time < scaled_duration;
}

bool layer_active_at_time(std::size_t index, const AegpTime& time) {
  if (index >= state().layer_in_points.size() ||
      index >= state().layer_durations.size()) return false;
  const auto& in_point = state().layer_in_points[index];
  const auto& duration = state().layer_durations[index];
  if (in_point.scale == 0 || duration.scale == 0 || duration.value <= 0) return false;
  const long double seconds = static_cast<long double>(time.value) / time.scale;
  const long double in_seconds = static_cast<long double>(in_point.value) / in_point.scale;
  const long double duration_seconds = static_cast<long double>(duration.value) / duration.scale;
  return seconds >= in_seconds && seconds < in_seconds + duration_seconds;
}

void set_identity(AegpMatrix4& matrix) {
  matrix = {};
  for (std::size_t index = 0; index < 4; ++index) matrix.mat[index][index] = 1.0;
}

AegpMatrix4 multiply(const AegpMatrix4& left, const AegpMatrix4& right) {
  AegpMatrix4 result{};
  for (std::size_t row = 0; row < 4; ++row) {
    for (std::size_t column = 0; column < 4; ++column) {
      for (std::size_t index = 0; index < 4; ++index)
        result.mat[row][column] += left.mat[row][index] * right.mat[index][column];
    }
  }
  return result;
}

bool finite_bounded(double value, double limit) {
  return std::isfinite(value) && std::abs(value) <= limit;
}

bool resolve_layer_transform(std::size_t index, const AegpTime& comp_time,
    AegpLayerTransform& output) {
  if (index >= g_aegp_layer_transforms.size() || !valid_comp_time(comp_time)) return false;
  const auto& keyframes = state().layer_transform_keyframes[index];
  if (!keyframes[0].valid && !keyframes[1].valid) {
    output = g_aegp_layer_transforms[index];
    return true;
  }
  if (!keyframes[0].valid || !keyframes[1].valid ||
      !valid_comp_time(keyframes[0].time) || !valid_comp_time(keyframes[1].time) ||
      keyframes[0].transform.is_3d != keyframes[1].transform.is_3d) return false;
  const long double first_time = static_cast<long double>(keyframes[0].time.value) /
      static_cast<long double>(keyframes[0].time.scale);
  const long double second_time = static_cast<long double>(keyframes[1].time.value) /
      static_cast<long double>(keyframes[1].time.scale);
  const long double current_time = static_cast<long double>(comp_time.value) /
      static_cast<long double>(comp_time.scale);
  if (!std::isfinite(first_time) || !std::isfinite(second_time) ||
      !std::isfinite(current_time) || !(first_time < second_time)) return false;
  if (current_time <= first_time) {
    output = keyframes[0].transform;
    return true;
  }
  if (current_time >= second_time) {
    output = keyframes[1].transform;
    return true;
  }
  const long double alpha = (current_time - first_time) / (second_time - first_time);
  if (!std::isfinite(alpha) || alpha < 0.0L || alpha > 1.0L) return false;
  output = keyframes[0].transform;
  const auto blend = [alpha](std::array<double, 3>& destination,
      const std::array<double, 3>& first, const std::array<double, 3>& second) {
    for (std::size_t component = 0; component < 3; ++component) {
      destination[component] = first[component] +
          static_cast<double>(alpha) * (second[component] - first[component]);
    }
  };
  blend(output.anchor, keyframes[0].transform.anchor, keyframes[1].transform.anchor);
  blend(output.position, keyframes[0].transform.position, keyframes[1].transform.position);
  blend(output.scale, keyframes[0].transform.scale, keyframes[1].transform.scale);
  blend(output.rotation_degrees, keyframes[0].transform.rotation_degrees,
      keyframes[1].transform.rotation_degrees);
  return true;
}

bool resolve_layer_camera_zoom(std::size_t index, const AegpTime& comp_time,
    double fallback, double& output) {
  constexpr double kZoomLimit = 1000000000.0;
  if (index >= state().layer_camera_zoom.size() || !valid_comp_time(comp_time) ||
      !finite_bounded(fallback, kZoomLimit) || fallback <= 0.0) return false;
  const auto& keyframes = state().layer_camera_zoom_keyframes[index];
  const auto valid_zoom = [=](double value) {
    return finite_bounded(value, kZoomLimit) && value > 0.0;
  };
  if (!keyframes[0].valid && !keyframes[1].valid) {
    const double authored = state().layer_camera_zoom[index];
    if (authored == 0.0) {
      output = fallback;
      return true;
    }
    if (!valid_zoom(authored)) return false;
    output = authored;
    return true;
  }
  if (!keyframes[0].valid || !keyframes[1].valid ||
      !valid_comp_time(keyframes[0].time) || !valid_comp_time(keyframes[1].time) ||
      !valid_zoom(keyframes[0].zoom) || !valid_zoom(keyframes[1].zoom)) return false;
  const long double first_time = static_cast<long double>(keyframes[0].time.value) /
      static_cast<long double>(keyframes[0].time.scale);
  const long double second_time = static_cast<long double>(keyframes[1].time.value) /
      static_cast<long double>(keyframes[1].time.scale);
  const long double current_time = static_cast<long double>(comp_time.value) /
      static_cast<long double>(comp_time.scale);
  if (!std::isfinite(first_time) || !std::isfinite(second_time) ||
      !std::isfinite(current_time) || !(first_time < second_time)) return false;
  if (current_time <= first_time) {
    output = keyframes[0].zoom;
    return true;
  }
  if (current_time >= second_time) {
    output = keyframes[1].zoom;
    return true;
  }
  const long double alpha = (current_time - first_time) / (second_time - first_time);
  if (!std::isfinite(alpha) || alpha < 0.0L || alpha > 1.0L) return false;
  output = keyframes[0].zoom +
      static_cast<double>(alpha) * (keyframes[1].zoom - keyframes[0].zoom);
  return valid_zoom(output);
}

bool build_layer_transform(const AegpLayerTransform& authored, AegpMatrix4& output) {
  constexpr double kLinearLimit = 1000000.0;
  constexpr double kRotationLimit = 360000.0;
  for (std::size_t index = 0; index < 3; ++index) {
    if (!finite_bounded(authored.anchor[index], kLinearLimit) ||
        !finite_bounded(authored.position[index], kLinearLimit) ||
        !finite_bounded(authored.scale[index], kLinearLimit) ||
        !finite_bounded(authored.rotation_degrees[index], kRotationLimit) ||
        authored.scale[index] == 0.0) return false;
  }

  std::array<double, 3> position = authored.position;
  std::array<double, 3> rotation = authored.rotation_degrees;
  if (!authored.is_3d) {
    position[2] = 0.0;
    rotation[0] = 0.0;
    rotation[1] = 0.0;
  }
  constexpr double kPi = 3.141592653589793238462643383279502884;
  const std::array<double, 3> radians{{
      rotation[0] * kPi / 180.0,
      rotation[1] * kPi / 180.0,
      rotation[2] * kPi / 180.0}};
  for (double value : radians)
    if (!std::isfinite(value)) return false;

  AegpMatrix4 translation{};
  set_identity(translation);
  translation.mat[0][3] = position[0];
  translation.mat[1][3] = position[1];
  translation.mat[2][3] = position[2];

  AegpMatrix4 scale{};
  set_identity(scale);
  scale.mat[0][0] = authored.scale[0] / 100.0;
  scale.mat[1][1] = authored.scale[1] / 100.0;
  scale.mat[2][2] = authored.scale[2] / 100.0;

  AegpMatrix4 rotate_x{};
  AegpMatrix4 rotate_y{};
  AegpMatrix4 rotate_z{};
  set_identity(rotate_x);
  set_identity(rotate_y);
  set_identity(rotate_z);
  const double sin_x = std::sin(radians[0]);
  const double cos_x = std::cos(radians[0]);
  const double sin_y = std::sin(radians[1]);
  const double cos_y = std::cos(radians[1]);
  const double sin_z = std::sin(radians[2]);
  const double cos_z = std::cos(radians[2]);
  rotate_x.mat[1][1] = cos_x;
  rotate_x.mat[1][2] = -sin_x;
  rotate_x.mat[2][1] = sin_x;
  rotate_x.mat[2][2] = cos_x;
  rotate_y.mat[0][0] = cos_y;
  rotate_y.mat[0][2] = sin_y;
  rotate_y.mat[2][0] = -sin_y;
  rotate_y.mat[2][2] = cos_y;
  rotate_z.mat[0][0] = cos_z;
  rotate_z.mat[0][1] = -sin_z;
  rotate_z.mat[1][0] = sin_z;
  rotate_z.mat[1][1] = cos_z;

  AegpMatrix4 negative_anchor{};
  set_identity(negative_anchor);
  negative_anchor.mat[0][3] = -authored.anchor[0];
  negative_anchor.mat[1][3] = -authored.anchor[1];
  negative_anchor.mat[2][3] = -authored.anchor[2];

  // Row-major matrices multiply column vectors: T(position) * Rz * Ry * Rx
  // * S(scale / 100) * T(-anchor), matching the authored AE transform order.
  const AegpMatrix4 result = multiply(
      multiply(multiply(multiply(translation, rotate_z), rotate_y), rotate_x),
      multiply(scale, negative_anchor));
  for (std::size_t row = 0; row < 4; ++row)
    for (std::size_t column = 0; column < 4; ++column)
      if (!std::isfinite(result.mat[row][column])) return false;
  output = result;
  return true;
}

bool build_layer_world_transform(std::size_t index, const AegpTime& comp_time,
    AegpMatrix4& output) {
  constexpr std::size_t kMaxParentDepth = 8;
  if (index >= g_aegp_layer_transforms.size()) return false;
  std::array<bool, 3> visited{};
  AegpMatrix4 world{};
  set_identity(world);
  std::size_t current = index;
  for (std::size_t depth = 0; depth < kMaxParentDepth; ++depth) {
    if (current >= g_aegp_layer_transforms.size() || visited[current]) return false;
    visited[current] = true;
    AegpLayerTransform authored{};
    if (!resolve_layer_transform(current, comp_time, authored)) return false;
    AegpMatrix4 local{};
    if (!build_layer_transform(authored, local)) return false;
    world = multiply(local, world);
    const int32_t parent = g_aegp_layer_parent_indices[current];
    if (parent == -1) {
      output = world;
      return true;
    }
    if (parent < 0 || static_cast<std::size_t>(parent) >=
        g_aegp_layer_parent_indices.size()) return false;
    current = static_cast<std::size_t>(parent);
  }
  return false;
}
}  // namespace

bool configure_scene_context(const SceneContext& context) noexcept {
  if (!context.hooks.bump_project_timestamp ||
      !context.hooks.validate_render_options_item ||
      !context.hooks.initialize_layer_render_options ||
      !context.hooks.suite_lease_balanced ||
      !context.hooks.make_utf16_handle || !context.hooks.free_mem_handle ||
      !context.hooks.suite_factory.render_scene_enabled ||
      !context.composition_item || !context.composition || !context.pf_layer ||
      !context.pf_effect || !context.full_resolution_width ||
      !context.full_resolution_height || !context.smart_width ||
      !context.smart_height) return false;
  g_scene_context = context;
  g_scene_context_configured = true;
  auto& runtime = scene_runtime_state();
  runtime.effect_instances[0].render_ref = context.pf_effect;
  if (!update_composition_item_render_metadata(
          context.composition_item, runtime.composition_item_identity,
          runtime.composition_item_sampling_policy,
          runtime.composition_item_dependencies.data(),
          runtime.composition_item_dependency_count))
    return false;
  return true;
}

const SceneContext* scene_context() noexcept {
  return g_scene_context_configured ? &g_scene_context : nullptr;
}

bool scene_translation_unit_linked() noexcept {
  return scene_context() != nullptr;
}

// State and catalog records live in worker_aegp_scene_runtime.cpp.
auto& g_aegp_update_menu_mode = state().update_menu_mode;
auto& g_aegp_command_roundtrip_mode = state().command_roundtrip_mode;
auto& g_aegp_comp_idle_roundtrip_mode = state().comp_idle_roundtrip_mode;
auto& g_aegp_item_current_time_calls = state().item_current_time_calls;
auto& g_aegp_item_set_current_time_calls = state().item_set_current_time_calls;
auto& g_aegp_item_last_set_time_value = state().item_last_set_time_value;
auto& g_aegp_item_last_set_time_scale = state().item_last_set_time_scale;
auto& g_aegp_item_name_calls = state().item_name_calls;
auto& g_aegp_item_duration_calls = state().item_duration_calls;
auto& g_aegp_item_type_calls = state().item_type_calls;
auto& g_aegp_comp_from_item_calls = state().comp_from_item_calls;
auto& g_aegp_comp_framerate_calls = state().comp_framerate_calls;
auto& g_aegp_layer_count_calls = state().layer_count_calls;
auto& g_aegp_layer_by_index_calls = state().layer_by_index_calls;
auto& g_aegp_layer_source_item_calls = state().layer_source_item_calls;
auto& g_aegp_layer_id_calls = state().layer_id_calls;
auto& g_aegp_layer_attribute_calls = state().layer_attribute_calls;
auto& g_aegp_layer_trim_set_calls = state().layer_trim_set_calls;
auto& g_aegp_layer_flag_set_calls = state().layer_flag_set_calls;
auto& g_aegp_layer_flags = state().layer_flags;
auto& g_aegp_layer_name_calls = state().layer_name_calls;
auto& g_aegp_effect_count_calls = state().effect_count_calls;
auto& g_aegp_effect_acquires = state().effect_acquires;
auto& g_aegp_effect_disposes = state().effect_disposes;
auto& g_aegp_effect_metadata_calls = state().effect_metadata_calls;
auto& g_aegp_stream_acquires = state().stream_acquires;
auto& g_aegp_stream_disposes = state().stream_disposes;
auto& g_aegp_stream_value_acquires = state().stream_value_acquires;
auto& g_aegp_stream_value_disposes = state().stream_value_disposes;
auto& g_aegp_stream_sampled_selector_mask = state().stream_sampled_selector_mask;
auto& g_aegp_effect_param_name_calls = state().effect_param_name_calls;
auto& g_aegp_effect_param_value_calls = state().effect_param_value_calls;
auto& g_aegp_keyframe_count_calls = state().keyframe_count_calls;
auto& g_aegp_keyframed_stream_reports = state().keyframed_stream_reports;
auto& g_aegp_keyframe_time_calls = state().keyframe_time_calls;
auto& g_aegp_keyframe_value_calls = state().keyframe_value_calls;
auto& g_aegp_keyframe_interpolation_calls = state().keyframe_interpolation_calls;
auto& g_aegp_collection_creates = state().collection_creates;
auto& g_aegp_collection_disposes = state().collection_disposes;
auto& g_aegp_collection_item_reads = state().collection_item_reads;
auto& g_aegp_scene_frame = state().scene_frame;
auto& g_aegp_first_observed_frame = state().first_observed_frame;
auto& g_aegp_last_observed_frame = state().last_observed_frame;
auto& g_aegp_active_camera_layer_index = state().active_camera_layer_index;
auto& g_aegp_selection = state().selection;
auto& g_aegp_comp_item = state().composition_item;
auto& g_aegp_comp = state().composition;
auto& g_aegp_layers = state().layers;
auto& g_aegp_effect = state().effect;

#define g_effect (*static_cast<AegpSceneObject*>(scene_context()->pf_effect))
#define g_layer (*static_cast<AegpSceneObject*>(scene_context()->pf_layer))
#define g_full_resolution_width (*scene_context()->full_resolution_width)
#define g_full_resolution_height (*scene_context()->full_resolution_height)
#define g_smart_width (scene_context()->smart_width())
#define g_smart_height (scene_context()->smart_height())
#define bump_render_project_timestamp() scene_context()->hooks.bump_project_timestamp()
#define make_utf16_handle(...) scene_context()->hooks.make_utf16_handle(__VA_ARGS__)
#define free_aegp_mem_handle(...) scene_context()->hooks.free_mem_handle(__VA_ARGS__)

aexcompat::scene_model::Registry& scene_registry() noexcept {
  return aexcompat::scene_model::registry();
}

bool resolve_scene_item(void* handle, ObjectSnapshot& output,
                        uint64_t required_project_id = 0) noexcept {
  return scene_registry().resolve_item_or_legacy(
      handle, output, required_project_id);
}

bool resolve_scene_comp(void* handle, ObjectSnapshot& output,
                        uint64_t required_project_id = 0) noexcept {
  return scene_registry().resolve_or_legacy(
      handle, ObjectKind::composition, output, required_project_id);
}

bool resolve_scene_project(void* handle, ObjectSnapshot& output) noexcept {
  return scene_registry().resolve(
      handle, ObjectKind::project, output);
}

bool resolve_scene_layer(void* handle, ObjectSnapshot& output,
                         uint64_t required_project_id = 0) noexcept {
  if (handle == &g_layer)
    handle = &g_aegp_layers[0];
  return scene_registry().resolve_or_legacy(
      handle, ObjectKind::layer, output, required_project_id);
}

void* borrow_scene_object(Identity identity) noexcept {
  return scene_registry().borrow(identity);
}

bool scene_handle_is_composition(void* handle) noexcept {
  if (!handle) return false;
  if (handle == aexcompat::scene_runtime::composition_handle()) return true;
  ObjectSnapshot resolved{};
  // Resolving as a composition is not enough: the registry carries other
  // comps, including one in a second project, and a plug-in can walk to any of
  // them (project -> item -> AEGP_GetCompFromItem) and hold a borrowed handle
  // to it. What this answers is "does this handle name *this worker's* comp",
  // so the resolved object has to be the one whose legacy handle is that comp.
  // Without the second half, a foreign comp's handle passed the caller checks
  // that use this - the working colour space would have been rewritten
  // through a handle belonging to another project (issue #894).
  return resolve_scene_comp(handle, resolved) &&
      resolved.legacy_handle == aexcompat::scene_runtime::composition_handle();
}

std::u16string scene_name(const ObjectSnapshot& snapshot) {
  const auto end = std::find(
      snapshot.name.begin(), snapshot.name.end(), char16_t{});
  return {snapshot.name.begin(), end};
}

int32_t primary_layer_index(const ObjectSnapshot& snapshot) noexcept {
  if (snapshot.identity.kind != ObjectKind::layer ||
      snapshot.identity.project_id != 1 ||
      snapshot.owner.object_id != 5001 || !snapshot.legacy_handle)
    return -1;
  for (std::size_t index = 0; index < g_aegp_layers.size(); ++index)
    if (snapshot.legacy_handle == &g_aegp_layers[index])
      return static_cast<int32_t>(index);
  return -1;
}

bool& g_aegp_effect_live = scene_runtime_state().effect_live;
std::array<AegpEffectInstance, kAegpEffectInstanceCapacity>& g_aegp_effect_instances =
    scene_runtime_state().effect_instances;
std::array<AegpEffectLease, kAegpEffectLeaseCapacity>& g_aegp_effect_leases =
    scene_runtime_state().effect_leases;
uint32_t& g_aegp_effect_lease_generation = scene_runtime_state().effect_lease_generation;

enum class AbiPossessionPolicy {
  explicit_plugin_id,
  possessed_borrowed_handle,
};

bool resolve_with_possession_policy(
    void* handle, ObjectKind kind, AbiPossessionPolicy policy,
    int32_t plugin_id, ObjectSnapshot& output) {
  if (policy == AbiPossessionPolicy::explicit_plugin_id)
    return plugin_id > 0 && scene_registry().resolve_possessed(
        handle, kind, plugin_id, output);
  int32_t possession_id = 0;
  return scene_registry().resolve(handle, kind, output) &&
      scene_registry().possession(handle, kind, possession_id) &&
      possession_id > 0;
}

bool ensure_effect_identity(std::size_t instance_index) {
  if (instance_index >= g_aegp_effect_instances.size()) return false;
  auto& instance = g_aegp_effect_instances[instance_index];
  if (!instance.occupied) return false;
  ObjectSnapshot existing{};
  if (instance.identity != Identity{} &&
      scene_registry().snapshot(instance.identity, existing))
    return true;
  ObjectSnapshot layer{};
  if (!resolve_scene_layer(instance.layer, layer)) return false;
  return scene_registry().create_child(
      ObjectKind::effect, layer.identity, static_cast<int32_t>(instance_index),
      &instance,
      u"Effect", instance.identity);
}

const AegpEffectInstance* resolve_effect_instance(void* effect, int32_t owner,
                                                  std::size_t* index) {
  // PF-interface callers historically receive this stable host-owned reference.
  if (effect == &g_aegp_effect && g_aegp_effect_live) {
    if (index) *index = 0;
    return &g_aegp_effect_instances[0];
  }
  ObjectSnapshot resolved{};
  const auto policy = owner > 0
      ? AbiPossessionPolicy::explicit_plugin_id
      : AbiPossessionPolicy::possessed_borrowed_handle;
  if (!resolve_with_possession_policy(
          effect, ObjectKind::effect, policy, owner, resolved))
    return nullptr;
  if (resolved.local_index < 0 ||
      static_cast<std::size_t>(resolved.local_index) >=
          g_aegp_effect_instances.size())
    return nullptr;
  const std::size_t instance_index =
      static_cast<std::size_t>(resolved.local_index);
  const auto& instance = g_aegp_effect_instances[instance_index];
  if (!instance.occupied || instance.identity != resolved.identity)
    return nullptr;
  if (index) *index = instance_index;
  return &instance;
}
bool acquire_effect_lease(int32_t plugin_id, std::size_t instance_index, void** output) {
  if (plugin_id < 0 || !output || instance_index >= g_aegp_effect_instances.size() ||
      !g_aegp_effect_instances[instance_index].occupied ||
      !ensure_effect_identity(instance_index) ||
      g_aegp_effect_lease_generation == UINT32_MAX)
    return false;
  for (std::size_t slot = 0; slot < g_aegp_effect_leases.size(); ++slot) {
    auto& lease = g_aegp_effect_leases[slot];
    if (lease.live) continue;
    void* handle = scene_registry().borrow_unique(
        g_aegp_effect_instances[instance_index].identity, plugin_id);
    if (!handle) return false;
    const uint32_t generation = ++g_aegp_effect_lease_generation;
    lease = {plugin_id, static_cast<uint32_t>(instance_index),
             g_aegp_effect_instances[instance_index].generation, generation,
             true, handle};
    *output = handle;
    ++g_aegp_effect_acquires;
    return true;
  }
  return false;
}
const AegpEffectLease* resolve_effect_lease(void* effect, std::size_t* slot_out = nullptr) {
  for (std::size_t slot = 0; slot < g_aegp_effect_leases.size(); ++slot) {
    const auto& lease = g_aegp_effect_leases[slot];
    ObjectSnapshot resolved{};
    if (lease.live && lease.handle == effect &&
        scene_registry().resolve_possessed(
            effect, ObjectKind::effect, lease.owner_plugin_id, resolved)) {
      if (slot_out) *slot_out = slot;
      return &lease;
    }
  }
  return nullptr;
}
bool any_effect_lease_live() {
  return std::any_of(g_aegp_effect_leases.begin(), g_aegp_effect_leases.end(),
                     [](const auto& lease) { return lease.live; });
}
bool layer_effect_boundary_is_live(const AegpLayerRenderOptionsValue& options) {
  return options.effect_boundary == AegpLayerEffectBoundary::all ||
      resolve_effect_instance(options.upstream_effect, options.owner_plugin_id) != nullptr;
}
uint64_t effect_instance_identity(std::size_t index,
                                  const AegpEffectInstance& instance) {
  if (!instance.occupied || instance.generation == 0 ||
      index >= kAegpEffectInstanceCapacity)
    return 0;
  return (static_cast<uint64_t>(instance.generation) << 32) |
      static_cast<uint64_t>(index + 1);
}
bool snapshot_staged_item_metadata(
    void* item, AegpStagedItemMetadata& metadata) noexcept {
  metadata = {};
  auto& runtime = scene_runtime_state();
  ObjectSnapshot item_snapshot{}, comp_snapshot{};
  if (!resolve_scene_item(item, item_snapshot) ||
      !scene_registry().project_identity(
          item_snapshot.identity.project_id, metadata.project) ||
      !scene_registry().comp_from_item(
          item_snapshot.identity, comp_snapshot))
    return false;
  metadata.identity = item_snapshot.identity;
  int32_t item_id = 0;
  if (item == composition_item_handle()) {
    item_id = static_cast<int32_t>(runtime.composition_item_identity);
  } else if (aegp_get_item_id(item, &item_id) != 0) {
    return false;
  }
  if (item_id <= 0)
    return false;
  metadata.stable_identity = static_cast<uint64_t>(item_id);
  metadata.sampling_policy = runtime.composition_item_sampling_policy;
  if (runtime.composition_item_dependency_count >
      metadata.direct_dependencies.size())
    return false;
  metadata.direct_dependency_count =
      runtime.composition_item_dependency_count;
  std::copy_n(runtime.composition_item_dependencies.begin(),
              metadata.direct_dependency_count,
              metadata.direct_dependencies.begin());
  for (std::size_t index = 0;
       index < metadata.direct_dependency_count; ++index) {
    ObjectSnapshot dependency{};
    if (!resolve_scene_item(
            metadata.direct_dependencies[index], dependency))
      return false;
    metadata.dependency_identities[index] = dependency.identity;
  }
  struct OrderedEffectIndex {
    std::size_t instance_index{};
    int32_t layer_index{};
    int32_t stack_order{};
  };
  std::array<OrderedEffectIndex, kAegpEffectInstanceCapacity>
      ordered_effects{};
  std::size_t ordered_count = 0;
  for (std::size_t index = 0; index < runtime.effect_instances.size(); ++index) {
    const auto& instance = runtime.effect_instances[index];
    const uint64_t identity = effect_instance_identity(index, instance);
    ObjectSnapshot effect{}, layer{};
    if (identity == 0 ||
        !scene_registry().snapshot(instance.identity, effect) ||
        !scene_registry().snapshot(effect.owner, layer) ||
        layer.owner != comp_snapshot.identity)
      continue;
    ordered_effects[ordered_count++] = {
        index, layer.local_index, instance.stack_order};
  }
  for (std::size_t left = 0; left < ordered_count; ++left) {
    if (ordered_effects[left].layer_index < 0 ||
        ordered_effects[left].stack_order < 0)
      return false;
    for (std::size_t right = left + 1; right < ordered_count; ++right) {
      if (ordered_effects[left].layer_index ==
              ordered_effects[right].layer_index &&
          ordered_effects[left].stack_order ==
              ordered_effects[right].stack_order)
        return false;
    }
  }
  std::sort(ordered_effects.begin(),
            ordered_effects.begin() + ordered_count,
            [](const auto& left, const auto& right) {
              return left.layer_index != right.layer_index
                  ? left.layer_index < right.layer_index
                  : left.stack_order < right.stack_order;
            });
  for (std::size_t order = 0; order < ordered_count; ++order) {
    const auto index = ordered_effects[order].instance_index;
    metadata.effect_instances[order] =
        effect_instance_identity(index, runtime.effect_instances[index]);
    metadata.effect_identities[order] =
        runtime.effect_instances[index].identity;
    metadata.effect_orders[order] = static_cast<uint32_t>(order);
  }
  metadata.effect_instance_count = ordered_count;
  return true;
}
uint64_t staged_effect_instance_identity(
    const AegpLayerRenderOptionsValue& options,
    uint64_t active_effect_instance) noexcept {
  std::size_t index = 0;
  const AegpEffectInstance* instance = nullptr;
  if (options.effect_boundary == AegpLayerEffectBoundary::all) {
    auto& runtime = scene_runtime_state();
    for (; index < runtime.effect_instances.size(); ++index) {
      const auto& candidate = runtime.effect_instances[index];
      if (effect_instance_identity(index, candidate) == active_effect_instance) {
        instance = &candidate;
        break;
      }
    }
    if (!instance || instance->layer != options.layer) return 0;
  } else {
    instance = resolve_effect_instance(
        options.upstream_effect, options.owner_plugin_id, &index);
  }
  return instance ? effect_instance_identity(index, *instance) : 0;
}
uint64_t staged_effect_identity_for_render_ref(
    void* render_ref) noexcept {
  if (!render_ref) return 0;
  auto& runtime = scene_runtime_state();
  for (std::size_t index = 0; index < runtime.effect_instances.size(); ++index) {
    const auto& instance = runtime.effect_instances[index];
    if (instance.render_ref == render_ref)
      return effect_instance_identity(index, instance);
  }
  return 0;
}
const AegpInstalledEffectRecord* find_installed_effect(int32_t key);
const AegpEffectParameterRecord* find_effect_parameter(
    const AegpEffectInstance& instance, int32_t index);
void initialize_effect_parameter_values(AegpEffectInstance& instance);
AegpTransformStream& g_aegp_transform_stream = scene_runtime_state().transform_stream;
std::array<AegpLegacyEffectStream, kAegpLegacyEffectStreamCapacity>&
    g_aegp_legacy_effect_streams = scene_runtime_state().legacy_effect_streams;
uint32_t& g_aegp_legacy_effect_stream_generation =
    scene_runtime_state().legacy_effect_stream_generation;

int32_t __cdecl aegp_get_active_item(void** item) {
  if (!item) return 4;
  if (!(g_aegp_update_menu_mode || g_aegp_command_roundtrip_mode ||
        g_aegp_comp_idle_roundtrip_mode)) {
    *item = nullptr;
    return 0;
  }
  void* borrowed = borrow_scene_object(scene_registry().active_item());
  if (!borrowed) return 4;
  *item = borrowed;
  return 0;
}
int32_t __cdecl aegp_get_num_projects(int32_t* count) {
  if (!count || scene_registry().project_count() >
                    static_cast<std::size_t>(INT32_MAX))
    return 4;
  *count = static_cast<int32_t>(scene_registry().project_count());
  return 0;
}
int32_t __cdecl aegp_get_project_by_index(int32_t index, void** project) {
  ObjectSnapshot resolved{};
  if (!project || index < 0 ||
      !scene_registry().project_by_index(
          static_cast<std::size_t>(index), resolved))
    return 4;
  void* borrowed = borrow_scene_object(resolved.identity);
  if (!borrowed) return 4;
  *project = borrowed;
  return 0;
}
int32_t __cdecl aegp_get_project_root_folder(void* project, void** folder) {
  ObjectSnapshot resolved_project{};
  ObjectSnapshot resolved_folder{};
  if (!folder || !resolve_scene_project(project, resolved_project) ||
      !scene_registry().first_child(
          resolved_project.identity, resolved_folder) ||
      resolved_folder.identity.kind != ObjectKind::folder)
    return 4;
  void* borrowed = borrow_scene_object(resolved_folder.identity);
  if (!borrowed) return 4;
  *folder = borrowed;
  return 0;
}
int32_t __cdecl aegp_get_first_project_item(void* project, void** item) {
  ObjectSnapshot resolved_project{};
  ObjectSnapshot resolved_item{};
  if (!item || !resolve_scene_project(project, resolved_project))
    return 4;
  *item = nullptr;
  if (!scene_registry().first_project_item(
          resolved_project.identity, resolved_item))
    return 0;
  void* borrowed = borrow_scene_object(resolved_item.identity);
  if (!borrowed) return 4;
  *item = borrowed;
  return 0;
}
int32_t __cdecl aegp_get_next_project_item(
    void* project, void* item, void** next_item) {
  ObjectSnapshot resolved_project{};
  ObjectSnapshot resolved_item{};
  ObjectSnapshot resolved_next{};
  if (!next_item || !resolve_scene_project(project, resolved_project) ||
      !resolve_scene_item(
          item, resolved_item, resolved_project.identity.project_id))
    return 4;
  *next_item = nullptr;
  if (!scene_registry().next_project_item(
          resolved_project.identity, resolved_item.identity, resolved_next))
    return 0;
  void* borrowed = borrow_scene_object(resolved_next.identity);
  if (!borrowed) return 4;
  *next_item = borrowed;
  return 0;
}
int32_t __cdecl aegp_get_item_type(void* item, int16_t* item_type);
AegpItemSuite g_aegp_item_suite{};

int32_t __cdecl aegp_get_item_type(void* item, int16_t* item_type) {
  ObjectSnapshot resolved{};
  if (!item_type || !resolve_scene_item(item, resolved)) return 4;
  int16_t result = 0;
  switch (resolved.item_kind) {
    case ItemKind::folder: result = 1; break;
    case ItemKind::composition: result = 2; break;
    case ItemKind::footage: result = 4; break;
    default: return 4;
  }
  ++g_aegp_item_type_calls;
  *item_type = result;
  return 0;
}
AegpLegacyItemSuite6 g_aegp_legacy_item_suite6{};

std::array<AegpTime, 3>& g_aegp_layer_in_points = state().layer_in_points;
std::array<AegpTime, 3>& g_aegp_layer_durations = state().layer_durations;
std::array<AegpLayerTransform, 3>& g_aegp_layer_transforms = state().layer_transforms;
std::array<int32_t, 3>& g_aegp_layer_parent_indices = state().layer_parent_indices;

int32_t __cdecl aegp_get_item_current_time(void* item, AegpTime* time) {
  ObjectSnapshot resolved{};
  if (!time || !resolve_scene_item(item, resolved) ||
      resolved.item_kind != ItemKind::composition ||
      resolved.legacy_handle != &g_aegp_comp_item)
    return 4;
  ++g_aegp_item_current_time_calls;
  if (g_aegp_first_observed_frame < 0) g_aegp_first_observed_frame = g_aegp_scene_frame;
  g_aegp_last_observed_frame = g_aegp_scene_frame;
  *time = {g_aegp_scene_frame, 30};
  return 0;
}
int32_t __cdecl aegp_set_item_current_time(void* item, const AegpTime* time) {
  ObjectSnapshot resolved{};
  if (!time || !resolve_scene_item(item, resolved) ||
      resolved.item_kind != ItemKind::composition ||
      resolved.legacy_handle != &g_aegp_comp_item || time->scale == 0 ||
      time->value < 0 || time->value > 300)
    return 4;
  ++g_aegp_item_set_current_time_calls;
  g_aegp_item_last_set_time_value = time->value;
  g_aegp_item_last_set_time_scale = time->scale;
  g_aegp_scene_frame = static_cast<int32_t>(
      (static_cast<int64_t>(time->value) * 30) / time->scale);
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_get_item_id(void* item, int32_t* id) {
  ObjectSnapshot resolved{};
  if (!id || !resolve_scene_item(item, resolved) ||
      resolved.identity.object_id > INT32_MAX)
    return 4;
  *id = resolved.legacy_handle == &g_aegp_comp_item
      ? static_cast<int32_t>(scene_runtime_state().composition_item_identity)
      : static_cast<int32_t>(resolved.identity.object_id);
  return 0;
}
int32_t __cdecl aegp_get_item_name(int32_t plugin_id, void* item, void** name) {
  ObjectSnapshot resolved{};
  if (plugin_id != 1 || !name || !resolve_scene_item(item, resolved))
    return 4;
  const std::u16string value = scene_name(resolved);
  if (value.empty() ||
      make_utf16_handle(value, "item name", name) != 0)
    return 4;
  ++g_aegp_item_name_calls;
  return 0;
}
int32_t __cdecl aegp_get_item_duration(void* item, AegpTime* duration) {
  ObjectSnapshot resolved{};
  if (!duration || !resolve_scene_item(item, resolved) ||
      resolved.item_kind != ItemKind::composition)
    return 4;
  *duration = {300, 30};
  ++g_aegp_item_duration_calls;
  return 0;
}
int32_t __cdecl aegp_get_comp_from_item(void* item, void** comp) {
  ObjectSnapshot resolved_item{};
  ObjectSnapshot resolved_comp{};
  if (!comp || !resolve_scene_item(item, resolved_item) ||
      !scene_registry().comp_from_item(
          resolved_item.identity, resolved_comp))
    return 4;
  void* borrowed = borrow_scene_object(resolved_comp.identity);
  if (!borrowed) return 4;
  ++g_aegp_comp_from_item_calls;
  *comp = borrowed;
  return 0;
}
int32_t __cdecl aegp_get_comp_framerate(void* comp, double* fps) {
  ObjectSnapshot resolved{};
  if (!fps || !resolve_scene_comp(comp, resolved)) return 4;
  ++g_aegp_comp_framerate_calls;
  *fps = 30.0;
  return 0;
}
int32_t __cdecl aegp_get_comp_frame_duration(void* comp, AegpTime* duration) {
  ObjectSnapshot resolved{};
  if (!duration || !resolve_scene_comp(comp, resolved)) return 4;
  *duration = {1, 30};
  return 0;
}
int32_t __cdecl aegp_get_comp_num_layers(void* comp, int32_t* count) {
  ObjectSnapshot resolved{};
  if (!count || !resolve_scene_comp(comp, resolved)) return 4;
  const std::size_t result = scene_registry().layer_count(resolved.identity);
  if (result > static_cast<std::size_t>(INT32_MAX)) return 4;
  ++g_aegp_layer_count_calls;
  *count = static_cast<int32_t>(result);
  return 0;
}
int32_t __cdecl aegp_get_comp_layer_by_index(void* comp, int32_t index, void** layer) {
  ObjectSnapshot resolved_comp{};
  ObjectSnapshot resolved_layer{};
  if (!layer || index < 0 || !resolve_scene_comp(comp, resolved_comp) ||
      !scene_registry().layer_by_index(
          resolved_comp.identity, static_cast<std::size_t>(index),
          resolved_layer))
    return 4;
  void* borrowed = borrow_scene_object(resolved_layer.identity);
  if (!borrowed) return 4;
  ++g_aegp_layer_by_index_calls;
  *layer = borrowed;
  return 0;
}
int32_t aegp_layer_index(void* layer) {
  if (layer == &g_layer) return 0;
  ObjectSnapshot resolved{};
  if (!resolve_scene_layer(layer, resolved)) return -1;
  const int32_t primary = primary_layer_index(resolved);
  if (primary >= 0) return primary;
  return state().dynamic_camera_live &&
      resolved.identity == state().dynamic_camera_identity
      ? resolved.local_index : -1;
}
// The same index, refused when it is past the per-layer attribute tables.
//
// Those hold three entries, one per fixture layer, while `aegp_layer_index`
// also answers for a camera the plug-in created at runtime - whose local index
// is the comp's layer count, so 3. Every attribute below reads or writes one
// of those tables at the index, so a camera handle reached the fourth element:
// reads off the end for the getters, and writes off the end for
// `AEGP_SetLayerFlag` and `AEGP_SetLayerInPointAndDuration`. Predates this
// stack; the transform path avoided it by bounds-checking separately
// (`build_layer_world_transform`).
int32_t aegp_layer_attribute_index(void* layer) {
  // One bound for all three tables, which is only right while they are the
  // same length.
  static_assert(std::tuple_size_v<std::remove_reference_t<
                    decltype(g_aegp_layer_durations)>> ==
                std::tuple_size_v<std::remove_reference_t<
                    decltype(g_aegp_layer_in_points)>>);
  static_assert(std::tuple_size_v<std::remove_reference_t<
                    decltype(g_aegp_layer_flags)>> ==
                std::tuple_size_v<std::remove_reference_t<
                    decltype(g_aegp_layer_in_points)>>);
  const int32_t index = aegp_layer_index(layer);
  // A negative index converts to a value past any size, so this rejects it.
  return static_cast<std::size_t>(index) < g_aegp_layer_in_points.size()
      ? index : -1;
}
int32_t __cdecl aegp_get_layer_to_world_xform(
    void* layer, const AegpTime* comp_time, AegpMatrix4* transform) {
  const int32_t index = aegp_layer_index(layer);
  if (index < 0 || !comp_time || !transform || !valid_comp_time(*comp_time)) return 4;
  AegpMatrix4 result{};
  if (!build_layer_world_transform(static_cast<std::size_t>(index), *comp_time, result)) return 4;
  *transform = result;
  return 0;
}
int32_t __cdecl aegp_get_item_from_comp(void* comp, void** item) {
  ObjectSnapshot resolved_comp{};
  ObjectSnapshot resolved_item{};
  if (!item || !resolve_scene_comp(comp, resolved_comp) ||
      !scene_registry().item_from_comp(
          resolved_comp.identity, resolved_item))
    return 4;
  void* borrowed = borrow_scene_object(resolved_item.identity);
  if (!borrowed) return 4;
  *item = borrowed;
  return 0;
}
int32_t __cdecl aegp_get_item_dimensions(
    void* item, int32_t* width, int32_t* height) {
  ObjectSnapshot resolved{};
  if (!width || !height || !resolve_scene_item(item, resolved) ||
      resolved.item_kind != ItemKind::composition)
    return 4;
  const int32_t result_width = g_full_resolution_width > 0
      ? g_full_resolution_width : g_smart_width;
  const int32_t result_height = g_full_resolution_height > 0
      ? g_full_resolution_height : g_smart_height;
  if (result_width <= 0 || result_height <= 0 ||
      result_width > 32768 || result_height > 32768) return 4;
  *width = result_width;
  *height = result_height;
  return 0;
}

// AEGP_GetLayerMaskedBounds: the layer's extent after its masks, in layer
// coordinates.
//
// This host's scene model carries no per-layer masks. Masks reach an effect as
// its own parameters and through the render request's mask trailer, which is
// effect state, not something hanging off the layer this suite hands out. With
// no mask on the layer the masked bounds are the layer's whole extent, which is
// what this returns: origin at (0,0), the same dimensions
// AEGP_GetItemDimensions reports for the composition.
//
// A layer whose extent differs from the composition, and the intersection AE
// computes when a layer does carry masks (including how feather widens it), are
// not derivable from what this host models. If either becomes observable, the
// answer here has to come from an AE measurement rather than from an
// approximation invented here (issue #891).
int32_t __cdecl aegp_get_layer_masked_bounds(
    void* layer, int32_t time_mode, const AegpTime* time,
    AegpFloatRect* bounds) {
  // AEGP_LTimeMode_LayerTime = 0, AEGP_LTimeMode_CompTime = 1.
  constexpr int32_t kLayerTimeMode = 0;
  constexpr int32_t kCompTimeMode = 1;
  const int32_t index = aegp_layer_index(layer);
  if (index < 0 || !time || !bounds ||
      (time_mode != kLayerTimeMode && time_mode != kCompTimeMode))
    return 4;
  // Only the time base is checked, not the range: the bounds this host returns
  // do not vary over time, and the layer-time mode measures from the layer's
  // own start, so the composition-duration check the comp-time callbacks use
  // would refuse valid layer times.
  if (time->scale == 0) return 4;
  const int32_t width = g_full_resolution_width > 0
      ? g_full_resolution_width : g_smart_width;
  const int32_t height = g_full_resolution_height > 0
      ? g_full_resolution_height : g_smart_height;
  if (width <= 0 || height <= 0 || width > 32768 || height > 32768) return 4;
  *bounds = AegpFloatRect{0.0, 0.0, static_cast<double>(width),
                          static_cast<double>(height)};
  return 0;
}

// Adds two rational times, keeping the result exact. Same denominator adds the
// numerators; different denominators go through their lcm. Anything that would
// leave the A_Time range is refused rather than rounded, so a caller never
// receives a time this host silently changed.
bool add_rational_times(const AegpTime& left, const AegpTime& right,
                        AegpTime& sum) {
  if (left.scale == 0 || right.scale == 0) return false;
  if (left.scale == right.scale) {
    const int64_t total = static_cast<int64_t>(left.value) + right.value;
    if (total < (std::numeric_limits<int32_t>::min)() ||
        total > (std::numeric_limits<int32_t>::max)())
      return false;
    sum = AegpTime{static_cast<int32_t>(total), left.scale};
    return true;
  }
  const uint64_t divisor = std::gcd<uint64_t, uint64_t>(left.scale, right.scale);
  const uint64_t common = static_cast<uint64_t>(left.scale) / divisor * right.scale;
  if (common > (std::numeric_limits<uint32_t>::max)()) return false;
  const int64_t total =
      static_cast<int64_t>(left.value) * static_cast<int64_t>(common / left.scale) +
      static_cast<int64_t>(right.value) * static_cast<int64_t>(common / right.scale);
  if (total < (std::numeric_limits<int32_t>::min)() ||
      total > (std::numeric_limits<int32_t>::max)())
    return false;
  sum = AegpTime{static_cast<int32_t>(total), static_cast<uint32_t>(common)};
  return true;
}

// AEGP_ConvertLayerToCompTime / AEGP_ConvertCompToLayerTime.
//
// A layer's time origin is its in point, so the two directions are an add and
// a subtract of it. AE also scales by the layer's stretch; this host's scene
// model has no stretch (nothing sets one and AEGP_SetLayerStretch is not
// implemented), so the conversion is the unstretched one. A layer that did
// carry a stretch would need the factor here, which is why this is written as
// the identity-stretch case rather than as the general one (issue #891).
int32_t __cdecl aegp_convert_layer_to_comp_time(
    void* layer, const AegpTime* layer_time, AegpTime* comp_time) {
  const int32_t index = aegp_layer_attribute_index(layer);
  if (index < 0 || !layer_time || !comp_time || layer_time->scale == 0) return 4;
  AegpTime converted{};
  if (!add_rational_times(*layer_time,
                          g_aegp_layer_in_points[static_cast<std::size_t>(index)],
                          converted))
    return 4;
  *comp_time = converted;
  ++g_aegp_layer_attribute_calls;
  return 0;
}

int32_t __cdecl aegp_convert_comp_to_layer_time(
    void* layer, const AegpTime* comp_time, AegpTime* layer_time) {
  const int32_t index = aegp_layer_attribute_index(layer);
  if (index < 0 || !comp_time || !layer_time || comp_time->scale == 0) return 4;
  const AegpTime& in_point = g_aegp_layer_in_points[static_cast<std::size_t>(index)];
  if (in_point.value == (std::numeric_limits<int32_t>::min)()) return 4;
  AegpTime converted{};
  if (!add_rational_times(*comp_time, AegpTime{-in_point.value, in_point.scale},
                          converted))
    return 4;
  *layer_time = converted;
  ++g_aegp_layer_attribute_calls;
  return 0;
}

int32_t __cdecl aegp_get_layer_stream_value_v2(void* layer, int32_t which_stream,
    int16_t time_mode, const AegpTime* time, uint8_t,
    AegpLegacyStreamVal* value, int32_t* stream_type) {
  constexpr int32_t kLayerStreamZoom = 11;
  constexpr int16_t kCompTimeMode = 1;
  constexpr int32_t kStreamTypeOneD = 5;
  const int32_t index = aegp_layer_index(layer);
  if (index < 0 || index != g_aegp_active_camera_layer_index ||
      which_stream != kLayerStreamZoom || time_mode != kCompTimeMode ||
      !time || !value || !valid_comp_time(*time) ||
      !layer_active_at_time(static_cast<std::size_t>(index), *time)) return 4;
  const int32_t width = g_full_resolution_width > 0
      ? g_full_resolution_width : g_smart_width;
  if (width <= 0 || width > INT16_MAX) return 4;
  double zoom = 0.0;
  if (!resolve_layer_camera_zoom(static_cast<std::size_t>(index), *time,
          static_cast<double>(width), zoom)) return 4;
  value->one_d = zoom;
  if (stream_type) *stream_type = kStreamTypeOneD;
  return 0;
}
int32_t __cdecl aegp_get_active_layer(void** layer) {
  if (!layer) return 4;
  // AE returns a non-null active layer only when exactly one layer is selected.
  *layer = nullptr;
  return 0;
}
int32_t __cdecl aegp_get_layer_index(void* layer, int32_t* index) {
  ObjectSnapshot resolved{};
  if (!index || !resolve_scene_layer(layer, resolved) ||
      resolved.local_index < 0)
    return 4;
  *index = resolved.local_index;
  return 0;
}
int32_t __cdecl aegp_get_layer_source_item(void* layer, void** item) {
  ObjectSnapshot resolved{};
  if (!item || !resolve_scene_layer(layer, resolved)) return 4;
  if (resolved.related_item.kind == ObjectKind::none) {
    *item = nullptr;
    ++g_aegp_layer_source_item_calls;
    return 0;
  }
  void* borrowed = borrow_scene_object(resolved.related_item);
  if (!borrowed) return 4;
  *item = borrowed;
  ++g_aegp_layer_source_item_calls;
  return 0;
}
int32_t __cdecl aegp_get_layer_parent_comp(void* layer, void** comp) {
  ObjectSnapshot resolved{};
  if (!comp || !resolve_scene_layer(layer, resolved)) return 4;
  void* borrowed = borrow_scene_object(resolved.owner);
  if (!borrowed) return 4;
  *comp = borrowed;
  return 0;
}
int32_t __cdecl aegp_get_layer_name(
    int32_t plugin_id, void* layer, void** layer_name, void** source_name) {
  ObjectSnapshot resolved{};
  if (plugin_id != 1 || !layer_name || !source_name ||
      !resolve_scene_layer(layer, resolved))
    return 4;
  *layer_name = nullptr;
  *source_name = nullptr;
  const std::u16string layer_value = scene_name(resolved);
  if (layer_value.empty() ||
      make_utf16_handle(layer_value, "layer name", layer_name) != 0)
    return 4;
  std::u16string source_value = u"Source";
  ObjectSnapshot source{};
  if (resolved.related_item.kind != ObjectKind::none &&
      scene_registry().snapshot(resolved.related_item, source))
    source_value = scene_name(source);
  if (source_value.empty() ||
      make_utf16_handle(source_value, "source name", source_name) != 0) {
    free_aegp_mem_handle(*layer_name);
    *layer_name = nullptr;
    return 4;
  }
  ++g_aegp_layer_name_calls;
  return 0;
}
int32_t __cdecl aegp_get_layer_parent(void* layer, void** parent) {
  ObjectSnapshot resolved{};
  if (!parent || !resolve_scene_layer(layer, resolved)) return 4;
  const int32_t index = primary_layer_index(resolved);
  Identity parent_identity = resolved.parent_layer;
  if (index >= 0) {
    const int32_t parent_index =
        g_aegp_layer_parent_indices[static_cast<std::size_t>(index)];
    if (parent_index == -1) {
      *parent = nullptr;
      return 0;
    }
    if (parent_index < 0 || static_cast<std::size_t>(parent_index) >=
        g_aegp_layers.size())
      return 4;
    ObjectSnapshot parent_snapshot{};
    if (!resolve_scene_layer(
            &g_aegp_layers[static_cast<std::size_t>(parent_index)],
            parent_snapshot, resolved.identity.project_id))
      return 4;
    parent_identity = parent_snapshot.identity;
  }
  if (parent_identity.kind == ObjectKind::none) {
    *parent = nullptr;
    return 0;
  }
  if (parent_identity.project_id != resolved.identity.project_id)
    return 4;
  void* borrowed = borrow_scene_object(parent_identity);
  if (!borrowed) return 4;
  *parent = borrowed;
  return 0;
}
int32_t __cdecl aegp_set_layer_parent(void* layer, void* parent) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  ObjectSnapshot resolved_layer{};
  ObjectSnapshot resolved_parent{};
  if (!resolve_scene_layer(layer, resolved_layer) ||
      (parent &&
       !resolve_scene_layer(
           parent, resolved_parent, resolved_layer.identity.project_id)))
    return 4;
  const int32_t layer_index = primary_layer_index(resolved_layer);
  const int32_t parent_index = parent
      ? primary_layer_index(resolved_parent) : -1;
  if (layer_index < 0 || (parent && parent_index < 0))
    return 4;
  auto candidate = g_aegp_layer_parent_indices;
  candidate[static_cast<std::size_t>(layer_index)] = parent_index;
  int32_t cursor = parent_index;
  for (std::size_t depth = 0; cursor >= 0 &&
       depth <= candidate.size(); ++depth) {
    if (cursor == layer_index) return 4;
    if (static_cast<std::size_t>(cursor) >= candidate.size()) return 4;
    cursor = candidate[static_cast<std::size_t>(cursor)];
  }
  if (cursor >= 0) return 4;
  auto& registry = scene_registry();
  aexcompat::scene_model::Registry::MutationCheckpoint checkpoint{};
  if (!registry.capture_mutation_checkpoint(checkpoint)) return 4;
  const auto parents_before = g_aegp_layer_parent_indices;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      registry, resolved_layer.identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        if (!registry.set_parent_layer(
                resolved_layer.identity,
                parent ? resolved_parent.identity : Identity{}))
          return false;
        g_aegp_layer_parent_indices = candidate;
        return true;
      },
      [&]() noexcept {
        g_aegp_layer_parent_indices = parents_before;
        return registry.restore_mutation_checkpoint(checkpoint);
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl aegp_create_camera_in_comp(
    const char16_t* name, AegpFloatPoint center, void* comp_handle,
    void** camera) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  ObjectSnapshot resolved_comp{};
  if (!name || !camera || !std::isfinite(center.x) ||
      !std::isfinite(center.y) ||
      !resolve_scene_comp(comp_handle, resolved_comp) ||
      state().dynamic_camera_live ||
      resolved_comp.identity.project_id != 1 ||
      !scene_registry().can_create_children(resolved_comp.identity, 1, 1))
    return 4;
  std::size_t name_length = 0;
  while (name_length < 47 && name[name_length] != 0) ++name_length;
  if (name_length == 0 || name_length >= 47) return 4;
  const std::u16string_view camera_name{name, name_length};
  Identity camera_identity{};
  void* published = nullptr;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), resolved_comp.identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  if (!transaction.commit(
          [&]() noexcept {
            if (!scene_registry().create_child(
                    ObjectKind::layer, resolved_comp.identity,
                    static_cast<int32_t>(
                        scene_registry().layer_count(resolved_comp.identity)),
                    &state().dynamic_camera, camera_name,
                    camera_identity))
              return false;
            published = borrow_scene_object(camera_identity);
            if (!published) {
              scene_registry().erase_tree(camera_identity);
              return false;
            }
            state().dynamic_camera_live = true;
            state().dynamic_camera_identity = camera_identity;
            state().dynamic_camera_zoom = 800.0;
            return true;
          },
          []() noexcept { bump_render_project_timestamp(); }))
    return 4;
  *camera = published;
  return 0;
}
int32_t __cdecl aegp_delete_layer(void* layer) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  ObjectSnapshot resolved{};
  if (!resolve_scene_layer(layer, resolved) ||
      !state().dynamic_camera_live ||
      resolved.identity != state().dynamic_camera_identity ||
      (g_aegp_transform_stream.live &&
       g_aegp_transform_stream.layer == layer))
    return 4;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), resolved.identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        if (!scene_registry().erase_tree(resolved.identity)) return false;
        state().dynamic_camera_live = false;
        state().dynamic_camera_identity = {};
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl aegp_get_layer_from_id(void* comp, int32_t id, void** layer) {
  ObjectSnapshot resolved_comp{};
  ObjectSnapshot resolved_layer{};
  if (!layer || id <= 0 || !resolve_scene_comp(comp, resolved_comp) ||
      !scene_registry().layer_from_id(
          resolved_comp.identity, static_cast<uint64_t>(id), resolved_layer))
    return 4;
  void* borrowed = borrow_scene_object(resolved_layer.identity);
  if (!borrowed) return 4;
  *layer = borrowed;
  return 0;
}
int32_t __cdecl aegp_get_comp_selection(
    int32_t plugin_id, void* comp, void** collection) {
  ObjectSnapshot resolved{};
  if (plugin_id < 0 || !collection || g_aegp_selection.live ||
      !resolve_scene_comp(comp, resolved) ||
      resolved.legacy_handle != &g_aegp_comp)
    return 4;
  g_aegp_selection.live = true;
  ++g_aegp_collection_creates;
  *collection = &g_aegp_selection;
  return 0;
}
int32_t __cdecl aegp_dispose_collection(void* collection) {
  if (collection != &g_aegp_selection || !g_aegp_selection.live) return 4;
  g_aegp_selection.live = false;
  ++g_aegp_collection_disposes;
  return 0;
}
int32_t __cdecl aegp_get_collection_count(void* collection, uint32_t* count) {
  if (collection != &g_aegp_selection || !g_aegp_selection.live || !count) return 4;
  *count = 2;
  return 0;
}
int32_t __cdecl aegp_get_collection_item(
    void* collection, uint32_t index, AegpCollectionItem* item) {
  if (collection != &g_aegp_selection || !g_aegp_selection.live || index >= 2 || !item)
    return 4;
  ObjectSnapshot layer_snapshot{};
  Identity primary_comp{};
  if (!scene_registry().identity_for_legacy(
          &g_aegp_comp, ObjectKind::composition, primary_comp) ||
      !scene_registry().layer_by_index(primary_comp, index, layer_snapshot))
    return 4;
  void* layer = borrow_scene_object(layer_snapshot.identity);
  if (!layer) return 4;
  *item = {};
  item->type = 1;
  std::memcpy(item->item.data(), &layer, sizeof(layer));
  ++g_aegp_collection_item_reads;
  return 0;
}
AegpCollectionSuite g_aegp_collection_suite{};
int32_t __cdecl aegp_get_layer_id(void* layer, int32_t* id) {
  ObjectSnapshot resolved{};
  if (!id || !resolve_scene_layer(layer, resolved) ||
      resolved.identity.object_id > INT32_MAX)
    return 4;
  ++g_aegp_layer_id_calls;
  *id = static_cast<int32_t>(resolved.identity.object_id);
  return 0;
}
int32_t __cdecl aegp_get_layer_flags(void* layer, uint32_t* flags) {
  const int32_t index = aegp_layer_attribute_index(layer);
  if (index < 0 || !flags) return 4;
  *flags = g_aegp_layer_flags[static_cast<std::size_t>(index)];
  ++g_aegp_layer_attribute_calls;
  return 0;
}
int32_t __cdecl aegp_set_layer_flag(void* layer, uint32_t flag, uint8_t value) {
  const int32_t index = aegp_layer_attribute_index(layer);
  constexpr uint32_t kWritableFlags = 0x00000001u | 0x00000002u |
      0x00000020u | 0x00004000u;
  if (index < 0 || (flag & kWritableFlags) == 0 || (flag & (flag - 1)) != 0 || value > 1)
    return 4;
  auto& flags = g_aegp_layer_flags[static_cast<std::size_t>(index)];
  if (value) flags |= flag;
  else flags &= ~flag;
  ++g_aegp_layer_flag_set_calls;
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_get_layer_transfer_mode(
    void* layer, AegpLayerTransferMode* transfer) {
  if (aegp_layer_index(layer) < 0 || !transfer) return 4;
  *transfer = {0, 0, 0};
  ++g_aegp_layer_attribute_calls;
  return 0;
}
int32_t __cdecl aegp_get_layer_object_type(void* layer, int32_t* type) {
  ObjectSnapshot resolved{};
  if (!type || !resolve_scene_layer(layer, resolved)) return 4;
  *type = state().dynamic_camera_live &&
      resolved.identity == state().dynamic_camera_identity ? 2 : 0;
  ++g_aegp_layer_attribute_calls;
  return 0;
}
int32_t __cdecl aegp_get_layer_in_point(void* layer, int32_t time_mode, AegpTime* time) {
  const int32_t index = aegp_layer_attribute_index(layer);
  if (index < 0 || time_mode != 1 || !time) return 4;
  *time = g_aegp_layer_in_points[static_cast<std::size_t>(index)];
  ++g_aegp_layer_attribute_calls;
  return 0;
}
int32_t __cdecl aegp_get_layer_duration(void* layer, int32_t time_mode, AegpTime* time) {
  const int32_t index = aegp_layer_attribute_index(layer);
  if (index < 0 || time_mode != 1 || !time) return 4;
  *time = g_aegp_layer_durations[static_cast<std::size_t>(index)];
  ++g_aegp_layer_attribute_calls;
  return 0;
}
int32_t __cdecl aegp_set_layer_in_point_and_duration(
    void* layer, int32_t time_mode, const AegpTime* in_point, const AegpTime* duration) {
  const int32_t index = aegp_layer_attribute_index(layer);
  if (index < 0 || time_mode != 1 || !in_point || !duration ||
      in_point->scale != 30 || duration->scale != 30 || in_point->value < 0 ||
      duration->value <= 0 || in_point->value + duration->value > 300) return 4;
  g_aegp_layer_in_points[static_cast<std::size_t>(index)] = *in_point;
  g_aegp_layer_durations[static_cast<std::size_t>(index)] = *duration;
  ++g_aegp_layer_trim_set_calls;
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_get_layer_num_effects(void* layer, int32_t* count) {
  const int32_t index = aegp_layer_index(layer);
  if (index < 0 || !count) return 4;
  ++g_aegp_effect_count_calls;
  *count = static_cast<int32_t>(std::count_if(g_aegp_effect_instances.begin(),
      g_aegp_effect_instances.end(), [layer](const auto& instance) {
        return instance.occupied && instance.layer == layer;
      }));
  return 0;
}
int32_t __cdecl aegp_get_layer_effect_by_index(
    int32_t plugin_id, void* layer, int32_t index, void** effect) {
  if (plugin_id < 0 || aegp_layer_index(layer) < 0 || index < 0 || !effect) return 4;
  for (std::size_t slot = 0; slot < g_aegp_effect_instances.size(); ++slot) {
    const auto& instance = g_aegp_effect_instances[slot];
    if (!instance.occupied || instance.layer != layer) continue;
    if (instance.stack_order == index)
      return acquire_effect_lease(plugin_id, slot, effect) ? 0 : 4;
  }
  return 4;
}
int32_t __cdecl aegp_get_installed_key_from_layer_effect(void* effect, int32_t* key) {
  const auto* instance = resolve_effect_instance(effect);
  if (!instance || !key) return 4;
  ++g_aegp_effect_metadata_calls;
  *key = instance->installed_key;
  return 0;
}
int32_t __cdecl aegp_get_effect_flags(void* effect, uint32_t* flags) {
  const auto* instance = resolve_effect_instance(effect);
  if (!instance || !flags) return 4;
  ++g_aegp_effect_metadata_calls;
  *flags = instance->flags;
  return 0;
}
int32_t __cdecl aegp_set_effect_flags(void* effect, uint32_t set_mask, uint32_t flags) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  std::size_t instance_index = 0;
  if (!resolve_effect_instance(effect, 0, &instance_index) || (flags & ~set_mask) != 0)
    return 4;
  ObjectSnapshot identity{};
  if (!scene_registry().resolve(effect, ObjectKind::effect, identity)) return 4;
  auto candidate = g_aegp_effect_instances;
  candidate[instance_index].flags =
      (candidate[instance_index].flags & ~set_mask) | flags;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), identity.identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        g_aegp_effect_instances = candidate;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl aegp_reorder_effect(void* effect, int32_t target_order) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  std::size_t instance_index = 0;
  const auto* resolved = resolve_effect_instance(effect, 0, &instance_index);
  if (!resolved || target_order < 0) return 4;
  ObjectSnapshot identity{};
  if (!scene_registry().resolve(effect, ObjectKind::effect, identity)) return 4;
  auto candidate = g_aegp_effect_instances;
  auto& instance = candidate[instance_index];
  const int32_t count = static_cast<int32_t>(std::count_if(
      candidate.begin(), candidate.end(),
      [&](const auto& value) { return value.occupied && value.layer == instance.layer; }));
  if (target_order >= count) return 4;
  const int32_t old_order = instance.stack_order;
  if (target_order < old_order) {
    for (auto& value : candidate)
      if (value.occupied && value.layer == instance.layer &&
          value.stack_order >= target_order && value.stack_order < old_order)
        ++value.stack_order;
  } else if (target_order > old_order) {
    for (auto& value : candidate)
      if (value.occupied && value.layer == instance.layer &&
          value.stack_order > old_order && value.stack_order <= target_order)
        --value.stack_order;
  }
  instance.stack_order = target_order;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), identity.identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        g_aegp_effect_instances = candidate;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl aegp_dispose_effect(void* effect) {
  if (effect == &g_aegp_effect) {
    if (!g_aegp_effect_live) return 4;
    g_aegp_effect_live = false;
    // `loaded_plugin` is not cleared here. It describes the instance, and
    // disposing the handle changes nothing about the instance: `occupied` and
    // `generation` both stay, so the streams opened through that handle are
    // still live - `legacy_effect_stream_parent_live` asks only those two -
    // and they would go on being answered, out of the probe fixture's table
    // instead of the plug-in's, if the flag went away with the handle. The
    // paths that do reuse slot 0 (`aegp_apply_effect`,
    // `aegp_delete_layer_effect`) assign a whole instance and clear it that
    // way.
    ++g_aegp_effect_disposes;
    return 0;
  }
  std::size_t slot = 0;
  const auto* lease = resolve_effect_lease(effect, &slot);
  if (!lease || !scene_registry().release(
          effect, ObjectKind::effect, lease->owner_plugin_id, true))
    return 4;
  g_aegp_effect_leases[slot].live = false;
  g_aegp_effect_leases[slot].handle = nullptr;
  ++g_aegp_effect_disposes;
  return 0;
}
int32_t __cdecl aegp_apply_effect(
    int32_t plugin_id, void* layer, int32_t installed_key, void** effect) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  ObjectSnapshot layer_identity{};
  if (plugin_id < 0 || !resolve_scene_layer(layer, layer_identity) ||
      layer_identity.identity.project_id != 1 ||
      !effect || !find_installed_effect(installed_key) ||
      g_aegp_effect_lease_generation == UINT32_MAX)
    return 4;
  const auto instance_slot = std::find_if(g_aegp_effect_instances.begin(),
      g_aegp_effect_instances.end(), [](const auto& instance) { return !instance.occupied; });
  const auto lease_slot = std::find_if(g_aegp_effect_leases.begin(),
      g_aegp_effect_leases.end(), [](const auto& lease) { return !lease.live; });
  if (instance_slot == g_aegp_effect_instances.end() ||
      lease_slot == g_aegp_effect_leases.end()) return 4;
  const std::size_t instance_index = static_cast<std::size_t>(
      std::distance(g_aegp_effect_instances.begin(), instance_slot));
  const std::size_t lease_index = static_cast<std::size_t>(
      std::distance(g_aegp_effect_leases.begin(), lease_slot));
  const int32_t stack_order = static_cast<int32_t>(std::count_if(
      g_aegp_effect_instances.begin(), g_aegp_effect_instances.end(),
      [layer](const auto& instance) { return instance.occupied && instance.layer == layer; }));
  if (instance_slot->generation == UINT32_MAX) return 4;
  const uint32_t instance_generation = instance_slot->generation + 1;
  auto candidate_instances = g_aegp_effect_instances;
  auto& candidate = candidate_instances[instance_index];
  candidate = {layer, installed_key, stack_order, 1,
               instance_generation, true};
  initialize_effect_parameter_values(candidate);
  auto candidate_leases = g_aegp_effect_leases;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), layer_identity.identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() ||
      !transaction.validate(scene_registry().can_create_child(
          layer_identity.identity)))
    return 4;
  void* published = nullptr;
  const bool committed = transaction.commit(
      [&]() noexcept {
        Identity created{};
        if (!scene_registry().create_child_borrowed(
                ObjectKind::effect, layer_identity.identity,
                static_cast<int32_t>(instance_index),
                &g_aegp_effect_instances[instance_index],
                u"Effect", plugin_id, created, published))
          return false;
        candidate.identity = created;
        candidate.render_ref = published;
        const uint32_t lease_generation =
            g_aegp_effect_lease_generation + 1;
        candidate_leases[lease_index] = {
            plugin_id, static_cast<uint32_t>(instance_index),
            instance_generation, lease_generation, true, published};
        g_aegp_effect_instances = candidate_instances;
        g_aegp_effect_leases = candidate_leases;
        g_aegp_effect_lease_generation = lease_generation;
        ++g_aegp_effect_acquires;
        *effect = published;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); });
  return committed ? 0 : 4;
}
int32_t __cdecl aegp_delete_layer_effect(void* effect) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  std::size_t instance_index = 0;
  if (!resolve_effect_instance(effect, 0, &instance_index)) return 4;
  ObjectSnapshot identity{};
  if (!scene_registry().resolve(effect, ObjectKind::effect, identity)) return 4;
  auto candidate_instances = g_aegp_effect_instances;
  auto candidate_leases = g_aegp_effect_leases;
  auto candidate_transform_stream = g_aegp_transform_stream;
  auto candidate_legacy_streams = g_aegp_legacy_effect_streams;
  uint32_t invalidated_streams = 0;
  uint32_t invalidated_values = 0;
  uint32_t invalidated_effect_leases = 0;
  auto& instance = candidate_instances[instance_index];
  void* layer = instance.layer;
  const int32_t deleted_order = instance.stack_order;
  if (instance.generation == UINT32_MAX) return 4;
  const uint32_t generation = instance.generation + 1;
  instance = {};
  instance.generation = generation;
  for (auto& value : candidate_instances)
    if (value.occupied && value.layer == layer && value.stack_order > deleted_order)
      --value.stack_order;
  for (auto& lease : candidate_leases)
    if (lease.live && lease.instance_index == instance_index) {
      ++invalidated_effect_leases;
      lease.live = false;
      lease.handle = nullptr;
    }
  if (candidate_transform_stream.live &&
      candidate_transform_stream.effect_param &&
      candidate_transform_stream.effect_instance_index == instance_index) {
    ++invalidated_streams;
    if (candidate_transform_stream.value_live) ++invalidated_values;
    candidate_transform_stream = {};
  }
  for (auto& stream : candidate_legacy_streams)
    if (stream.live && stream.effect_instance_index == instance_index) {
      ++invalidated_streams;
      if (stream.value_live) ++invalidated_values;
      stream = {};
    }
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), identity.identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        if (!scene_registry().erase_tree(identity.identity)) return false;
        g_aegp_effect_instances = candidate_instances;
        g_aegp_effect_leases = candidate_leases;
        g_aegp_transform_stream = candidate_transform_stream;
        g_aegp_legacy_effect_streams = candidate_legacy_streams;
        g_aegp_effect_disposes += invalidated_effect_leases;
        g_aegp_stream_disposes += invalidated_streams;
        g_aegp_stream_value_disposes += invalidated_values;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl aegp_duplicate_effect(void* original, void** duplicate) {
  if (!duplicate) return 4;
  aexcompat::scene_transaction::MutationLock mutation_lock;
  std::size_t original_index = 0;
  const auto* source = resolve_effect_instance(original, 0, &original_index);
  const auto* source_lease = resolve_effect_lease(original);
  if (!source || !source_lease) return 4;
  ObjectSnapshot source_identity{};
  if (!scene_registry().resolve(
          original, ObjectKind::effect, source_identity) ||
      g_aegp_effect_lease_generation == UINT32_MAX)
    return 4;
  const auto instance_slot = std::find_if(g_aegp_effect_instances.begin(),
      g_aegp_effect_instances.end(), [](const auto& value) { return !value.occupied; });
  const auto lease_slot = std::find_if(g_aegp_effect_leases.begin(),
      g_aegp_effect_leases.end(), [](const auto& value) { return !value.live; });
  if (instance_slot == g_aegp_effect_instances.end() ||
      lease_slot == g_aegp_effect_leases.end()) return 4;
  const void* layer = source->layer;
  const int32_t inserted_order = source->stack_order + 1;
  const int32_t installed_key = source->installed_key;
  const uint32_t flags = source->flags;
  auto candidate_instances = g_aegp_effect_instances;
  auto candidate_leases = g_aegp_effect_leases;
  for (auto& value : candidate_instances)
    if (value.occupied && value.layer == layer && value.stack_order >= inserted_order)
      ++value.stack_order;
  const std::size_t instance_index = static_cast<std::size_t>(
      std::distance(g_aegp_effect_instances.begin(), instance_slot));
  if (instance_slot->generation == UINT32_MAX) return 4;
  const uint32_t generation = instance_slot->generation + 1;
  auto& candidate = candidate_instances[instance_index];
  candidate = {const_cast<void*>(layer), installed_key, inserted_order,
               flags, generation, true};
  candidate.parameter_values = source->parameter_values;
  // A duplicate is the same effect, so it answers out of the same parameter
  // table as its source. Left at the aggregate's default it would have said
  // "fixture" for a copy of the loaded plug-in's own instance.
  candidate.loaded_plugin = source->loaded_plugin;
  const std::size_t lease_index = static_cast<std::size_t>(
      std::distance(g_aegp_effect_leases.begin(), lease_slot));
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), source_identity.identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() ||
      !transaction.validate(scene_registry().can_create_child(
          source_identity.owner)))
    return 4;
  void* published = nullptr;
  const bool committed = transaction.commit(
      [&]() noexcept {
        Identity created{};
        if (!scene_registry().create_child_borrowed(
                ObjectKind::effect, source_identity.owner,
                static_cast<int32_t>(instance_index),
                &g_aegp_effect_instances[instance_index],
                u"Effect", source_lease->owner_plugin_id,
                created, published))
          return false;
        candidate.identity = created;
        candidate.render_ref = published;
        const uint32_t lease_generation =
            g_aegp_effect_lease_generation + 1;
        candidate_leases[lease_index] = {
            source_lease->owner_plugin_id,
            static_cast<uint32_t>(instance_index), generation,
            lease_generation, true, published};
        g_aegp_effect_instances = candidate_instances;
        g_aegp_effect_leases = candidate_leases;
        g_aegp_effect_lease_generation = lease_generation;
        ++g_aegp_effect_acquires;
        *duplicate = published;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); });
  return committed ? 0 : 4;
}
int32_t __cdecl get_new_effect_for_effect(int32_t plugin_id, void* effect, void** effect_ref) {
  // A negative id is malformed; 0 is an unregistered caller, which is admitted
  // for the same reason the colour settings suite admits it (issue #894): the
  // id names the caller for AE's own accounting, and this host's ownership of
  // the effect handle is tracked by `g_aegp_effect_live` rather than by the id.
  // DeepGlow2 never calls AEGP_RegisterWithAEGP, so it reaches every AEGP entry
  // point with 0, and refusing here ended its SmartRender before the matte path
  // could even ask its question (issue #909).
  if (plugin_id < 0 || effect != &g_effect || !effect_ref || g_aegp_effect_live)
    return 4;
  // Give the handle a scene identity before publishing it.
  //
  // This used to hand back `&g_aegp_effect`, a bare host object the scene
  // registry knows nothing about. Every AEGP call that takes the handle and
  // then checks possession - AEGP_GetNewEffectStreamByIndex among them -
  // refused it, so a plug-in could acquire an effect handle and do nothing
  // with it (issue #909). Registering instance 0 and borrowing from the
  // registry makes the handle resolvable the same way every other scene
  // object is.
  auto& instance = g_aegp_effect_instances[0];
  if (!instance.occupied) {
    instance = AegpEffectInstance{};
    instance.occupied = true;
    instance.layer = &g_aegp_layers[0];
    ++instance.generation;
  }
  // Whatever slot 0 held, from here it stands for the plug-in that just asked
  // for its own effect handle. The scene runtime seeds it with the probe
  // fixture's key at start, and this is the only place that fact stops being
  // true: without saying so, every stream question about this handle is
  // answered out of a five-parameter fixture table (issue #909).
  instance.loaded_plugin = true;
  if (!ensure_effect_identity(0)) return 4;
  g_aegp_effect_live = true;
  // The handle stays `&g_aegp_effect`: `resolve_effect_instance` recognizes
  // it directly, and the callers that take it back are matched to that.
  // What changed is that instance 0 is now occupied and carries a registry
  // identity, so a possession check on this handle has something to find.
  *effect_ref = &g_aegp_effect;
  ++g_aegp_effect_acquires;
  return 0;
}
const AegpInstalledEffectRecord* find_installed_effect(int32_t key) {
  for (const auto& effect : kAegpInstalledEffects)
    if (effect.key == key) return &effect;
  return nullptr;
}
// The parameters of the plug-in this worker actually loaded.
//
// The installed-effect table below it is a compile-time list of the fixtures
// this host was built against, which no real AEX appears in. A plug-in that
// walks its own streams - DeepGlow2 asks what is plugged into its matte layer
// parameter during SmartRender - was refused because its parameters were not
// in that list (issue #909). PARAMS_SETUP already told the worker what they
// are, so that is what answers here.
//
// Indices are 1-based, matching `records[index - 1]` everywhere else in the
// worker; index 0 is the input layer and has no record.
// PF parameter types and AEGP stream types are different enumerations, and
// the records the worker keeps carry the PF one. The fixture table below was
// written with AEGP values, so a loaded plug-in's parameter has to be
// translated before it reaches `stream_value_kind` - handing the PF value
// straight through made every stream read as StreamValueKind::none and the
// stream refuse to open (issue #909).
//
// PF_Param values are from AE_Effect.h; AEGP_StreamType from AE_GeneralPlug.h,
// whose enumerators are unnumbered so the value is the ordinal: NO_DATA 0,
// ThreeD_SPATIAL 1, ThreeD 2, TwoD_SPATIAL 3, TwoD 4, OneD 5, COLOR 6, ARB 7,
// MARKER 8, LAYER_ID 9, MASK_ID 10, MASK 11, TEXT_DOCUMENT 12.
//
// A PF type this host models no stream for answers NO_DATA, which
// `stream_value_kind` already answers as none. That covers more than the group
// markers: PF_Param_PATH has no MASK-stream support here, and a group start,
// group end, or button has no stream at all in this model, so an index landing
// on one refuses to open. AE returns a stream reference for the group markers,
// so a plug-in enumerating every parameter stops early here (issue #919).
constexpr int32_t aegp_stream_type_for_param_type(int32_t param_type) noexcept {
  constexpr int32_t kAegpStreamNoData = 0;
  constexpr int32_t kAegpStreamThreeD = 2;
  constexpr int32_t kAegpStreamTwoD = 4;
  constexpr int32_t kAegpStreamOneD = 5;
  constexpr int32_t kAegpStreamColor = 6;
  constexpr int32_t kAegpStreamArb = 7;
  constexpr int32_t kAegpStreamLayerId = 9;
  switch (param_type) {
    case 0: return kAegpStreamLayerId;   // PF_Param_LAYER
    case 1:                              // PF_Param_SLIDER (obsolete)
    case 2:                              // PF_Param_FIX_SLIDER (obsolete)
    case 3:                              // PF_Param_ANGLE
    case 4:                              // PF_Param_CHECKBOX
    case 7:                              // PF_Param_POPUP
    case 10: return kAegpStreamOneD;     // PF_Param_FLOAT_SLIDER
    case 5: return kAegpStreamColor;     // PF_Param_COLOR
    case 6: return kAegpStreamTwoD;      // PF_Param_POINT
    case 11: return kAegpStreamArb;      // PF_Param_ARBITRARY_DATA
    case 18: return kAegpStreamThreeD;   // PF_Param_POINT_3D
    default: return kAegpStreamNoData;   // groups, buttons, path, NO_DATA
  }
}
// The whole table, checked at compile time. A pure function of an int is worth
// no less coverage than a runtime self-test would give it, and this one cannot
// be skipped or left unwired: ARB shipped as 10 (which is MASK_ID) because
// nothing compared the mapping against the SDK enumeration.
static_assert(aegp_stream_type_for_param_type(-1) == 0);   // PF_Param_RESERVED
static_assert(aegp_stream_type_for_param_type(0) == 9);    // LAYER -> LAYER_ID
static_assert(aegp_stream_type_for_param_type(1) == 5);    // SLIDER -> OneD
static_assert(aegp_stream_type_for_param_type(2) == 5);    // FIX_SLIDER
static_assert(aegp_stream_type_for_param_type(3) == 5);    // ANGLE
static_assert(aegp_stream_type_for_param_type(4) == 5);    // CHECKBOX
static_assert(aegp_stream_type_for_param_type(5) == 6);    // COLOR
static_assert(aegp_stream_type_for_param_type(6) == 4);    // POINT -> TwoD
static_assert(aegp_stream_type_for_param_type(7) == 5);    // POPUP
static_assert(aegp_stream_type_for_param_type(8) == 0);    // CUSTOM (obsolete)
static_assert(aegp_stream_type_for_param_type(9) == 0);    // NO_DATA
static_assert(aegp_stream_type_for_param_type(10) == 5);   // FLOAT_SLIDER
static_assert(aegp_stream_type_for_param_type(11) == 7);   // ARBITRARY_DATA
static_assert(aegp_stream_type_for_param_type(12) == 0);   // PATH (no MASK yet)
static_assert(aegp_stream_type_for_param_type(13) == 0);   // GROUP_START
static_assert(aegp_stream_type_for_param_type(14) == 0);   // GROUP_END
static_assert(aegp_stream_type_for_param_type(15) == 0);   // BUTTON
static_assert(aegp_stream_type_for_param_type(16) == 0);   // RESERVED2
static_assert(aegp_stream_type_for_param_type(17) == 0);   // RESERVED3
static_assert(aegp_stream_type_for_param_type(18) == 2);   // POINT_3D -> ThreeD

const AegpEffectParameterRecord* loaded_effect_parameter(int32_t index) {
  const auto& records = aexcompat::worker_runtime::parameters::state().records;
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:loaded_param index=" << index
              << " records=" << records.size() << "\n" << std::flush;
  if (index < 0 || static_cast<std::size_t>(index) > records.size())
    return nullptr;
  // Published through a static so the `const char*` outlives the call. One
  // live answer at a time is enough: every caller reads what it asked for
  // before asking again, and the storage is thread-local so two threads
  // asking at once do not overwrite each other.
  static thread_local std::string published_name;
  static thread_local AegpEffectParameterRecord published{};
  // Index 0 is the input layer. It carries no PARAMS_SETUP record - the
  // records are 1-based, which is why every other index reads `index - 1` -
  // but AE counts it and a plug-in asking what is plugged into its input is
  // an ordinary question. The fixture tables answer it the same way, with a
  // LAYER_ID stream whose value `aegp_get_new_stream_value_v2` fills from the
  // instance's layer.
  if (index == 0) {
    published_name = "Input";
    published.name = published_name.c_str();
    published.type = aegp_stream_type_for_param_type(0);  // PF_Param_LAYER
    published.default_value = {};
    published.writable = false;
    return &published;
  }
  const auto& record = records[static_cast<std::size_t>(index - 1)];
  published_name = record.name;
  published.name = published_name.c_str();
  published.type = aegp_stream_type_for_param_type(record.type);
  published.default_value = {record.default_value, 0.0, 0.0, 0.0};
  // Writes go through the parameter runtime's own path, not this one.
  published.writable = false;
  return &published;
}
const AegpEffectParameterRecord* find_effect_parameter(
    const AegpEffectInstance& instance, int32_t index) {
  const int32_t key = instance.installed_key;
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:find_param key=" << key << " index=" << index
              << " loaded=" << instance.loaded_plugin << "\n" << std::flush;
  // Whose parameters these are is a property of the instance, not of the key.
  // Slot 0 is seeded with the probe's key at scene start and
  // `AEGP_GetNewEffectForEffect` hands that same slot to the loaded plug-in,
  // so a key check alone answers a 159-parameter effect out of a
  // five-parameter fixture table (issue #909).
  if (instance.loaded_plugin) return loaded_effect_parameter(index);
  const auto* effect = find_installed_effect(key);
  // A key the compile-time table does not carry is not a fixture either.
  if (!effect) return loaded_effect_parameter(index);
  // A fixture answers within its own declared parameter count and no further.
  // `aegp_get_effect_num_param_streams_v2` reports that count, and the arrays
  // below are sized to it, so letting an index past it fall through would both
  // contradict the reported count and index those arrays out of range.
  if (index < 0 || index >= effect->parameter_count) return nullptr;
  if (key == kAegpInstalledEffects[0].key)
    return &kAegpProbeParameters[static_cast<std::size_t>(index)];
  if (key == kAegpInstalledEffects[1].key || key == kAegpInstalledEffects[2].key)
    return &kAegpLevelsParameters[static_cast<std::size_t>(index)];
  return loaded_effect_parameter(index);
}
void initialize_effect_parameter_values(AegpEffectInstance& instance) {
  instance.parameter_values = {};
  const auto* effect = find_installed_effect(instance.installed_key);
  if (!effect) return;
  for (int32_t index = 1; index < effect->parameter_count; ++index) {
    const auto* parameter = find_effect_parameter(instance, index);
    if (parameter)
      instance.parameter_values[static_cast<std::size_t>(index - 1)] =
          parameter->default_value;
  }
}
int32_t __cdecl aegp_get_num_installed_effects(int32_t* count) {
  if (!count) return 4;
  *count = static_cast<int32_t>(kAegpInstalledEffects.size());
  ++g_aegp_effect_metadata_calls;
  return 0;
}
int32_t __cdecl aegp_get_next_installed_effect(int32_t key, int32_t* next_key) {
  if (!next_key) return 4;
  if (key == kAegpInstalledEffectKeyNone) {
    *next_key = kAegpInstalledEffects[0].key;
  } else {
    const auto found = std::find_if(kAegpInstalledEffects.begin(),
        kAegpInstalledEffects.end(), [key](const auto& effect) { return effect.key == key; });
    if (found == kAegpInstalledEffects.end()) return 4;
    const auto next = std::next(found);
    *next_key = next == kAegpInstalledEffects.end()
        ? kAegpInstalledEffectKeyNone : next->key;
  }
  ++g_aegp_effect_metadata_calls;
  return 0;
}
int32_t __cdecl aegp_get_effect_name(int32_t key, char* name) {
  const auto* effect = find_installed_effect(key);
  if (!effect || !name) return 4;
  ++g_aegp_effect_metadata_calls;
  std::memcpy(name, effect->name, std::strlen(effect->name) + 1);
  return 0;
}
int32_t __cdecl aegp_get_effect_match_name(int32_t key, char* name) {
  const auto* effect = find_installed_effect(key);
  if (!effect || !name) return 4;
  ++g_aegp_effect_metadata_calls;
  std::memcpy(name, effect->match_name, std::strlen(effect->match_name) + 1);
  return 0;
}
int32_t __cdecl aegp_get_effect_category(int32_t key, char* category) {
  const auto* effect = find_installed_effect(key);
  if (!effect || !category) return 4;
  const std::size_t length = std::strlen(effect->category);
  if (length + 1 > kAegpMaxEffectCategoryNameSize) return 4;
  std::memcpy(category, effect->category, length + 1);
  ++g_aegp_effect_metadata_calls;
  return 0;
}
int32_t __cdecl aegp_get_effect_num_param_streams_v2(void* effect, int32_t* count) {
  const auto* instance = resolve_effect_instance(effect);
  if (!instance || !count) return 4;
  const auto* installed = instance->loaded_plugin
      ? nullptr : find_installed_effect(instance->installed_key);
  if (installed) {
    *count = installed->parameter_count;
    return 0;
  }
  // A real loaded plug-in, whose parameters are the ones
  // `find_effect_parameter` answers from. Refusing here left the ordinary
  // walk - ask how many, then open each - stopping at its first call even
  // though every stream it would have asked for is now openable (issue #909).
  const auto& records = aexcompat::worker_runtime::parameters::state().records;
  // The count is `records.size() + 1` because AE counts the input layer, which
  // has no record and sits at index 0. `loaded_effect_parameter` answers
  // 1..records.size() and refuses 0, so a walk over `0..count-1` is one index
  // wider than what opens; #919 covers closing that gap along with the group
  // markers that have no stream either.
  *count = static_cast<int32_t>(records.size()) + 1;
  return 0;
}
bool supported_transform_stream(int32_t selector) {
  return selector == 0 || selector == 1 || selector == 2 || selector == 3 ||
      selector == 4 || selector == 8 || selector == 9;
}

aexcompat::scene_model::StreamValueKind stream_value_kind(
    int32_t type) noexcept {
  using aexcompat::scene_model::StreamValueKind;
  switch (type) {
    case 2:
    case 3:
    case 4:
    case 5: return StreamValueKind::scalar;
    case 6: return StreamValueKind::color;
    // ARB is 7, not 10 - 10 is MASK_ID, which this host does not model. The
    // table read 10 for as long as no caller reached it: every fixture uses
    // 2/4/5/6/9, so the mismatch only surfaced once a loaded plug-in's
    // arbitrary-data parameter could be translated into a stream type.
    case 7: return StreamValueKind::arbitrary;
    case 9: return StreamValueKind::layer;
    default: return StreamValueKind::none;
  }
}

bool effect_stream_parent_live();

bool publish_transform_stream(
    AegpTransformStream candidate, Identity owner, int32_t plugin_id,
    int32_t stream_type, bool keyframed, void** output) {
  const std::size_t child_count = keyframed ? 3 : 1;
  if (!output || plugin_id < 0 ||
      !scene_registry().can_create_children(owner, child_count, 1))
    return false;
  void* handle = nullptr;
  if (!scene_registry().create_child_borrowed(
          ObjectKind::stream, owner, candidate.selector,
          &g_aegp_transform_stream, u"Stream", plugin_id,
          candidate.identity, handle))
    return false;
  aexcompat::scene_model::StreamState stream_state{};
  stream_state.value_kind = stream_value_kind(stream_type);
  stream_state.dimensions =
      stream_type == 6 ? 4 : (stream_type == 2 ? 3 :
      (stream_type == 3 || stream_type == 4 ? 2 : 1));
  stream_state.temporal_dimensions = 1;
  if (stream_state.value_kind ==
          aexcompat::scene_model::StreamValueKind::none ||
      !scene_registry().initialize_stream_state(
          candidate.identity, stream_state)) {
    scene_registry().erase_tree(candidate.identity);
    return false;
  }
  candidate.handle = handle;
  if (keyframed) {
    for (std::size_t index = 0;
         index < candidate.keyframe_identities.size(); ++index) {
      if (!scene_registry().create_child(
              ObjectKind::keyframe, candidate.identity,
              static_cast<int32_t>(index), nullptr, u"Keyframe",
              candidate.keyframe_identities[index])) {
        scene_registry().erase_tree(candidate.identity);
        return false;
      }
      aexcompat::scene_model::KeyframeState key{};
      key.time_value = index == 0 ? 0 : 60;
      key.time_scale = 30;
      key.in_interpolation = index == 0 ? 1 : 3;
      key.out_interpolation = key.in_interpolation;
      key.temporal_in[0] = {0.0, 33.333333333333336};
      key.temporal_out[0] = {0.0, 33.333333333333336};
      if (!scene_registry().initialize_keyframe_state(
              candidate.keyframe_identities[index], key)) {
        scene_registry().erase_tree(candidate.identity);
        return false;
      }
    }
  }
  g_aegp_transform_stream = candidate;
  ++g_aegp_stream_acquires;
  *output = handle;
  return true;
}

bool resolve_transform_stream(void* handle, ObjectSnapshot& resolved,
                              int32_t plugin_id = 0) {
  const auto policy = plugin_id > 0
      ? AbiPossessionPolicy::explicit_plugin_id
      : AbiPossessionPolicy::possessed_borrowed_handle;
  const bool valid = resolve_with_possession_policy(
      handle, ObjectKind::stream, policy, plugin_id, resolved);
  return valid && g_aegp_transform_stream.live &&
      g_aegp_transform_stream.handle == handle &&
      g_aegp_transform_stream.identity == resolved.identity &&
      effect_stream_parent_live();
}

int32_t __cdecl aegp_get_new_layer_stream(
    int32_t plugin_id, void* layer, int32_t selector, void** stream) {
  ObjectSnapshot layer_identity{};
  const bool dynamic_camera =
      resolve_scene_layer(layer, layer_identity) &&
      state().dynamic_camera_live &&
      layer_identity.identity == state().dynamic_camera_identity;
  if (plugin_id < 0 || !resolve_scene_layer(layer, layer_identity) || !stream ||
      layer_identity.identity.project_id != 1 ||
      (!(dynamic_camera && selector == 11) &&
       !supported_transform_stream(selector)) ||
      g_aegp_transform_stream.live)
    return 4;
  AegpTransformStream candidate{};
  candidate.selector = selector;
  candidate.layer = layer;
  candidate.effect_param = false;
  candidate.live = true;
  candidate.value_live = false;
  candidate.owner_plugin_id = plugin_id;
  const int32_t type = selector == 11 ? 5 :
      (selector <= 2 ? 3 : 5);
  return publish_transform_stream(
      candidate, layer_identity.identity, plugin_id, type, false, stream)
      ? 0 : 4;
}
int32_t __cdecl aegp_get_new_effect_stream_by_index(
    int32_t plugin_id, void* effect, int32_t index, void** stream) {
  std::size_t instance_index = 0;
  const auto* instance = resolve_effect_instance(effect, plugin_id, &instance_index);
  if (plugin_id < 0 || !instance ||
      index < 0 || index > 4 || !stream || g_aegp_transform_stream.live) return 4;
  ObjectSnapshot effect_identity{};
  if (!scene_registry().resolve_possessed(
          effect, ObjectKind::effect, plugin_id, effect_identity))
    return 4;
  AegpTransformStream candidate{};
  candidate.selector = index;
  candidate.layer = instance->layer;
  candidate.effect_param = true;
  candidate.live = true;
  candidate.value_live = false;
  candidate.effect_instance_index = static_cast<uint32_t>(instance_index);
  candidate.effect_instance_generation = instance->generation;
  candidate.owner_plugin_id = plugin_id;
  const int32_t type = index == 0 ? 9 : (index == 1 ? 5 :
      (index == 2 ? 4 : (index == 3 ? 2 : 6)));
  const bool keyframed = index == 1 && instance_index == 0;
  return publish_transform_stream(
      candidate, effect_identity.identity, plugin_id, type, keyframed,
      stream) ? 0 : 4;
}
int32_t __cdecl aegp_get_effect_num_param_streams_v6(void* effect, int32_t* count) {
  const auto* instance = resolve_effect_instance(effect);
  if (!instance || instance->installed_key != kAegpInstalledEffects[0].key || !count) return 4;
  *count = kAegpInstalledEffects[0].parameter_count;
  return 0;
}
bool effect_stream_parent_live() {
  if (!g_aegp_transform_stream.effect_param) return true;
  const std::size_t index = g_aegp_transform_stream.effect_instance_index;
  if (index >= g_aegp_effect_instances.size()) return false;
  const auto& instance = g_aegp_effect_instances[index];
  return instance.occupied &&
      instance.generation == g_aegp_transform_stream.effect_instance_generation;
}
int32_t __cdecl aegp_get_stream_type(void* stream, int32_t* type) {
  if (aexcompat::l2_detail::find_stream(stream))
    return aexcompat::l2_detail::get_stream_type(stream, type);
  ObjectSnapshot resolved{};
  if (!type || !resolve_transform_stream(stream, resolved))
    return 4;
  if (g_aegp_transform_stream.effect_param) {
    switch (g_aegp_transform_stream.selector) {
      case 0: *type = 9; break;
      case 1: *type = 5; break;
      case 2: *type = 4; break;
      case 3: *type = 2; break;
      case 4: *type = 6; break;
      default: return 4;
    }
  } else {
    *type = g_aegp_transform_stream.selector == 11 ? 5 :
        (g_aegp_transform_stream.selector <= 2 ? 3 : 5);
  }
  return 0;
}
int32_t __cdecl aegp_get_stream_num_keyframes(void* stream, int32_t* count) {
  if (aexcompat::l2_detail::find_stream(stream))
    return aexcompat::l2_detail::get_stream_num_keyframes(stream, count);
  ObjectSnapshot resolved{};
  if (!count || !resolve_transform_stream(stream, resolved))
    return 4;
  ++g_aegp_keyframe_count_calls;
  if (g_aegp_transform_stream.effect_param && g_aegp_transform_stream.selector == 1 &&
      g_aegp_transform_stream.effect_instance_index == 0) {
    *count = 2;
    ++g_aegp_keyframed_stream_reports;
  } else {
    *count = 0;
  }
  return 0;
}
bool valid_amount_keyframe(void* stream, int32_t index) {
  ObjectSnapshot resolved{};
  return resolve_transform_stream(stream, resolved) &&
      g_aegp_transform_stream.effect_param && g_aegp_transform_stream.selector == 1 &&
      g_aegp_transform_stream.effect_instance_index == 0 &&
      index >= 0 && index < 2 &&
      g_aegp_transform_stream.keyframe_identities[
          static_cast<std::size_t>(index)] != Identity{};
}
int32_t __cdecl aegp_get_keyframe_time(
    void* stream, int32_t index, int32_t time_mode, AegpTime* time) {
  if (aexcompat::l2_detail::find_stream(stream))
    return aexcompat::l2_detail::get_keyframe_time(
        stream, index, static_cast<int16_t>(time_mode),
        reinterpret_cast<aexcompat::l2_detail::HostTime*>(time));
  if (!valid_amount_keyframe(stream, index) || time_mode != 1 || !time) return 4;
  ObjectSnapshot key{};
  if (!scene_registry().snapshot(
          g_aegp_transform_stream.keyframe_identities[
              static_cast<std::size_t>(index)],
          key))
    return 4;
  *time = {key.keyframe.time_value, key.keyframe.time_scale};
  ++g_aegp_keyframe_time_calls;
  return 0;
}
int32_t __cdecl aegp_get_new_keyframe_value(
    int32_t plugin_id, void* stream, int32_t index, AegpStreamValue* value) {
  if (aexcompat::l2_detail::find_stream(stream))
    return aexcompat::l2_detail::get_new_keyframe_value(
        plugin_id, stream, index,
        reinterpret_cast<aexcompat::l2_detail::StreamValue*>(value));
  if (plugin_id < 0 || !valid_amount_keyframe(stream, index) || !value ||
      g_aegp_transform_stream.value_live) return 4;
  ObjectSnapshot stream_identity{};
  if (!resolve_transform_stream(stream, stream_identity, plugin_id) ||
      !scene_registry().create_child(
          ObjectKind::value,
          g_aegp_transform_stream.keyframe_identities[
              static_cast<std::size_t>(index)],
          index, value,
          u"Keyframe Value", g_aegp_transform_stream.value_identity))
    return 4;
  *value = {};
  value->stream = stream;
  const double amount = index == 0 ? 10.0 : 90.0;
  std::memcpy(value->value.data(), &amount, sizeof(amount));
  g_aegp_transform_stream.value_live = true;
  ++g_aegp_stream_value_acquires;
  ++g_aegp_keyframe_value_calls;
  return 0;
}
int32_t __cdecl aegp_get_keyframe_interpolation(
    void* stream, int32_t index, int32_t* in_type, int32_t* out_type) {
  if (aexcompat::l2_detail::find_stream(stream))
    return aexcompat::l2_detail::get_keyframe_interpolation(
        stream, index, in_type, out_type);
  if (!valid_amount_keyframe(stream, index) || !in_type || !out_type) return 4;
  ObjectSnapshot key{};
  if (!scene_registry().snapshot(
          g_aegp_transform_stream.keyframe_identities[
              static_cast<std::size_t>(index)],
          key))
    return 4;
  *in_type = key.keyframe.in_interpolation;
  *out_type = key.keyframe.out_interpolation;
  ++g_aegp_keyframe_interpolation_calls;
  return 0;
}
int32_t __cdecl aegp_get_new_stream_value(
    int32_t plugin_id, void* stream, int32_t, const AegpTime* time,
    uint8_t, AegpStreamValue* value) {
  if (aexcompat::l2_detail::find_stream(stream))
    return aexcompat::l2_detail::get_new_stream_value(
        plugin_id, stream, 0,
        reinterpret_cast<const aexcompat::l2_detail::HostTime*>(time), 0,
        reinterpret_cast<aexcompat::l2_detail::StreamValue*>(value));
  ObjectSnapshot stream_identity{};
  if (plugin_id < 0 ||
      !resolve_transform_stream(stream, stream_identity, plugin_id) ||
      g_aegp_transform_stream.value_live || !time ||
      time->scale == 0 || !value) return 4;
  Identity value_identity{};
  if (g_aegp_transform_stream.effect_param &&
      (g_aegp_transform_stream.selector < 0 ||
       g_aegp_transform_stream.selector > 4))
    return 4;
  if (!scene_registry().create_child(
          ObjectKind::value, stream_identity.identity, 0, value,
          u"Stream Value", value_identity))
    return 4;
  value->stream = stream;
  value->value.fill(std::byte{});
  double components[2]{};
  if (g_aegp_transform_stream.effect_param) {
    const int32_t selector = g_aegp_transform_stream.selector;
    const auto& instance =
        g_aegp_effect_instances[g_aegp_transform_stream.effect_instance_index];
    if (selector == 0) {
      ObjectSnapshot input_layer{};
      int32_t layer_id = 0;
      if (!resolve_scene_layer(instance.layer, input_layer) ||
          input_layer.identity.object_id > INT32_MAX) {
        scene_registry().erase_tree(value_identity);
        return 4;
      }
      layer_id = static_cast<int32_t>(input_layer.identity.object_id);
      std::memcpy(
          value->value.data(), &layer_id, sizeof(layer_id));
    } else {
      std::memcpy(
          value->value.data(), instance.parameter_values[selector - 1].data(),
          sizeof(instance.parameter_values[selector - 1]));
    }
    ++g_aegp_effect_param_value_calls;
  } else switch (g_aegp_transform_stream.selector) {
    case 1: components[0] = 320.0; components[1] = 180.0; break;
    case 2: components[0] = 100.0; components[1] = 100.0; break;
    case 4: components[0] = 100.0; break;
    case 11:
      components[0] = state().dynamic_camera_zoom;
      break;
    default: break;
  }
  if (!g_aegp_transform_stream.effect_param)
    std::memcpy(value->value.data(), components, sizeof(components));
  g_aegp_transform_stream.value_live = true;
  g_aegp_transform_stream.value_identity = value_identity;
  if (!g_aegp_transform_stream.effect_param)
    g_aegp_stream_sampled_selector_mask |= 1u << g_aegp_transform_stream.selector;
  ++g_aegp_stream_value_acquires;
  return 0;
}
int32_t __cdecl aegp_get_stream_name(
    int32_t plugin_id, void* stream, uint8_t, void** name_handle) {
  ObjectSnapshot resolved{};
  if (plugin_id < 0 ||
      !resolve_transform_stream(stream, resolved, plugin_id) ||
      !g_aegp_transform_stream.effect_param || !name_handle)
    return 4;
  *name_handle = nullptr;
  std::u16string name;
  switch (g_aegp_transform_stream.selector) {
    case 0: name = u"Input"; break;
    case 1: name = u"Amount"; break;
    case 2: name = u"Center"; break;
    case 3: name = u"Vector"; break;
    case 4: name = u"Tint"; break;
    default: return 4;
  }
  if (make_utf16_handle(name, "effect parameter name", name_handle) != 0) return 4;
  ++g_aegp_effect_param_name_calls;
  return 0;
}
int32_t __cdecl aegp_set_effect_stream_value(
    int32_t plugin_id, void* stream, AegpStreamValue* value) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  ObjectSnapshot stream_identity{};
  // A write still names its caller. `resolve_transform_stream` drops to the
  // borrowed-handle policy for id 0, which asks only that *somebody* possesses
  // the handle - fine for the reads that admitting 0 was about (issue #909),
  // not for a mutation, where it would let a caller write through a stream
  // another id opened. Nothing observed needs an id-0 write.
  if (plugin_id <= 0 ||
      !resolve_transform_stream(stream, stream_identity, plugin_id) || !value ||
      value->stream != stream || !g_aegp_transform_stream.value_live ||
      !g_aegp_transform_stream.effect_param) return 4;
  const int32_t selector = g_aegp_transform_stream.selector;
  if (selector < 1 || selector > 4 ||
      (selector == 1 && g_aegp_transform_stream.effect_instance_index == 0)) return 4;
  std::array<double, 4> candidate{};
  std::memcpy(candidate.data(), value->value.data(), sizeof(candidate));
  for (int32_t index = 0; index < selector; ++index)
    if (!std::isfinite(candidate[static_cast<std::size_t>(index)])) return 4;
  auto candidate_instances = g_aegp_effect_instances;
  candidate_instances[g_aegp_transform_stream.effect_instance_index]
      .parameter_values[selector - 1] = candidate;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), stream_identity.identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        g_aegp_effect_instances = candidate_instances;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl aegp_dispose_stream_value(AegpStreamValue* value) {
  if (value && aexcompat::l2_detail::find_stream(value->stream))
    return aexcompat::l2_detail::dispose_stream_value(
        reinterpret_cast<aexcompat::l2_detail::StreamValue*>(value));
  ObjectSnapshot stream_identity{};
  ObjectSnapshot value_identity{};
  ObjectSnapshot value_owner{};
  Identity value_id{};
  if (!value ||
      !resolve_transform_stream(value->stream, stream_identity) ||
      !g_aegp_transform_stream.value_live ||
      !scene_registry().identity_for_legacy(
          value, ObjectKind::value, value_id) ||
      !scene_registry().snapshot(value_id, value_identity) ||
      value_identity.identity != g_aegp_transform_stream.value_identity ||
      (value_identity.owner != stream_identity.identity &&
       (!scene_registry().snapshot(value_identity.owner, value_owner) ||
        value_owner.identity.kind != ObjectKind::keyframe ||
        value_owner.owner != stream_identity.identity)) ||
      !scene_registry().erase_tree(value_identity.identity))
    return 4;
  value->stream = nullptr;
  g_aegp_transform_stream.value_live = false;
  g_aegp_transform_stream.value_identity = {};
  ++g_aegp_stream_value_disposes;
  return 0;
}
int32_t __cdecl aegp_dispose_stream(void* stream) {
  if (aexcompat::l2_detail::find_stream(stream))
    return aexcompat::l2_detail::dispose_stream(stream);
  ObjectSnapshot resolved{};
  if (!resolve_transform_stream(stream, resolved) ||
      g_aegp_transform_stream.value_live ||
      !scene_registry().erase_tree(resolved.identity))
    return 4;
  g_aegp_transform_stream = {};
  g_aegp_transform_stream.selector = -1;
  ++g_aegp_stream_disposes;
  return 0;
}

std::array<void*, 14> g_aegp_project_suite6{};
std::array<void*, 41> g_aegp_comp_suite10{};
std::array<void*, 28> g_aegp_comp_suite4{};
std::array<void*, 44> g_aegp_comp_suite11{};
std::array<void*, 44> g_aegp_comp_suite12{};
// `AEGP_LayerSuite1` (acquired as version 5, frozen in AE 5.0) is the oldest
// shape and needs its own table: two functions were inserted before version 11,
// so its slots do not line up with `g_aegp_layer_suite5` - which, confusingly,
// is `AEGP_LayerSuite5` and is acquired as version 11 (issue #712). Everything
// here is named after the struct rather than the acquire version to keep the
// two apart.
std::array<void*, 39> g_aegp_layer_suite1{};
std::array<void*, 46> g_aegp_layer_suite5{};
// `AEGP_LayerSuite7` (acquired as version 13, frozen in AE 10.0 build 396) is
// `AEGP_LayerSuite8` without its last two slots: version 14 appends
// AEGP_GetLayerSamplingQuality and AEGP_SetLayerSamplingQuality and changes
// nothing before them, so the two tables are filled the same way and only the
// length differs (issue #890).
std::array<void*, 48> g_aegp_layer_suite7{};
std::array<void*, 50> g_aegp_layer_suite8{};
std::array<void*, 53> g_aegp_layer_suite9{};
std::array<void*, 17> g_aegp_effect_suite2{};
std::array<void*, 17> g_aegp_effect_suite3{};
std::array<void*, 22> g_aegp_effect_suite4{};
std::array<void*, 22> g_aegp_stream_suite2{};
std::array<void*, 23> g_aegp_stream_suite6{};
std::array<void*, 22> g_aegp_keyframe_suite5{};
static_assert(sizeof(g_aegp_project_suite6) == 14 * sizeof(void*));
static_assert(sizeof(g_aegp_comp_suite10) == 41 * sizeof(void*));
static_assert(sizeof(g_aegp_comp_suite4) == 28 * sizeof(void*));
static_assert(sizeof(g_aegp_comp_suite11) == 352);
static_assert(sizeof(g_aegp_comp_suite12) == 352);
static_assert(sizeof(g_aegp_layer_suite5) == 368);
static_assert(sizeof(g_aegp_layer_suite7) == 384);
static_assert(sizeof(g_aegp_layer_suite8) == 400);
static_assert(sizeof(g_aegp_layer_suite9) == 424);
static_assert(sizeof(g_aegp_effect_suite4) == 176);
static_assert(sizeof(g_aegp_effect_suite2) == 136);
static_assert(sizeof(g_aegp_effect_suite3) == 136);
static_assert(sizeof(g_aegp_stream_suite2) == 176);
static_assert(sizeof(g_aegp_stream_suite6) == 184);
static_assert(sizeof(g_aegp_keyframe_suite5) == 176);

SceneSuiteAcquireResult scene_acquire_suite(
    const char* name, int32_t version, const void** suite) noexcept {
  if (!name || !suite || !scene_context()) return SceneSuiteAcquireResult::rejected;
  *suite = nullptr;
  const auto named = [name](const char* expected) {
    return std::strcmp(name, expected) == 0;
  };
  const auto& factory = scene_context()->hooks.suite_factory;

  if (named("AEGP Proj Suite") && version == 9 &&
      state().comp_idle_roundtrip_mode) {
    g_aegp_project_suite6 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_proj_9, 14>();
    g_aegp_project_suite6[0] =
        reinterpret_cast<void*>(&aegp_get_num_projects);
    g_aegp_project_suite6[1] =
        reinterpret_cast<void*>(&aegp_get_project_by_index);
    g_aegp_project_suite6[4] =
        reinterpret_cast<void*>(&aegp_get_project_root_folder);
    *suite = g_aegp_project_suite6.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Item Suite") && version == 14 &&
      (state().active_idle_roundtrip_mode || state().comp_idle_roundtrip_mode)) {
    std::copy_n(unsupported_suite_slots<UnsupportedSuiteId::aegp_item_14, 26>().data(),
                26, reinterpret_cast<void**>(&g_aegp_item_suite));
    reinterpret_cast<void**>(&g_aegp_item_suite)[0] =
        reinterpret_cast<void*>(&aegp_get_first_project_item);
    reinterpret_cast<void**>(&g_aegp_item_suite)[1] =
        reinterpret_cast<void*>(&aegp_get_next_project_item);
    g_aegp_item_suite.get_active_item = &aegp_get_active_item;
    g_aegp_item_suite.get_item_type = &aegp_get_item_type;
    g_aegp_item_suite.after_get_item_type[1] = reinterpret_cast<void*>(&aegp_get_item_name);
    g_aegp_item_suite.after_get_item_type[3] = reinterpret_cast<void*>(&aegp_get_item_id);
    g_aegp_item_suite.after_get_item_type[8] = reinterpret_cast<void*>(&aegp_get_item_duration);
    g_aegp_item_suite.after_get_item_type[9] =
        reinterpret_cast<void*>(&aegp_get_item_current_time);
    g_aegp_item_suite.after_get_item_type[14] =
        reinterpret_cast<void*>(&aegp_set_item_current_time);
    *suite = &g_aegp_item_suite;
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Item Suite") && version == 10) {
    std::copy_n(unsupported_suite_slots<UnsupportedSuiteId::aegp_item_10, 26>().data(),
                26, reinterpret_cast<void**>(&g_aegp_legacy_item_suite6));
    g_aegp_legacy_item_suite6.get_active_item = &aegp_get_active_item;
    g_aegp_legacy_item_suite6.get_item_type = &aegp_get_item_type;
    if (!(state().update_menu_mode || state().command_roundtrip_mode ||
          state().active_idle_roundtrip_mode || state().comp_idle_roundtrip_mode)) {
      reinterpret_cast<void**>(&g_aegp_legacy_item_suite6)[16] =
          reinterpret_cast<void*>(&aegp_get_item_dimensions);
    }
    *suite = &g_aegp_legacy_item_suite6;
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Comp Suite") && version == 25 && state().comp_idle_roundtrip_mode) {
    g_aegp_comp_suite11 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_comp_25, 44>();
    g_aegp_comp_suite11[0] = reinterpret_cast<void*>(&aegp_get_comp_from_item);
    g_aegp_comp_suite11[11] = reinterpret_cast<void*>(&aegp_get_comp_framerate);
    g_aegp_comp_suite11[23] =
        reinterpret_cast<void*>(&aegp_create_camera_in_comp);
    g_aegp_comp_suite11[26] = reinterpret_cast<void*>(&aegp_get_comp_selection);
    g_aegp_comp_suite11[37] = reinterpret_cast<void*>(&aegp_get_comp_frame_duration);
    *suite = g_aegp_comp_suite11.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Comp Suite") && version == 26 && state().comp_idle_roundtrip_mode) {
    g_aegp_comp_suite12 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_comp_26, 44>();
    g_aegp_comp_suite12[26] = reinterpret_cast<void*>(&aegp_get_comp_selection);
    *suite = g_aegp_comp_suite12.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Comp Suite") && version == 21) {
    g_aegp_comp_suite10 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_comp_21, 41>();
    g_aegp_comp_suite10[4] = factory.comp_bg_color;
    *suite = g_aegp_comp_suite10.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Comp Suite") && version == 9) {
    g_aegp_comp_suite4 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_comp_9, 28>();
    g_aegp_comp_suite4[0] = reinterpret_cast<void*>(&aegp_get_comp_from_item);
    g_aegp_comp_suite4[1] = reinterpret_cast<void*>(&aegp_get_item_from_comp);
    *suite = g_aegp_comp_suite4.data();
    return SceneSuiteAcquireResult::acquired;
  }

  if (named("AEGP Layer Suite") && version == 15) {
    const bool render_receipt = factory.render_scene_enabled();
    if (render_receipt || state().comp_idle_roundtrip_mode) {
      g_aegp_layer_suite9 =
          unsupported_suite_slots<UnsupportedSuiteId::aegp_layer_15, 53>();
      g_aegp_layer_suite9[0] = reinterpret_cast<void*>(&aegp_get_comp_num_layers);
      g_aegp_layer_suite9[1] = reinterpret_cast<void*>(&aegp_get_comp_layer_by_index);
      g_aegp_layer_suite9[2] = reinterpret_cast<void*>(&aegp_get_active_layer);
      g_aegp_layer_suite9[3] = reinterpret_cast<void*>(&aegp_get_layer_index);
      g_aegp_layer_suite9[4] = reinterpret_cast<void*>(&aegp_get_layer_source_item);
      g_aegp_layer_suite9[6] = reinterpret_cast<void*>(&aegp_get_layer_parent_comp);
      g_aegp_layer_suite9[27] = reinterpret_cast<void*>(&aegp_get_layer_masked_bounds);
      // These sit outside the receipt block because every other table wires
      // them ungated - version 14 has had `AEGP_SetLayerFlag` there since
      // before this gate existed. That is the honest state of the gate: it is
      // not a boundary. A plug-in that wants what the block below withholds
      // acquires version 13 or 14 instead and gets it, in-point included.
      // Making it one, or dropping it, is #921.
      g_aegp_layer_suite9[34] = reinterpret_cast<void*>(&aegp_convert_comp_to_layer_time);
      g_aegp_layer_suite9[35] = reinterpret_cast<void*>(&aegp_convert_layer_to_comp_time);
      g_aegp_layer_suite9[38] = reinterpret_cast<void*>(&aegp_get_layer_to_world_xform);
      if (!render_receipt) {
        g_aegp_layer_suite9[7] = reinterpret_cast<void*>(&aegp_get_layer_name);
        g_aegp_layer_suite9[15] = reinterpret_cast<void*>(&aegp_get_layer_in_point);
        g_aegp_layer_suite9[16] = reinterpret_cast<void*>(&aegp_get_layer_duration);
        g_aegp_layer_suite9[17] =
            reinterpret_cast<void*>(&aegp_set_layer_in_point_and_duration);
        g_aegp_layer_suite9[28] = reinterpret_cast<void*>(&aegp_get_layer_object_type);
        g_aegp_layer_suite9[37] = reinterpret_cast<void*>(&aegp_get_layer_id);
        g_aegp_layer_suite9[41] = reinterpret_cast<void*>(&aegp_get_layer_parent);
        g_aegp_layer_suite9[42] =
            reinterpret_cast<void*>(&aegp_set_layer_parent);
        g_aegp_layer_suite9[43] =
            reinterpret_cast<void*>(&aegp_delete_layer);
        g_aegp_layer_suite9[45] = reinterpret_cast<void*>(&aegp_get_layer_from_id);
      }
      g_aegp_layer_suite9[10] = reinterpret_cast<void*>(&aegp_get_layer_flags);
      g_aegp_layer_suite9[11] = reinterpret_cast<void*>(&aegp_set_layer_flag);
      g_aegp_layer_suite9[22] = reinterpret_cast<void*>(&aegp_get_layer_transfer_mode);
      *suite = g_aegp_layer_suite9.data();
      return SceneSuiteAcquireResult::acquired;
    }
  }
  // Version 5 is `AEGP_LayerSuite1`, frozen in AE 5.0. The suite struct number
  // and the version a plug-in acquires with do not line up - `AEGP_LayerSuite5`
  // is acquired as 11, `Suite8` as 14, `Suite9` as 15 - and 5 is the oldest
  // shape. `Unmult.aex` asks for it and reported
  // PF_Err_INTERNAL_STRUCT_DAMAGED when it could not be acquired (issue #712).
  //
  // Its slots are not the later table shifted by a constant. Version 11
  // inserted `AEGP_GetLayerSourceItemID` at 5 and `AEGP_ConvertLayerToCompTime`
  // at 35, so a slot here moves by +1 from 5 through 33 and by +2 from 34 on:
  // `AEGP_GetLayerTransferMode` is 21 here and 22 there, `AEGP_GetLayerID` is
  // 35 here and 37 there. Using the later indices would hand the plug-in a
  // different function at every one of them.
  //
  // Only functions whose version 1 declaration matches the implementation are
  // wired; the rest keep the unsupported stub, so a call is recorded instead of
  // guessed at. `AEGP_GetLayerName` (slot 6) is one of those on purpose:
  // versions 1 and 5 hand back two `A_char` buffers, while
  // `aegp_get_layer_name` implements the version 8 shape (a plug-in id plus two
  // `AEGP_MemHandle` outputs). Pointing one at the other would be a different
  // call, not a compatible one.
  //
  // Unconditional, like version 14 beside it. Version 11 is behind
  // `comp_idle_roundtrip_mode` and version 15 withholds part of its table under
  // a render receipt; neither gate is adopted here because this table exposes
  // nothing they withhold that version 14 does not already expose ungated.
  if (named("AEGP Layer Suite") && version == 5) {
    g_aegp_layer_suite1 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_layer_5, 39>();
    g_aegp_layer_suite1[0] = reinterpret_cast<void*>(&aegp_get_comp_num_layers);
    g_aegp_layer_suite1[1] = reinterpret_cast<void*>(&aegp_get_comp_layer_by_index);
    g_aegp_layer_suite1[2] = reinterpret_cast<void*>(&aegp_get_active_layer);
    g_aegp_layer_suite1[3] = reinterpret_cast<void*>(&aegp_get_layer_index);
    g_aegp_layer_suite1[4] = reinterpret_cast<void*>(&aegp_get_layer_source_item);
    g_aegp_layer_suite1[5] = reinterpret_cast<void*>(&aegp_get_layer_parent_comp);
    g_aegp_layer_suite1[9] = reinterpret_cast<void*>(&aegp_get_layer_flags);
    g_aegp_layer_suite1[10] = reinterpret_cast<void*>(&aegp_set_layer_flag);
    g_aegp_layer_suite1[14] = reinterpret_cast<void*>(&aegp_get_layer_in_point);
    g_aegp_layer_suite1[15] = reinterpret_cast<void*>(&aegp_get_layer_duration);
    g_aegp_layer_suite1[16] =
        reinterpret_cast<void*>(&aegp_set_layer_in_point_and_duration);
    g_aegp_layer_suite1[21] = reinterpret_cast<void*>(&aegp_get_layer_transfer_mode);
    // Version 5 is the one table where this sits at 26, not 27: the single
    // function version 11 inserted ahead of it (`AEGP_GetLayerSourceItemID`,
    // at slot 5) accounts for the whole shift. The other insertion that table
    // note describes lands at 35, behind this.
    g_aegp_layer_suite1[26] = reinterpret_cast<void*>(&aegp_get_layer_masked_bounds);
    g_aegp_layer_suite1[27] = reinterpret_cast<void*>(&aegp_get_layer_object_type);
    // Version 5 publishes only the comp-to-layer direction; the layer-to-comp
    // one arrived later, so there is nothing to wire for it here.
    g_aegp_layer_suite1[33] = reinterpret_cast<void*>(&aegp_convert_comp_to_layer_time);
    g_aegp_layer_suite1[35] = reinterpret_cast<void*>(&aegp_get_layer_id);
    g_aegp_layer_suite1[36] = reinterpret_cast<void*>(&aegp_get_layer_to_world_xform);
    *suite = g_aegp_layer_suite1.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Layer Suite") && version == 11) {
    g_aegp_layer_suite5 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_layer_11, 46>();
    g_aegp_layer_suite5[0] = reinterpret_cast<void*>(&aegp_get_comp_num_layers);
    g_aegp_layer_suite5[1] = reinterpret_cast<void*>(&aegp_get_comp_layer_by_index);
    g_aegp_layer_suite5[2] = reinterpret_cast<void*>(&aegp_get_active_layer);
    g_aegp_layer_suite5[3] = reinterpret_cast<void*>(&aegp_get_layer_index);
    g_aegp_layer_suite5[4] = reinterpret_cast<void*>(&aegp_get_layer_source_item);
    g_aegp_layer_suite5[6] = reinterpret_cast<void*>(&aegp_get_layer_parent_comp);
    // `AEGP_LayerSuite5::AEGP_GetLayerName` is the legacy three-argument
    // fixed-buffer form. `aegp_get_layer_name` implements the later
    // four-argument MemHandle form, so slot 7 must remain the fail-closed stub
    // until a matching legacy implementation exists (issue #718).
    g_aegp_layer_suite5[15] = reinterpret_cast<void*>(&aegp_get_layer_in_point);
    g_aegp_layer_suite5[16] = reinterpret_cast<void*>(&aegp_get_layer_duration);
    g_aegp_layer_suite5[17] = reinterpret_cast<void*>(&aegp_set_layer_in_point_and_duration);
    g_aegp_layer_suite5[38] = reinterpret_cast<void*>(&aegp_get_layer_to_world_xform);
    g_aegp_layer_suite5[27] = reinterpret_cast<void*>(&aegp_get_layer_masked_bounds);
    g_aegp_layer_suite5[34] = reinterpret_cast<void*>(&aegp_convert_comp_to_layer_time);
    g_aegp_layer_suite5[35] = reinterpret_cast<void*>(&aegp_convert_layer_to_comp_time);
    g_aegp_layer_suite5[41] = reinterpret_cast<void*>(&aegp_get_layer_parent);
    g_aegp_layer_suite5[45] = reinterpret_cast<void*>(&aegp_get_layer_from_id);
    g_aegp_layer_suite5[10] = reinterpret_cast<void*>(&aegp_get_layer_flags);
    g_aegp_layer_suite5[11] = reinterpret_cast<void*>(&aegp_set_layer_flag);
    g_aegp_layer_suite5[22] = reinterpret_cast<void*>(&aegp_get_layer_transfer_mode);
    g_aegp_layer_suite5[28] = reinterpret_cast<void*>(&aegp_get_layer_object_type);
    *suite = g_aegp_layer_suite5.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Layer Suite") && version == 13) {
    // The same slots version 14 fills, in a table two entries shorter. Filled
    // separately rather than copied from the version 14 table so each table
    // carries its own version's diagnostic stubs: a plug-in that reaches an
    // unimplemented slot is recorded against the version it acquired.
    g_aegp_layer_suite7 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_layer_13, 48>();
    g_aegp_layer_suite7[0] = reinterpret_cast<void*>(&aegp_get_comp_num_layers);
    g_aegp_layer_suite7[1] = reinterpret_cast<void*>(&aegp_get_comp_layer_by_index);
    g_aegp_layer_suite7[2] = reinterpret_cast<void*>(&aegp_get_active_layer);
    g_aegp_layer_suite7[3] = reinterpret_cast<void*>(&aegp_get_layer_index);
    g_aegp_layer_suite7[4] = reinterpret_cast<void*>(&aegp_get_layer_source_item);
    g_aegp_layer_suite7[6] = reinterpret_cast<void*>(&aegp_get_layer_parent_comp);
    g_aegp_layer_suite7[7] = reinterpret_cast<void*>(&aegp_get_layer_name);
    g_aegp_layer_suite7[10] = reinterpret_cast<void*>(&aegp_get_layer_flags);
    g_aegp_layer_suite7[11] = reinterpret_cast<void*>(&aegp_set_layer_flag);
    g_aegp_layer_suite7[22] = reinterpret_cast<void*>(&aegp_get_layer_transfer_mode);
    g_aegp_layer_suite7[27] = reinterpret_cast<void*>(&aegp_get_layer_masked_bounds);
    g_aegp_layer_suite7[34] = reinterpret_cast<void*>(&aegp_convert_comp_to_layer_time);
    g_aegp_layer_suite7[35] = reinterpret_cast<void*>(&aegp_convert_layer_to_comp_time);
    g_aegp_layer_suite7[38] = reinterpret_cast<void*>(&aegp_get_layer_to_world_xform);
    g_aegp_layer_suite7[41] = reinterpret_cast<void*>(&aegp_get_layer_parent);
    g_aegp_layer_suite7[45] = reinterpret_cast<void*>(&aegp_get_layer_from_id);
    g_aegp_layer_suite7[15] = reinterpret_cast<void*>(&aegp_get_layer_in_point);
    g_aegp_layer_suite7[16] = reinterpret_cast<void*>(&aegp_get_layer_duration);
    g_aegp_layer_suite7[17] = reinterpret_cast<void*>(&aegp_set_layer_in_point_and_duration);
    g_aegp_layer_suite7[28] = reinterpret_cast<void*>(&aegp_get_layer_object_type);
    *suite = g_aegp_layer_suite7.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Layer Suite") && version == 14) {
    g_aegp_layer_suite8 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_layer_14, 50>();
    g_aegp_layer_suite8[0] = reinterpret_cast<void*>(&aegp_get_comp_num_layers);
    g_aegp_layer_suite8[1] = reinterpret_cast<void*>(&aegp_get_comp_layer_by_index);
    g_aegp_layer_suite8[2] = reinterpret_cast<void*>(&aegp_get_active_layer);
    g_aegp_layer_suite8[3] = reinterpret_cast<void*>(&aegp_get_layer_index);
    g_aegp_layer_suite8[4] = reinterpret_cast<void*>(&aegp_get_layer_source_item);
    g_aegp_layer_suite8[6] = reinterpret_cast<void*>(&aegp_get_layer_parent_comp);
    g_aegp_layer_suite8[7] = reinterpret_cast<void*>(&aegp_get_layer_name);
    g_aegp_layer_suite8[10] = reinterpret_cast<void*>(&aegp_get_layer_flags);
    g_aegp_layer_suite8[11] = reinterpret_cast<void*>(&aegp_set_layer_flag);
    g_aegp_layer_suite8[22] = reinterpret_cast<void*>(&aegp_get_layer_transfer_mode);
    g_aegp_layer_suite8[27] = reinterpret_cast<void*>(&aegp_get_layer_masked_bounds);
    g_aegp_layer_suite8[34] = reinterpret_cast<void*>(&aegp_convert_comp_to_layer_time);
    g_aegp_layer_suite8[35] = reinterpret_cast<void*>(&aegp_convert_layer_to_comp_time);
    g_aegp_layer_suite8[38] = reinterpret_cast<void*>(&aegp_get_layer_to_world_xform);
    g_aegp_layer_suite8[41] = reinterpret_cast<void*>(&aegp_get_layer_parent);
    g_aegp_layer_suite8[45] = reinterpret_cast<void*>(&aegp_get_layer_from_id);
    g_aegp_layer_suite8[15] = reinterpret_cast<void*>(&aegp_get_layer_in_point);
    g_aegp_layer_suite8[16] = reinterpret_cast<void*>(&aegp_get_layer_duration);
    g_aegp_layer_suite8[17] = reinterpret_cast<void*>(&aegp_set_layer_in_point_and_duration);
    g_aegp_layer_suite8[28] = reinterpret_cast<void*>(&aegp_get_layer_object_type);
    *suite = g_aegp_layer_suite8.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Collection Suite") && version == 2 && state().comp_idle_roundtrip_mode) {
    std::copy_n(
        unsupported_suite_slots<UnsupportedSuiteId::aegp_collection_2, 6>().data(),
        6, reinterpret_cast<void**>(&g_aegp_collection_suite));
    g_aegp_collection_suite.dispose_collection = &aegp_dispose_collection;
    g_aegp_collection_suite.get_count = &aegp_get_collection_count;
    g_aegp_collection_suite.get_by_index = &aegp_get_collection_item;
    *suite = &g_aegp_collection_suite;
    return SceneSuiteAcquireResult::acquired;
  }

  if (named("AEGP Effect Suite") && version == 4 && state().comp_idle_roundtrip_mode) {
    g_aegp_effect_suite4 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_effect_4, 22>();
    g_aegp_effect_suite4[0] = reinterpret_cast<void*>(&aegp_get_layer_num_effects);
    g_aegp_effect_suite4[1] = reinterpret_cast<void*>(&aegp_get_layer_effect_by_index);
    g_aegp_effect_suite4[2] =
        reinterpret_cast<void*>(&aegp_get_installed_key_from_layer_effect);
    g_aegp_effect_suite4[3] = factory.effect_param_union;
    g_aegp_effect_suite4[4] = reinterpret_cast<void*>(&aegp_get_effect_flags);
    g_aegp_effect_suite4[5] = reinterpret_cast<void*>(&aegp_set_effect_flags);
    g_aegp_effect_suite4[6] = reinterpret_cast<void*>(&aegp_reorder_effect);
    g_aegp_effect_suite4[8] = reinterpret_cast<void*>(&aegp_dispose_effect);
    g_aegp_effect_suite4[9] = reinterpret_cast<void*>(&aegp_apply_effect);
    g_aegp_effect_suite4[10] = reinterpret_cast<void*>(&aegp_delete_layer_effect);
    g_aegp_effect_suite4[11] = reinterpret_cast<void*>(&aegp_get_num_installed_effects);
    g_aegp_effect_suite4[12] = reinterpret_cast<void*>(&aegp_get_next_installed_effect);
    g_aegp_effect_suite4[13] = reinterpret_cast<void*>(&aegp_get_effect_name);
    g_aegp_effect_suite4[14] = reinterpret_cast<void*>(&aegp_get_effect_match_name);
    g_aegp_effect_suite4[15] = reinterpret_cast<void*>(&aegp_get_effect_category);
    g_aegp_effect_suite4[16] = reinterpret_cast<void*>(&aegp_duplicate_effect);
    *suite = g_aegp_effect_suite4.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Effect Suite") && (version == 2 || version == 3)) {
    auto& effect_suite = version == 2
        ? g_aegp_effect_suite2 : g_aegp_effect_suite3;
    effect_suite = version == 2
        ? unsupported_suite_slots<UnsupportedSuiteId::aegp_effect_2, 17>()
        : unsupported_suite_slots<UnsupportedSuiteId::aegp_effect_3, 17>();
    effect_suite[0] = reinterpret_cast<void*>(&aegp_get_layer_num_effects);
    effect_suite[1] = reinterpret_cast<void*>(&aegp_get_layer_effect_by_index);
    effect_suite[2] =
        reinterpret_cast<void*>(&aegp_get_installed_key_from_layer_effect);
    effect_suite[3] = factory.effect_param_union;
    effect_suite[4] = reinterpret_cast<void*>(&aegp_get_effect_flags);
    effect_suite[5] = reinterpret_cast<void*>(&aegp_set_effect_flags);
    effect_suite[6] = reinterpret_cast<void*>(&aegp_reorder_effect);
    effect_suite[8] = reinterpret_cast<void*>(&aegp_dispose_effect);
    effect_suite[9] = reinterpret_cast<void*>(&aegp_apply_effect);
    effect_suite[10] = reinterpret_cast<void*>(&aegp_delete_layer_effect);
    effect_suite[16] = reinterpret_cast<void*>(&aegp_duplicate_effect);
    *suite = effect_suite.data();
    return SceneSuiteAcquireResult::acquired;
  }

  if (named("AEGP Stream Suite") && version == 11 && state().comp_idle_roundtrip_mode) {
    g_aegp_stream_suite6 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_stream_11, 23>();
    g_aegp_stream_suite6[3] = reinterpret_cast<void*>(&aegp_get_new_layer_stream);
    g_aegp_stream_suite6[4] = reinterpret_cast<void*>(&aegp_get_effect_num_param_streams_v6);
    g_aegp_stream_suite6[5] = reinterpret_cast<void*>(&aegp_get_new_effect_stream_by_index);
    g_aegp_stream_suite6[6] =
        reinterpret_cast<void*>(&aexcompat::l2_detail::get_new_mask_stream);
    g_aegp_stream_suite6[7] = reinterpret_cast<void*>(&aegp_dispose_stream);
    g_aegp_stream_suite6[8] = reinterpret_cast<void*>(&aegp_get_stream_name);
    g_aegp_stream_suite6[12] = reinterpret_cast<void*>(&aegp_get_stream_type);
    g_aegp_stream_suite6[13] = reinterpret_cast<void*>(&aegp_get_new_stream_value);
    g_aegp_stream_suite6[14] = reinterpret_cast<void*>(&aegp_dispose_stream_value);
    g_aegp_stream_suite6[15] = reinterpret_cast<void*>(&aegp_set_effect_stream_value);
    *suite = g_aegp_stream_suite6.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Stream Suite") && version == 7) {
    g_aegp_stream_suite2 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_stream_7, 22>();
    g_aegp_stream_suite2[4] = reinterpret_cast<void*>(&aegp_get_effect_num_param_streams_v2);
    g_aegp_stream_suite2[5] = factory.legacy_stream_callbacks[0];
    g_aegp_stream_suite2[7] = factory.legacy_stream_callbacks[1];
    g_aegp_stream_suite2[8] = factory.legacy_stream_callbacks[2];
    g_aegp_stream_suite2[12] = factory.legacy_stream_callbacks[3];
    g_aegp_stream_suite2[13] = factory.legacy_stream_callbacks[4];
    g_aegp_stream_suite2[14] = factory.legacy_stream_callbacks[5];
    g_aegp_stream_suite2[15] = factory.legacy_stream_callbacks[6];
    g_aegp_stream_suite2[16] = reinterpret_cast<void*>(&aegp_get_layer_stream_value_v2);
    *suite = g_aegp_stream_suite2.data();
    return SceneSuiteAcquireResult::acquired;
  }
  if (named("AEGP Keyframe Suite") && version == 5 && state().comp_idle_roundtrip_mode) {
    g_aegp_keyframe_suite5 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_keyframe_5, 22>();
    g_aegp_keyframe_suite5[0] = reinterpret_cast<void*>(&aegp_get_stream_num_keyframes);
    g_aegp_keyframe_suite5[1] = reinterpret_cast<void*>(&aegp_get_keyframe_time);
    g_aegp_keyframe_suite5[4] = reinterpret_cast<void*>(&aegp_get_new_keyframe_value);
    g_aegp_keyframe_suite5[14] = reinterpret_cast<void*>(&aegp_get_keyframe_interpolation);
    for (std::size_t slot = 0; slot < g_aegp_keyframe_suite5.size(); ++slot) {
      if (factory.keyframe_callbacks[slot])
        g_aegp_keyframe_suite5[slot] = factory.keyframe_callbacks[slot];
    }
    *suite = g_aegp_keyframe_suite5.data();
    return SceneSuiteAcquireResult::acquired;
  }

  return SceneSuiteAcquireResult::not_handled;
}

// Legacy AEGP effect stream (v2) and effect-param-union (v3) callbacks moved
// from worker_main (issue #170); their slot storage already lives in
// scene_runtime_state(), and worker_main's suite tables keep resolving them
// through its own cross-TU declarations.
namespace aexcompat::l2_detail {

// bump_render_project_timestamp resolves through this TU's scene-context
// hook macro to the same worker-entry bump the callbacks used before.

namespace {
constexpr std::size_t kParamSize = 176;
auto& g_aegp_stream_acquires = scene_runtime_state().stream_acquires;
auto& g_aegp_stream_disposes = scene_runtime_state().stream_disposes;
auto& g_aegp_stream_value_acquires = scene_runtime_state().stream_value_acquires;
auto& g_aegp_stream_value_disposes = scene_runtime_state().stream_value_disposes;
auto& g_aegp_effect_param_union_calls = scene_runtime_state().effect_param_union_calls;
}  // namespace

constexpr int32_t kPfErrBadCallbackParam = 516;

int32_t __cdecl aegp_get_new_effect_stream_by_index_v2(
    int32_t plugin_id, void* effect, int32_t index, void** stream) {
  std::size_t instance_index = 0;
  const auto* instance = resolve_effect_instance(effect, plugin_id, &instance_index);
  const auto* parameter = instance
      ? find_effect_parameter(*instance, index) : nullptr;
  ObjectSnapshot effect_identity{};
  bool possessed = scene_registry().resolve_possessed(
      effect, ObjectKind::effect, plugin_id, effect_identity);
  // The PF-interface handle is not a registry-borrowed one, so possession
  // has to be read off the instance it names (issue #909).
  if (!possessed && instance && effect == &g_aegp_effect && g_aegp_effect_live &&
      ensure_effect_identity(instance_index)) {
    effect_identity = ObjectSnapshot{};
    if (scene_registry().snapshot(
            g_aegp_effect_instances[instance_index].identity, effect_identity))
      possessed = true;
  }
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:effect_stream index=" << index
              << " plugin_id=" << plugin_id
              << " instance=" << (instance != nullptr)
              << " stream=" << (stream != nullptr)
              << " parameter=" << (parameter != nullptr)
              << " possessed=" << possessed << "\n" << std::flush;
  if (plugin_id < 0 || !instance || !stream || !parameter || !possessed ||
      g_aegp_legacy_effect_stream_generation == UINT32_MAX)
    return 4;
  const auto free_slot = std::find_if(g_aegp_legacy_effect_streams.begin(),
      g_aegp_legacy_effect_streams.end(), [](const auto& value) { return !value.live; });
  if (free_slot == g_aegp_legacy_effect_streams.end()) return 4;
  auto& value = *free_slot;
  const std::size_t slot = static_cast<std::size_t>(
      std::distance(g_aegp_legacy_effect_streams.begin(), free_slot));
  Identity identity{};
  void* published = nullptr;
  if (!scene_registry().create_child_borrowed(
          ObjectKind::stream, effect_identity.identity,
          static_cast<int32_t>(slot), &value, u"Effect Stream",
          plugin_id, identity, published))
    return 4;
  const uint32_t generation = ++g_aegp_legacy_effect_stream_generation;
  value.param_index = index;
  value.live = true;
  value.hidden = false;
  value.value_live = false;
  value.effect_instance_index = static_cast<uint32_t>(instance_index);
  value.effect_instance_generation = instance->generation;
  value.generation = generation;
  value.owner_plugin_id = plugin_id;
  value.identity = identity;
  value.handle = published;
  aexcompat::scene_model::StreamState stream_state{};
  stream_state.value_kind = stream_value_kind(parameter->type);
  stream_state.dimensions = parameter->type == 6 ? 4 :
      (parameter->type == 2 ? 3 : (parameter->type == 4 ? 2 : 1));
  stream_state.temporal_dimensions = 1;
  if (stream_state.value_kind ==
          aexcompat::scene_model::StreamValueKind::none ||
      !scene_registry().initialize_stream_state(identity, stream_state)) {
    scene_registry().erase_tree(identity);
    value = {};
    return 4;
  }
  ++g_aegp_stream_acquires;
  *stream = published;
  return 0;
}
AegpLegacyEffectStream* legacy_effect_stream(void* stream) {
  ObjectSnapshot resolved{};
  int32_t possession_id = 0;
  if (!scene_registry().resolve(stream, ObjectKind::stream, resolved) ||
      !scene_registry().possession(
          stream, ObjectKind::stream, possession_id) ||
      resolved.local_index < 0 ||
      static_cast<std::size_t>(resolved.local_index) >=
          g_aegp_legacy_effect_streams.size())
    return nullptr;
  auto& value = g_aegp_legacy_effect_streams[
      static_cast<std::size_t>(resolved.local_index)];
  return value.live && value.handle == stream &&
      value.identity == resolved.identity &&
      value.owner_plugin_id == possession_id ? &value : nullptr;
}
bool legacy_effect_stream_parent_live(const AegpLegacyEffectStream& stream) {
  if (stream.effect_instance_index >= g_aegp_effect_instances.size()) return false;
  const auto& instance = g_aegp_effect_instances[stream.effect_instance_index];
  ObjectSnapshot resolved{};
  return instance.occupied &&
      instance.generation == stream.effect_instance_generation &&
      scene_registry().snapshot(stream.identity, resolved) &&
      resolved.owner == instance.identity;
}
int32_t __cdecl aegp_get_stream_name_v2(void* stream, uint8_t, char* name) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) || !name) return 4;
  const auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(instance, value->param_index);
  if (!parameter) return 4;
  // AEGP_MAX_STREAM_NAME_SIZE is PF_MAX_EFFECT_PARAM_NAME_LEN + 1 = 32, and
  // the caller's buffer is that size. A fixture name is a short literal, but a
  // loaded plug-in's is whatever `add_param` read out of its PF_ParamDef, and
  // that is `strnlen_s(name, 32)` - a name field with no terminator yields 32
  // characters, one more than `strcpy` may write here. Truncate instead.
  constexpr std::size_t kAegpMaxStreamNameSize = 32;
  const std::size_t length =
      strnlen_s(parameter->name, kAegpMaxStreamNameSize - 1);
  std::memcpy(name, parameter->name, length);
  name[length] = '\0';
  return 0;
}
int32_t __cdecl aegp_get_stream_type_v2(void* stream, int32_t* type) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) || !type) return 4;
  const auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(instance, value->param_index);
  if (!parameter) return 4;
  *type = parameter->type;
  return 0;
}
int32_t __cdecl aegp_get_new_stream_value_v2(
    int32_t plugin_id, void* stream, int32_t, const AegpTime* time,
    uint8_t, AegpStreamValue* output) {
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:stream_value plugin_id=" << plugin_id
              << " stream=" << stream << " time=" << (time != nullptr)
              << " output=" << (output != nullptr) << "\n" << std::flush;
  auto* value = legacy_effect_stream(stream);
  // `value` is null for a stale, foreign, disposed, or malformed handle, so
  // nothing may read through it before this refusal. Hoisting the id
  // comparison above this line dereferenced null for exactly the handles the
  // check exists to reject, turning a diagnostic into a crash.
  if (!value || !legacy_effect_stream_parent_live(*value)) return 4;
  // The id only has to name the same caller the stream was opened for. An
  // unregistered plug-in passes 0 here and may have passed something else
  // when it opened the stream - DeepGlow2 does exactly that - and the stream
  // it is reading is still its own: this worker hosts one plug-in, and the
  // registry already refused any handle that is not this stream (issue #909).
  // A negative id stays malformed, as at every other entry point here.
  const bool owner_matches = plugin_id >= 0 &&
      (plugin_id == value->owner_plugin_id || plugin_id == 0 ||
       value->owner_plugin_id == 0);
  if (!owner_matches || value->value_live || !time || time->scale == 0 ||
      !output)
    return 4;
  const auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(instance, value->param_index);
  if (!parameter) return 4;
  output->stream = stream;
  output->value.fill(std::byte{});
  const std::size_t fixture_slot =
      static_cast<std::size_t>(value->param_index - 1);
  if (value->param_index == 0) {
    std::memcpy(output->value.data(), &instance.layer, sizeof(instance.layer));
  } else if (!instance.loaded_plugin &&
             fixture_slot < instance.parameter_values.size()) {
    std::memcpy(output->value.data(),
                instance.parameter_values[fixture_slot].data(),
                sizeof(instance.parameter_values[0]));
  }
  // `parameter_values` holds fixture defaults, seeded for the probe and
  // written only by `initialize_effect_parameter_values` when an effect is
  // applied. For the loaded plug-in they are not its values, and reading the
  // first four of them would have answered its first four parameters with the
  // probe's numbers while name and type came from the plug-in - worse than
  // the zero every parameter past those four already reads as. This host does
  // not model a loaded plug-in's current stream values; #929 is where that
  // would be built.
  // Past the fixture's slots the value stays zero, which is what a stream
  // this host does not model reads as: for a layer parameter that is "no
  // layer", the answer DeepGlow2's matte path is asking for when its matte
  // parameter is left unset. Reading `parameter_values` there would have been
  // an out-of-bounds index - the array holds four.
  value->value_live = true;
  value->checked_out_value = output;
  if (!scene_registry().create_child(
          ObjectKind::value, value->identity, 0, output,
          u"Stream Value", value->value_identity)) {
    value->value_live = false;
    value->checked_out_value = nullptr;
    return 4;
  }
  ++g_aegp_stream_value_acquires;
  return 0;
}
int32_t __cdecl aegp_dispose_stream_value_v2(AegpStreamValue* output) {
  if (!output) return 4;
  auto* stream = legacy_effect_stream(output->stream);
  Identity value_identity{};
  ObjectSnapshot value_snapshot{};
  if (!stream || !stream->value_live || stream->checked_out_value != output ||
      !scene_registry().identity_for_legacy(
          output, ObjectKind::value, value_identity) ||
      !scene_registry().snapshot(value_identity, value_snapshot) ||
      value_identity != stream->value_identity ||
      value_snapshot.owner != stream->identity ||
      !scene_registry().erase_tree(value_identity))
    return 4;
  output->stream = nullptr;
  stream->value_live = false;
  stream->checked_out_value = nullptr;
  stream->value_identity = {};
  ++g_aegp_stream_value_disposes;
  return 0;
}
int32_t __cdecl aegp_set_stream_value_v2(
    int32_t plugin_id, void* stream, AegpStreamValue* input) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) ||
      plugin_id != value->owner_plugin_id || !value->value_live || !input ||
      input->stream != stream || value->checked_out_value != input)
    return 4;
  const auto& current = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(current, value->param_index);
  if (!parameter || value->param_index == 0 || !parameter->writable) return 4;
  std::array<double, 4> candidate{};
  std::memcpy(candidate.data(), input->value.data(), sizeof(candidate));
  for (std::size_t index = 0; index < candidate.size(); ++index)
    if (!std::isfinite(candidate[static_cast<std::size_t>(index)])) return 4;
  auto candidate_instances = g_aegp_effect_instances;
  candidate_instances[value->effect_instance_index]
      .parameter_values[static_cast<std::size_t>(value->param_index - 1)] =
          candidate;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), value->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        g_aegp_effect_instances = candidate_instances;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl aegp_dispose_stream_v2(void* stream) {
  auto* value = legacy_effect_stream(stream);
  if (!value || value->value_live ||
      !scene_registry().erase_tree(value->identity))
    return 4;
  value->live = false;
  value->param_index = -1;
  value->effect_instance_index = 0;
  value->effect_instance_generation = 0;
  value->checked_out_value = nullptr;
  value->owner_plugin_id = 0;
  value->identity = {};
  value->value_identity = {};
  value->handle = nullptr;
  ++g_aegp_stream_disposes;
  return 0;
}
int32_t __cdecl aegp_set_dynamic_stream_flag_v2(
    void* stream, uint32_t one_flag, uint8_t undoable, uint8_t set) {
  aexcompat::scene_transaction::MutationLock mutation_lock;
  auto* value = legacy_effect_stream(stream);
  constexpr uint32_t kHidden = 1u << 1;
  if (!value || !legacy_effect_stream_parent_live(*value) ||
      one_flag != kHidden || undoable > 1 || set > 1) return 4;
  const bool hidden = set != 0;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      scene_registry(), value->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation,
      std::move(mutation_lock));
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        value->hidden = hidden;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl aegp_get_effect_param_union_by_index_v3(
    int32_t plugin_id, void* effect, int32_t index, int32_t* type, void* param_union) {
  if (plugin_id < 0 || !resolve_effect_instance(effect, plugin_id) || !type ||
      !param_union || index < 0 || index >= 5) return 4;
  // AEGP effect inspection is independent of PF selector-local parameter
  // buffers. These are definition unions for the bounded synthetic scene,
  // never current values (which belong to the Stream Suite).
  static constexpr std::array<int32_t, 5> kTypes{0, 1, 4, 5, 10};
  static constexpr std::array<std::array<std::byte, kParamSize - 56>, 5> kUnions{};
  *type = kTypes[static_cast<std::size_t>(index)];
  std::memcpy(param_union, kUnions[static_cast<std::size_t>(index)].data(),
              kUnions[static_cast<std::size_t>(index)].size());
  ++g_aegp_effect_param_union_calls;
  return 0;
}

}  // namespace aexcompat::l2_detail
