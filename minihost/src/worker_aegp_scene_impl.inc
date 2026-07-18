// Copyright (c) AEXCompat contributors.
// Private implementation fragment for the AEGP scene family. Included by
// l2_main.cpp after the worker ABI/state prelude so callback addresses retain
// their exact clean-room __cdecl ABI and bounded worker-state ownership.

#include "worker_aegp_scene.hpp"

// State and catalog records live in worker_aegp_scene_runtime.cpp.  This
// callback fragment intentionally receives aliases below so stale-handle
// validation keeps operating on the one shared scene instance.
bool& g_aegp_effect_live = scene_runtime_state().effect_live;
auto& g_aegp_effect_instances = scene_runtime_state().effect_instances;
auto& g_aegp_effect_leases = scene_runtime_state().effect_leases;
uint32_t& g_aegp_effect_lease_generation = scene_runtime_state().effect_lease_generation;

void* effect_lease_handle(std::size_t slot, uint32_t generation) {
  const uintptr_t value = (static_cast<uintptr_t>(generation) << 8) |
      (static_cast<uintptr_t>(slot) << 2) | 1;
  return value > 1 ? reinterpret_cast<void*>(value) : nullptr;
}
const AegpEffectInstance* resolve_effect_instance(void* effect, int32_t owner = 0,
                                                  std::size_t* index = nullptr) {
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
const AegpInstalledEffectRecord* find_installed_effect(int32_t key);
const AegpEffectParameterRecord* find_effect_parameter(int32_t key, int32_t index);
void initialize_effect_parameter_values(AegpEffectInstance& instance);
auto& g_aegp_transform_stream = scene_runtime_state().transform_stream;
auto& g_aegp_legacy_effect_streams = scene_runtime_state().legacy_effect_streams;
uint32_t& g_aegp_legacy_effect_stream_generation =
    scene_runtime_state().legacy_effect_stream_generation;

int32_t __cdecl aegp_get_active_item(void** item) {
  if (!item) return 4;
  *item = (g_aegp_update_menu_mode || g_aegp_command_roundtrip_mode ||
           g_aegp_comp_idle_roundtrip_mode)
      ? &g_aegp_comp_item : nullptr;
  return 0;
}
int32_t __cdecl aegp_get_item_type(void* item, int16_t* item_type);
struct AegpItemSuite {
  void* before_get_active_item[2]{};
  decltype(&aegp_get_active_item) get_active_item;
  void* before_get_item_type[2]{};
  decltype(&aegp_get_item_type) get_item_type;
  void* after_get_item_type[20]{};
};
static_assert(offsetof(AegpItemSuite, get_active_item) == 16);
static_assert(offsetof(AegpItemSuite, get_item_type) == 40);
static_assert(sizeof(AegpItemSuite) == 208);
AegpItemSuite g_aegp_item_suite{};

int32_t __cdecl aegp_get_item_type(void* item, int16_t* item_type) {
  if (item != &g_aegp_comp_item || !item_type) return 4;
  ++g_aegp_item_type_calls;
  *item_type = 2;  // AEGP_ItemType_COMP in AE_GeneralPlug.h.
  return 0;
}
struct AegpLegacyItemSuite6 {
  void* before_get_active_item[2]{};
  decltype(&aegp_get_active_item) get_active_item;
  void* before_get_item_type[2]{};
  decltype(&aegp_get_item_type) get_item_type;
  void* unsupported[20]{};
};
static_assert(offsetof(AegpLegacyItemSuite6, get_active_item) == 16);
static_assert(offsetof(AegpLegacyItemSuite6, get_item_type) == 40);
static_assert(sizeof(AegpLegacyItemSuite6) == 208);
AegpLegacyItemSuite6 g_aegp_legacy_item_suite6{};

std::array<AegpTime, 3> g_aegp_layer_in_points{{{0, 30}, {0, 30}, {0, 30}}};
std::array<AegpTime, 3> g_aegp_layer_durations{{{300, 30}, {300, 30}, {300, 30}}};
int32_t __cdecl aegp_unsupported_suite_call() { return 4; }

int32_t __cdecl aegp_get_item_current_time(void* item, AegpTime* time) {
  if (item != &g_aegp_comp_item || !time) return 4;
  ++g_aegp_item_current_time_calls;
  if (g_aegp_first_observed_frame < 0) g_aegp_first_observed_frame = g_aegp_scene_frame;
  g_aegp_last_observed_frame = g_aegp_scene_frame;
  *time = {g_aegp_scene_frame, 30};
  return 0;
}
int32_t __cdecl aegp_set_item_current_time(void* item, const AegpTime* time) {
  if (item != &g_aegp_comp_item || !time || time->scale == 0 || time->value < 0 ||
      time->value > 300) return 4;
  ++g_aegp_item_set_current_time_calls;
  g_aegp_item_last_set_time_value = time->value;
  g_aegp_item_last_set_time_scale = time->scale;
  g_aegp_scene_frame = static_cast<int32_t>(
      (static_cast<int64_t>(time->value) * 30) / time->scale);
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_get_item_id(void* item, int32_t* id) {
  if (item != &g_aegp_comp_item || !id) return 4;
  *id = 1001;
  return 0;
}
int32_t __cdecl aegp_get_item_name(int32_t plugin_id, void* item, void** name) {
  if (plugin_id != 1 || item != &g_aegp_comp_item || !name) return 4;
  if (make_utf16_handle(u"AEXCompat Composition", "item name", name) != 0) return 4;
  ++g_aegp_item_name_calls;
  return 0;
}
int32_t __cdecl aegp_get_item_duration(void* item, AegpTime* duration) {
  if (item != &g_aegp_comp_item || !duration) return 4;
  *duration = {300, 30};
  ++g_aegp_item_duration_calls;
  return 0;
}
int32_t __cdecl aegp_get_comp_from_item(void* item, void** comp) {
  if (item != &g_aegp_comp_item || !comp) return 4;
  ++g_aegp_comp_from_item_calls;
  *comp = &g_aegp_comp;
  return 0;
}
int32_t __cdecl aegp_get_comp_framerate(void* comp, double* fps) {
  if (comp != &g_aegp_comp || !fps) return 4;
  ++g_aegp_comp_framerate_calls;
  *fps = 30.0;
  return 0;
}
int32_t __cdecl aegp_get_comp_frame_duration(void* comp, AegpTime* duration) {
  if (comp != &g_aegp_comp || !duration) return 4;
  *duration = {1, 30};
  return 0;
}
int32_t __cdecl aegp_get_comp_num_layers(void* comp, int32_t* count) {
  if (comp != &g_aegp_comp || !count) return 4;
  ++g_aegp_layer_count_calls;
  *count = static_cast<int32_t>(g_aegp_layers.size());
  return 0;
}
int32_t __cdecl aegp_get_comp_layer_by_index(void* comp, int32_t index, void** layer) {
  if (comp != &g_aegp_comp || index < 0 ||
      static_cast<std::size_t>(index) >= g_aegp_layers.size() || !layer) return 4;
  ++g_aegp_layer_by_index_calls;
  *layer = &g_aegp_layers[static_cast<std::size_t>(index)];
  return 0;
}
int32_t aegp_layer_index(void* layer) {
  if (layer == &g_layer) return 0;
  for (std::size_t index = 0; index < g_aegp_layers.size(); ++index)
    if (layer == &g_aegp_layers[index]) return static_cast<int32_t>(index);
  return -1;
}
int32_t __cdecl aegp_get_layer_to_world_xform(
    void* layer, const AegpTime* comp_time, AegpMatrix4* transform) {
  if (aegp_layer_index(layer) < 0 || !comp_time || !transform ||
      !valid_comp_time(*comp_time)) return 4;
  AegpMatrix4 result{};
  for (std::size_t index = 0; index < 4; ++index) result.mat[index][index] = 1.0;
  *transform = result;
  return 0;
}
int32_t __cdecl aegp_get_item_from_comp(void* comp, void** item) {
  if (comp != &g_aegp_comp || !item) return 4;
  *item = &g_aegp_comp_item;
  return 0;
}
int32_t __cdecl aegp_get_item_dimensions(
    void* item, int32_t* width, int32_t* height) {
  if (item != &g_aegp_comp_item || !width || !height) return 4;
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

union AegpLegacyStreamVal { double one_d; };
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
  value->one_d = static_cast<double>(width);
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
  const int32_t found = aegp_layer_index(layer);
  if (found < 0 || !index) return 4;
  *index = found;
  return 0;
}
int32_t __cdecl aegp_get_layer_source_item(void* layer, void** item) {
  if (aegp_layer_index(layer) < 0 || !item) return 4;
  *item = aegp_comp_item_handle();
  ++g_aegp_layer_source_item_calls;
  return 0;
}
int32_t __cdecl aegp_get_layer_parent_comp(void* layer, void** comp) {
  if (aegp_layer_index(layer) < 0 || !comp) return 4;
  *comp = &g_aegp_comp;
  return 0;
}
int32_t __cdecl aegp_get_layer_name(
    int32_t plugin_id, void* layer, void** layer_name, void** source_name) {
  const int32_t index = aegp_layer_index(layer);
  if (plugin_id != 1 || index < 0 || !layer_name || !source_name) return 4;
  *layer_name = nullptr;
  *source_name = nullptr;
  const std::u16string suffix(1, static_cast<char16_t>(u'1' + index));
  if (make_utf16_handle(u"Layer " + suffix, "layer name", layer_name) != 0) return 4;
  if (make_utf16_handle(u"Source " + suffix, "source name", source_name) != 0) {
    free_aegp_mem_handle(*layer_name);
    *layer_name = nullptr;
    return 4;
  }
  ++g_aegp_layer_name_calls;
  return 0;
}
int32_t __cdecl aegp_get_layer_parent(void* layer, void** parent) {
  if (aegp_layer_index(layer) < 0 || !parent) return 4;
  *parent = nullptr;
  return 0;
}
int32_t __cdecl aegp_get_layer_from_id(void* comp, int32_t id, void** layer) {
  const int32_t index = id - 2001;
  if (comp != &g_aegp_comp || index < 0 ||
      static_cast<std::size_t>(index) >= g_aegp_layers.size() || !layer) return 4;
  *layer = &g_aegp_layers[static_cast<std::size_t>(index)];
  return 0;
}
int32_t __cdecl aegp_get_comp_selection(
    int32_t plugin_id, void* comp, void** collection) {
  if (plugin_id <= 0 || comp != &g_aegp_comp || !collection || g_aegp_selection.live)
    return 4;
  g_aegp_selection.live = true;
  ++g_aegp_collection_creates;
  *collection = &g_aegp_selection;
  return 0;
}
struct AegpCollectionItem {
  int32_t type{};
  int32_t padding{};
  std::array<std::byte, 40> item{};
  void* stream{};
};
static_assert(sizeof(AegpCollectionItem) == 56);
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
  *item = {};
  item->type = 1;
  void* layer = &g_aegp_layers[index];
  std::memcpy(item->item.data(), &layer, sizeof(layer));
  ++g_aegp_collection_item_reads;
  return 0;
}
struct AegpCollectionSuite {
  void* new_collection{};
  decltype(&aegp_dispose_collection) dispose_collection;
  decltype(&aegp_get_collection_count) get_count;
  decltype(&aegp_get_collection_item) get_by_index;
  void* push_back{};
  void* erase{};
};
AegpCollectionSuite g_aegp_collection_suite{
    reinterpret_cast<void*>(&aegp_unsupported_suite_call),
    &aegp_dispose_collection, &aegp_get_collection_count,
    &aegp_get_collection_item,
    reinterpret_cast<void*>(&aegp_unsupported_suite_call),
    reinterpret_cast<void*>(&aegp_unsupported_suite_call)};
static_assert(sizeof(AegpCollectionSuite) == 48);
int32_t __cdecl aegp_get_layer_id(void* layer, int32_t* id) {
  const int32_t index = aegp_layer_index(layer);
  if (index < 0 || !id) return 4;
  ++g_aegp_layer_id_calls;
  *id = 2001 + index;
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
struct AegpLayerTransferMode { int32_t mode{}; int32_t flags{}; int32_t track_matte{}; };
static_assert(sizeof(AegpLayerTransferMode) == 12);
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
std::array<void*, 22> g_aegp_effect_suite4{};
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
static_assert(sizeof(g_aegp_effect_suite3) == 136);
static_assert(sizeof(g_aegp_stream_suite2) == 176);
static_assert(sizeof(g_aegp_stream_suite6) == 184);
static_assert(sizeof(g_aegp_keyframe_suite5) == 176);

