// Copyright (c) AEXCompat contributors.
// Independent compiled implementation for the AEGP scene family.

#include "worker_aegp_scene.hpp"
#include "worker_aegp_scene_model.hpp"
#include "worker_suite_registry.hpp"

#include <algorithm>
#include <climits>
#include <cmath>
#include <cstring>
#include <iterator>

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

void* effect_lease_handle(std::size_t slot, uint32_t generation) {
  const uintptr_t value = (static_cast<uintptr_t>(generation) << 8) |
      (static_cast<uintptr_t>(slot) << 2) | 1;
  return value > 1 ? reinterpret_cast<void*>(value) : nullptr;
}
const AegpEffectInstance* resolve_effect_instance(void* effect, int32_t owner,
                                                  std::size_t* index) {
  // PF-interface callers historically receive this stable host-owned reference.
  if (effect == &g_aegp_effect && g_aegp_effect_live) {
    if (index) *index = 0;
    return &g_aegp_effect_instances[0];
  }
  const uintptr_t value = reinterpret_cast<uintptr_t>(effect);
  if (!effect || (value & 3) != 1) return nullptr;
  const std::size_t slot = (value >> 2) & 0x3f;
  const uint32_t generation = static_cast<uint32_t>(value >> 8);
  if (slot >= g_aegp_effect_leases.size()) return nullptr;
  const auto& lease = g_aegp_effect_leases[slot];
  if (!lease.live || lease.generation != generation ||
      (owner > 0 && lease.owner_plugin_id != owner) ||
      lease.instance_index >= g_aegp_effect_instances.size()) return nullptr;
  const auto& instance = g_aegp_effect_instances[lease.instance_index];
  if (!instance.occupied || instance.generation != lease.instance_generation) return nullptr;
  if (index) *index = lease.instance_index;
  return &instance;
}
bool acquire_effect_lease(int32_t plugin_id, std::size_t instance_index, void** output) {
  if (plugin_id <= 0 || !output || instance_index >= g_aegp_effect_instances.size() ||
      !g_aegp_effect_instances[instance_index].occupied) return false;
  for (std::size_t slot = 0; slot < g_aegp_effect_leases.size(); ++slot) {
    auto& lease = g_aegp_effect_leases[slot];
    if (lease.live) continue;
    uint32_t generation = ++g_aegp_effect_lease_generation;
    if (generation == 0) generation = ++g_aegp_effect_lease_generation;
    void* handle = effect_lease_handle(slot, generation);
    if (!handle) return false;
    lease = {plugin_id, static_cast<uint32_t>(instance_index),
             g_aegp_effect_instances[instance_index].generation, generation, true};
    *output = handle;
    ++g_aegp_effect_acquires;
    return true;
  }
  return false;
}
const AegpEffectLease* resolve_effect_lease(void* effect, std::size_t* slot_out = nullptr) {
  const uintptr_t value = reinterpret_cast<uintptr_t>(effect);
  if (!effect || (value & 3) != 1) return nullptr;
  const std::size_t slot = (value >> 2) & 0x3f;
  const uint32_t generation = static_cast<uint32_t>(value >> 8);
  if (slot >= g_aegp_effect_leases.size()) return nullptr;
  const auto& lease = g_aegp_effect_leases[slot];
  if (!lease.live || lease.generation != generation) return nullptr;
  if (slot_out) *slot_out = slot;
  return &lease;
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
  for (std::size_t index = 0; index < runtime.effect_instances.size(); ++index) {
    const uint64_t identity =
        effect_instance_identity(index, runtime.effect_instances[index]);
    if (identity != 0)
      metadata.effect_instances[metadata.effect_instance_count++] = identity;
  }
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
const AegpEffectParameterRecord* find_effect_parameter(int32_t key, int32_t index);
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
int32_t __cdecl aegp_get_item_type(void* item, int16_t* item_type);
AegpItemSuite g_aegp_item_suite{};

int32_t __cdecl aegp_get_item_type(void* item, int16_t* item_type) {
  ObjectSnapshot resolved{};
  if (!item_type || !resolve_scene_item(item, resolved)) return 4;
  int16_t result = 0;
  switch (resolved.item_kind) {
    case ItemKind::folder: result = 1; break;
    case ItemKind::composition: result = 2; break;
    case ItemKind::footage: result = 3; break;
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
  return resolve_scene_layer(layer, resolved)
      ? primary_layer_index(resolved) : -1;
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
  if (plugin_id <= 0 || !collection || g_aegp_selection.live ||
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
  const int32_t index = aegp_layer_index(layer);
  if (index < 0 || !flags) return 4;
  *flags = g_aegp_layer_flags[static_cast<std::size_t>(index)];
  ++g_aegp_layer_attribute_calls;
  return 0;
}
int32_t __cdecl aegp_set_layer_flag(void* layer, uint32_t flag, uint8_t value) {
  const int32_t index = aegp_layer_index(layer);
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
  if (aegp_layer_index(layer) < 0 || !type) return 4;
  *type = 0;
  ++g_aegp_layer_attribute_calls;
  return 0;
}
int32_t __cdecl aegp_get_layer_in_point(void* layer, int32_t time_mode, AegpTime* time) {
  const int32_t index = aegp_layer_index(layer);
  if (index < 0 || time_mode != 1 || !time) return 4;
  *time = g_aegp_layer_in_points[static_cast<std::size_t>(index)];
  ++g_aegp_layer_attribute_calls;
  return 0;
}
int32_t __cdecl aegp_get_layer_duration(void* layer, int32_t time_mode, AegpTime* time) {
  const int32_t index = aegp_layer_index(layer);
  if (index < 0 || time_mode != 1 || !time) return 4;
  *time = g_aegp_layer_durations[static_cast<std::size_t>(index)];
  ++g_aegp_layer_attribute_calls;
  return 0;
}
int32_t __cdecl aegp_set_layer_in_point_and_duration(
    void* layer, int32_t time_mode, const AegpTime* in_point, const AegpTime* duration) {
  const int32_t index = aegp_layer_index(layer);
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
  if (plugin_id <= 0 || aegp_layer_index(layer) < 0 || index < 0 || !effect) return 4;
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
  std::size_t instance_index = 0;
  if (!resolve_effect_instance(effect, 0, &instance_index) || (flags & ~set_mask) != 0)
    return 4;
  auto& instance = g_aegp_effect_instances[instance_index];
  instance.flags = (instance.flags & ~set_mask) | flags;
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_reorder_effect(void* effect, int32_t target_order) {
  std::size_t instance_index = 0;
  const auto* resolved = resolve_effect_instance(effect, 0, &instance_index);
  if (!resolved || target_order < 0) return 4;
  auto& instance = g_aegp_effect_instances[instance_index];
  const int32_t count = static_cast<int32_t>(std::count_if(
      g_aegp_effect_instances.begin(), g_aegp_effect_instances.end(),
      [&](const auto& value) { return value.occupied && value.layer == instance.layer; }));
  if (target_order >= count) return 4;
  const int32_t old_order = instance.stack_order;
  if (target_order < old_order) {
    for (auto& value : g_aegp_effect_instances)
      if (value.occupied && value.layer == instance.layer &&
          value.stack_order >= target_order && value.stack_order < old_order)
        ++value.stack_order;
  } else if (target_order > old_order) {
    for (auto& value : g_aegp_effect_instances)
      if (value.occupied && value.layer == instance.layer &&
          value.stack_order > old_order && value.stack_order <= target_order)
        --value.stack_order;
  }
  instance.stack_order = target_order;
  if (target_order != old_order) bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_dispose_effect(void* effect) {
  if (effect == &g_aegp_effect) {
    if (!g_aegp_effect_live) return 4;
    g_aegp_effect_live = false;
    ++g_aegp_effect_disposes;
    return 0;
  }
  std::size_t slot = 0;
  if (!resolve_effect_lease(effect, &slot)) return 4;
  g_aegp_effect_leases[slot].live = false;
  ++g_aegp_effect_disposes;
  return 0;
}
int32_t __cdecl aegp_apply_effect(
    int32_t plugin_id, void* layer, int32_t installed_key, void** effect) {
  if (plugin_id <= 0 || aegp_layer_index(layer) < 0 || !effect ||
      !find_installed_effect(installed_key)) return 4;
  const auto instance_slot = std::find_if(g_aegp_effect_instances.begin(),
      g_aegp_effect_instances.end(), [](const auto& instance) { return !instance.occupied; });
  const auto lease_slot = std::find_if(g_aegp_effect_leases.begin(),
      g_aegp_effect_leases.end(), [](const auto& lease) { return !lease.live; });
  if (instance_slot == g_aegp_effect_instances.end() ||
      lease_slot == g_aegp_effect_leases.end()) return 4;
  const std::size_t instance_index = static_cast<std::size_t>(
      std::distance(g_aegp_effect_instances.begin(), instance_slot));
  const int32_t stack_order = static_cast<int32_t>(std::count_if(
      g_aegp_effect_instances.begin(), g_aegp_effect_instances.end(),
      [layer](const auto& instance) { return instance.occupied && instance.layer == layer; }));
  uint32_t generation = instance_slot->generation + 1;
  if (generation == 0) generation = 1;
  *instance_slot = {layer, installed_key, stack_order, 1, generation, true};
  initialize_effect_parameter_values(*instance_slot);
  if (!acquire_effect_lease(plugin_id, instance_index, effect)) {
    *instance_slot = {};
    instance_slot->generation = generation;
    return 4;
  }
  instance_slot->render_ref = *effect;
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_delete_layer_effect(void* effect) {
  std::size_t instance_index = 0;
  if (!resolve_effect_instance(effect, 0, &instance_index)) return 4;
  auto& instance = g_aegp_effect_instances[instance_index];
  void* layer = instance.layer;
  const int32_t deleted_order = instance.stack_order;
  uint32_t generation = instance.generation + 1;
  if (generation == 0) generation = 1;
  instance = {};
  instance.generation = generation;
  for (auto& value : g_aegp_effect_instances)
    if (value.occupied && value.layer == layer && value.stack_order > deleted_order)
      --value.stack_order;
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_duplicate_effect(void* original, void** duplicate) {
  if (!duplicate) return 4;
  std::size_t original_index = 0;
  const auto* source = resolve_effect_instance(original, 0, &original_index);
  const auto* source_lease = resolve_effect_lease(original);
  if (!source || !source_lease) return 4;
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
  for (auto& value : g_aegp_effect_instances)
    if (value.occupied && value.layer == layer && value.stack_order >= inserted_order)
      ++value.stack_order;
  const std::size_t instance_index = static_cast<std::size_t>(
      std::distance(g_aegp_effect_instances.begin(), instance_slot));
  uint32_t generation = instance_slot->generation + 1;
  if (generation == 0) generation = 1;
  *instance_slot = {const_cast<void*>(layer), installed_key, inserted_order,
                    flags, generation, true};
  instance_slot->parameter_values = source->parameter_values;
  if (!acquire_effect_lease(source_lease->owner_plugin_id, instance_index, duplicate)) {
    *instance_slot = {};
    instance_slot->generation = generation;
    for (auto& value : g_aegp_effect_instances)
      if (value.occupied && value.layer == layer && value.stack_order > inserted_order)
        --value.stack_order;
    return 4;
  }
  instance_slot->render_ref = *duplicate;
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl get_new_effect_for_effect(int32_t plugin_id, void* effect, void** effect_ref) {
  if (plugin_id <= 0 || effect != &g_effect || !effect_ref || g_aegp_effect_live) return 4;
  g_aegp_effect_live = true;
  *effect_ref = &g_aegp_effect;
  ++g_aegp_effect_acquires;
  return 0;
}
const AegpInstalledEffectRecord* find_installed_effect(int32_t key) {
  for (const auto& effect : kAegpInstalledEffects)
    if (effect.key == key) return &effect;
  return nullptr;
}
const AegpEffectParameterRecord* find_effect_parameter(int32_t key, int32_t index) {
  const auto* effect = find_installed_effect(key);
  if (!effect || index < 0 || index >= effect->parameter_count) return nullptr;
  if (key == kAegpInstalledEffects[0].key)
    return &kAegpProbeParameters[static_cast<std::size_t>(index)];
  if (key == kAegpInstalledEffects[1].key || key == kAegpInstalledEffects[2].key)
    return &kAegpLevelsParameters[static_cast<std::size_t>(index)];
  return nullptr;
}
void initialize_effect_parameter_values(AegpEffectInstance& instance) {
  instance.parameter_values = {};
  const auto* effect = find_installed_effect(instance.installed_key);
  if (!effect) return;
  for (int32_t index = 1; index < effect->parameter_count; ++index) {
    const auto* parameter = find_effect_parameter(instance.installed_key, index);
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
  const auto* installed = instance ? find_installed_effect(instance->installed_key) : nullptr;
  if (!installed || !count) return 4;
  *count = installed->parameter_count;
  return 0;
}
bool supported_transform_stream(int32_t selector) {
  return selector == 0 || selector == 1 || selector == 2 || selector == 3 ||
      selector == 4 || selector == 8 || selector == 9;
}
int32_t __cdecl aegp_get_new_layer_stream(
    int32_t plugin_id, void* layer, int32_t selector, void** stream) {
  if (plugin_id <= 0 || aegp_layer_index(layer) < 0 || !stream ||
      !supported_transform_stream(selector) || g_aegp_transform_stream.live) return 4;
  g_aegp_transform_stream.selector = selector;
  g_aegp_transform_stream.layer = layer;
  g_aegp_transform_stream.effect_param = false;
  g_aegp_transform_stream.live = true;
  g_aegp_transform_stream.value_live = false;
  g_aegp_transform_stream.owner_plugin_id = plugin_id;
  ++g_aegp_stream_acquires;
  *stream = &g_aegp_transform_stream.object;
  return 0;
}
int32_t __cdecl aegp_get_new_effect_stream_by_index(
    int32_t plugin_id, void* effect, int32_t index, void** stream) {
  std::size_t instance_index = 0;
  const auto* instance = resolve_effect_instance(effect, plugin_id, &instance_index);
  if (plugin_id <= 0 || !instance ||
      index < 1 || index > 4 || !stream || g_aegp_transform_stream.live) return 4;
  g_aegp_transform_stream.selector = index;
  g_aegp_transform_stream.layer = instance->layer;
  g_aegp_transform_stream.effect_param = true;
  g_aegp_transform_stream.live = true;
  g_aegp_transform_stream.value_live = false;
  g_aegp_transform_stream.effect_instance_index = static_cast<uint32_t>(instance_index);
  g_aegp_transform_stream.effect_instance_generation = instance->generation;
  g_aegp_transform_stream.owner_plugin_id = plugin_id;
  ++g_aegp_stream_acquires;
  *stream = &g_aegp_transform_stream.object;
  return 0;
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
  if (stream != &g_aegp_transform_stream.object || !g_aegp_transform_stream.live ||
      !effect_stream_parent_live() || !type)
    return 4;
  if (g_aegp_transform_stream.effect_param) {
    switch (g_aegp_transform_stream.selector) {
      case 1: *type = 5; break;
      case 2: *type = 4; break;
      case 3: *type = 2; break;
      case 4: *type = 6; break;
      default: return 4;
    }
  } else {
    *type = g_aegp_transform_stream.selector <= 2 ? 3 : 5;
  }
  return 0;
}
int32_t __cdecl aegp_get_stream_num_keyframes(void* stream, int32_t* count) {
  if (stream != &g_aegp_transform_stream.object || !g_aegp_transform_stream.live ||
      !effect_stream_parent_live() || !count)
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
  return stream == &g_aegp_transform_stream.object && g_aegp_transform_stream.live &&
      effect_stream_parent_live() &&
      g_aegp_transform_stream.effect_param && g_aegp_transform_stream.selector == 1 &&
      g_aegp_transform_stream.effect_instance_index == 0 &&
      index >= 0 && index < 2;
}
int32_t __cdecl aegp_get_keyframe_time(
    void* stream, int32_t index, int32_t time_mode, AegpTime* time) {
  if (!valid_amount_keyframe(stream, index) || time_mode != 1 || !time) return 4;
  *time = {index == 0 ? 0 : 60, 30};
  ++g_aegp_keyframe_time_calls;
  return 0;
}
int32_t __cdecl aegp_get_new_keyframe_value(
    int32_t plugin_id, void* stream, int32_t index, AegpStreamValue* value) {
  if (plugin_id != 1 || !valid_amount_keyframe(stream, index) || !value ||
      g_aegp_transform_stream.value_live) return 4;
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
  if (!valid_amount_keyframe(stream, index) || !in_type || !out_type) return 4;
  *in_type = index == 0 ? 1 : 3;
  *out_type = index == 0 ? 1 : 3;
  ++g_aegp_keyframe_interpolation_calls;
  return 0;
}
int32_t __cdecl aegp_get_new_stream_value(
    int32_t plugin_id, void* stream, int32_t, const AegpTime* time,
    uint8_t, AegpStreamValue* value) {
  if (plugin_id <= 0 || stream != &g_aegp_transform_stream.object ||
      !g_aegp_transform_stream.live || !effect_stream_parent_live() ||
      plugin_id != g_aegp_transform_stream.owner_plugin_id ||
      g_aegp_transform_stream.value_live || !time ||
      time->scale == 0 || !value) return 4;
  value->stream = stream;
  value->value.fill(std::byte{});
  double components[2]{};
  if (g_aegp_transform_stream.effect_param) {
    const int32_t selector = g_aegp_transform_stream.selector;
    if (selector < 1 || selector > 4) return 4;
    const auto& instance =
        g_aegp_effect_instances[g_aegp_transform_stream.effect_instance_index];
    std::memcpy(value->value.data(), instance.parameter_values[selector - 1].data(),
                sizeof(instance.parameter_values[selector - 1]));
    ++g_aegp_effect_param_value_calls;
  } else switch (g_aegp_transform_stream.selector) {
    case 1: components[0] = 320.0; components[1] = 180.0; break;
    case 2: components[0] = 100.0; components[1] = 100.0; break;
    case 4: components[0] = 100.0; break;
    default: break;
  }
  if (!g_aegp_transform_stream.effect_param)
    std::memcpy(value->value.data(), components, sizeof(components));
  g_aegp_transform_stream.value_live = true;
  if (!g_aegp_transform_stream.effect_param)
    g_aegp_stream_sampled_selector_mask |= 1u << g_aegp_transform_stream.selector;
  ++g_aegp_stream_value_acquires;
  return 0;
}
int32_t __cdecl aegp_get_stream_name(
    int32_t plugin_id, void* stream, uint8_t, void** name_handle) {
  if (plugin_id <= 0 || stream != &g_aegp_transform_stream.object ||
      !g_aegp_transform_stream.live || !effect_stream_parent_live() ||
      plugin_id != g_aegp_transform_stream.owner_plugin_id ||
      !g_aegp_transform_stream.effect_param || !name_handle)
    return 4;
  *name_handle = nullptr;
  std::u16string name;
  switch (g_aegp_transform_stream.selector) {
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
  if (plugin_id <= 0 || stream != &g_aegp_transform_stream.object ||
      !g_aegp_transform_stream.live || !effect_stream_parent_live() ||
      plugin_id != g_aegp_transform_stream.owner_plugin_id || !value ||
      value->stream != stream || !g_aegp_transform_stream.value_live ||
      !g_aegp_transform_stream.effect_param) return 4;
  const int32_t selector = g_aegp_transform_stream.selector;
  if (selector < 1 || selector > 4 ||
      (selector == 1 && g_aegp_transform_stream.effect_instance_index == 0)) return 4;
  std::array<double, 4> candidate{};
  std::memcpy(candidate.data(), value->value.data(), sizeof(candidate));
  for (int32_t index = 0; index < selector; ++index)
    if (!std::isfinite(candidate[static_cast<std::size_t>(index)])) return 4;
  auto& instance =
      g_aegp_effect_instances[g_aegp_transform_stream.effect_instance_index];
  instance.parameter_values[selector - 1] = candidate;
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_dispose_stream_value(AegpStreamValue* value) {
  if (!value || value->stream != &g_aegp_transform_stream.object ||
      !g_aegp_transform_stream.live || !g_aegp_transform_stream.value_live) return 4;
  value->stream = nullptr;
  g_aegp_transform_stream.value_live = false;
  ++g_aegp_stream_value_disposes;
  return 0;
}
int32_t __cdecl aegp_dispose_stream(void* stream) {
  if (stream != &g_aegp_transform_stream.object || !g_aegp_transform_stream.live ||
      g_aegp_transform_stream.value_live) return 4;
  g_aegp_transform_stream.live = false;
  g_aegp_transform_stream.selector = -1;
  g_aegp_transform_stream.layer = nullptr;
  g_aegp_transform_stream.effect_param = false;
  g_aegp_transform_stream.effect_instance_index = 0;
  g_aegp_transform_stream.effect_instance_generation = 0;
  g_aegp_transform_stream.owner_plugin_id = 0;
  ++g_aegp_stream_disposes;
  return 0;
}

std::array<void*, 41> g_aegp_comp_suite10{};
std::array<void*, 28> g_aegp_comp_suite4{};
std::array<void*, 44> g_aegp_comp_suite11{};
std::array<void*, 44> g_aegp_comp_suite12{};
std::array<void*, 46> g_aegp_layer_suite5{};
std::array<void*, 50> g_aegp_layer_suite8{};
std::array<void*, 53> g_aegp_layer_suite9{};
std::array<void*, 17> g_aegp_effect_suite2{};
std::array<void*, 17> g_aegp_effect_suite3{};
std::array<void*, 22> g_aegp_effect_suite4{};
std::array<void*, 22> g_aegp_stream_suite2{};
std::array<void*, 23> g_aegp_stream_suite6{};
std::array<void*, 22> g_aegp_keyframe_suite5{};
static_assert(sizeof(g_aegp_comp_suite10) == 41 * sizeof(void*));
static_assert(sizeof(g_aegp_comp_suite4) == 28 * sizeof(void*));
static_assert(sizeof(g_aegp_comp_suite11) == 352);
static_assert(sizeof(g_aegp_comp_suite12) == 352);
static_assert(sizeof(g_aegp_layer_suite5) == 368);
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

  if (named("AEGP Item Suite") && version == 14 &&
      (state().active_idle_roundtrip_mode || state().comp_idle_roundtrip_mode)) {
    std::copy_n(unsupported_suite_slots<UnsupportedSuiteId::aegp_item_14, 26>().data(),
                26, reinterpret_cast<void**>(&g_aegp_item_suite));
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
        g_aegp_layer_suite9[45] = reinterpret_cast<void*>(&aegp_get_layer_from_id);
      }
      *suite = g_aegp_layer_suite9.data();
      return SceneSuiteAcquireResult::acquired;
    }
  }
  if (named("AEGP Layer Suite") && version == 11 && state().comp_idle_roundtrip_mode) {
    g_aegp_layer_suite5 =
        unsupported_suite_slots<UnsupportedSuiteId::aegp_layer_11, 46>();
    g_aegp_layer_suite5[0] = reinterpret_cast<void*>(&aegp_get_comp_num_layers);
    g_aegp_layer_suite5[1] = reinterpret_cast<void*>(&aegp_get_comp_layer_by_index);
    g_aegp_layer_suite5[2] = reinterpret_cast<void*>(&aegp_get_active_layer);
    g_aegp_layer_suite5[3] = reinterpret_cast<void*>(&aegp_get_layer_index);
    g_aegp_layer_suite5[4] = reinterpret_cast<void*>(&aegp_get_layer_source_item);
    g_aegp_layer_suite5[6] = reinterpret_cast<void*>(&aegp_get_layer_parent_comp);
    g_aegp_layer_suite5[7] = reinterpret_cast<void*>(&aegp_get_layer_name);
    g_aegp_layer_suite5[15] = reinterpret_cast<void*>(&aegp_get_layer_in_point);
    g_aegp_layer_suite5[16] = reinterpret_cast<void*>(&aegp_get_layer_duration);
    g_aegp_layer_suite5[17] = reinterpret_cast<void*>(&aegp_set_layer_in_point_and_duration);
    g_aegp_layer_suite5[38] = reinterpret_cast<void*>(&aegp_get_layer_to_world_xform);
    g_aegp_layer_suite5[41] = reinterpret_cast<void*>(&aegp_get_layer_parent);
    g_aegp_layer_suite5[45] = reinterpret_cast<void*>(&aegp_get_layer_from_id);
    *suite = g_aegp_layer_suite5.data();
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
    g_aegp_layer_suite8[38] = reinterpret_cast<void*>(&aegp_get_layer_to_world_xform);
    g_aegp_layer_suite8[41] = reinterpret_cast<void*>(&aegp_get_layer_parent);
    g_aegp_layer_suite8[45] = reinterpret_cast<void*>(&aegp_get_layer_from_id);
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
      ? find_effect_parameter(instance->installed_key, index) : nullptr;
  if (plugin_id <= 0 || !instance || !stream || !parameter) return 4;
  const auto free_slot = std::find_if(g_aegp_legacy_effect_streams.begin(),
      g_aegp_legacy_effect_streams.end(), [](const auto& value) { return !value.live; });
  if (free_slot == g_aegp_legacy_effect_streams.end()) return 4;
  auto& value = *free_slot;
  const std::size_t slot = static_cast<std::size_t>(
      std::distance(g_aegp_legacy_effect_streams.begin(), free_slot));
  uint32_t generation = ++g_aegp_legacy_effect_stream_generation;
  if (generation == 0) generation = ++g_aegp_legacy_effect_stream_generation;
  const uintptr_t encoded = (static_cast<uintptr_t>(generation) << 8) |
      (static_cast<uintptr_t>(slot) << 2) | 3;
  if (encoded <= 3) return 4;
  value.param_index = index;
  value.live = true;
  value.hidden = false;
  value.value_live = false;
  value.effect_instance_index = static_cast<uint32_t>(instance_index);
  value.effect_instance_generation = instance->generation;
  value.generation = generation;
  value.owner_plugin_id = plugin_id;
  ++g_aegp_stream_acquires;
  *stream = reinterpret_cast<void*>(encoded);
  return 0;
}
AegpLegacyEffectStream* legacy_effect_stream(void* stream) {
  const uintptr_t encoded = reinterpret_cast<uintptr_t>(stream);
  if (!stream || (encoded & 3) != 3) return nullptr;
  const std::size_t slot = (encoded >> 2) & 0x3f;
  const uint32_t generation = static_cast<uint32_t>(encoded >> 8);
  if (slot >= g_aegp_legacy_effect_streams.size()) return nullptr;
  auto& value = g_aegp_legacy_effect_streams[slot];
  return value.live && value.generation == generation ? &value : nullptr;
}
bool legacy_effect_stream_parent_live(const AegpLegacyEffectStream& stream) {
  if (stream.effect_instance_index >= g_aegp_effect_instances.size()) return false;
  const auto& instance = g_aegp_effect_instances[stream.effect_instance_index];
  return instance.occupied && instance.generation == stream.effect_instance_generation;
}
int32_t __cdecl aegp_get_stream_name_v2(void* stream, uint8_t, char* name) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) || !name) return 4;
  const auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(instance.installed_key, value->param_index);
  if (!parameter) return 4;
  std::strcpy(name, parameter->name);
  return 0;
}
int32_t __cdecl aegp_get_stream_type_v2(void* stream, int32_t* type) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) || !type) return 4;
  const auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(instance.installed_key, value->param_index);
  if (!parameter) return 4;
  *type = parameter->type;
  return 0;
}
int32_t __cdecl aegp_get_new_stream_value_v2(
    int32_t plugin_id, void* stream, int32_t, const AegpTime* time,
    uint8_t, AegpStreamValue* output) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) ||
      plugin_id != value->owner_plugin_id || value->value_live || !time ||
      time->scale == 0 || !output)
    return 4;
  const auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(instance.installed_key, value->param_index);
  if (!parameter) return 4;
  output->stream = stream;
  output->value.fill(std::byte{});
  if (value->param_index == 0) {
    std::memcpy(output->value.data(), &instance.layer, sizeof(instance.layer));
  } else {
    std::memcpy(output->value.data(),
                instance.parameter_values[static_cast<std::size_t>(value->param_index - 1)].data(),
                sizeof(instance.parameter_values[0]));
  }
  value->value_live = true;
  value->checked_out_value = output;
  ++g_aegp_stream_value_acquires;
  return 0;
}
int32_t __cdecl aegp_dispose_stream_value_v2(AegpStreamValue* output) {
  if (!output) return 4;
  auto* stream = legacy_effect_stream(output->stream);
  if (!stream || !stream->value_live || stream->checked_out_value != output) return 4;
  output->stream = nullptr;
  stream->value_live = false;
  stream->checked_out_value = nullptr;
  ++g_aegp_stream_value_disposes;
  return 0;
}
int32_t __cdecl aegp_set_stream_value_v2(
    int32_t plugin_id, void* stream, AegpStreamValue* input) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) ||
      plugin_id != value->owner_plugin_id || !value->value_live || !input ||
      input->stream != stream || value->checked_out_value != input)
    return 4;
  const auto& current = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(current.installed_key, value->param_index);
  if (!parameter || value->param_index == 0 || !parameter->writable) return 4;
  std::array<double, 4> candidate{};
  std::memcpy(candidate.data(), input->value.data(), sizeof(candidate));
  for (std::size_t index = 0; index < candidate.size(); ++index)
    if (!std::isfinite(candidate[static_cast<std::size_t>(index)])) return 4;
  auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  instance.parameter_values[static_cast<std::size_t>(value->param_index - 1)] = candidate;
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_dispose_stream_v2(void* stream) {
  auto* value = legacy_effect_stream(stream);
  if (!value || value->value_live) return 4;
  value->live = false;
  value->param_index = -1;
  value->effect_instance_index = 0;
  value->effect_instance_generation = 0;
  value->checked_out_value = nullptr;
  value->owner_plugin_id = 0;
  ++g_aegp_stream_disposes;
  return 0;
}
int32_t __cdecl aegp_set_dynamic_stream_flag_v2(
    void* stream, uint32_t one_flag, uint8_t undoable, uint8_t set) {
  auto* value = legacy_effect_stream(stream);
  constexpr uint32_t kHidden = 1u << 1;
  if (!value || !legacy_effect_stream_parent_live(*value) ||
      one_flag != kHidden || undoable > 1 || set > 1) return 4;
  value->hidden = set != 0;
  return 0;
}
int32_t __cdecl aegp_get_effect_param_union_by_index_v3(
    int32_t plugin_id, void* effect, int32_t index, int32_t* type, void* param_union) {
  if (plugin_id <= 0 || !resolve_effect_instance(effect, plugin_id) || !type ||
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
