#include <windows.h>

#include "worker_mask_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_aegp_external_render_runtime.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_scene_model.hpp"
#include "worker_aegp_scene_transaction.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_pf_path_runtime.hpp"
#include "worker_world_registry.hpp"
#include "worker_world_safety.hpp"

#include <algorithm>
#include <array>
#include <cstring>
#include <limits>
#include <new>
#include <utility>

namespace aexcompat::l2_detail {

using aexcompat::worker_runtime::handles::make_utf16_handle;

std::vector<HostMask> g_mask_scene;
std::list<HostStreamRef> g_stream_refs;
std::unordered_map<StreamValue*, CheckedStreamValue> g_stream_values;
MaskLifetimeCounts g_mask_lifetime;
uint32_t g_invalid_outline_operations{}, g_outline_mutations{}, g_mask_mutations{},
    g_invalid_mask_operations{}, g_invalid_stream_operations{}, g_stream_metadata_queries{},
    g_stream_duplicates{}, g_keyframe_mutations{}, g_invalid_keyframe_operations{},
    g_dynamic_stream_queries{}, g_dynamic_stream_mutations{},
    g_invalid_dynamic_stream_operations{}, g_layer_dynamic_flags{},
    g_mask_parade_dynamic_flags{};
int32_t g_next_mask_id{1}, g_next_stream_id{1};
int32_t g_keyframe_apply_failure_after{-1};
std::list<AddKeyframesTransaction> g_add_keyframe_transactions;

bool is_primary_mask_layer(void* layer) {
  return layer == aexcompat::mask_runtime::host_context().layer ||
      aegp_layer_index(layer) == 0;
}

int32_t __cdecl get_layer_num_masks(void* layer, int32_t* count) {
  const auto host = aexcompat::mask_runtime::host_context();
  if (!is_primary_mask_layer(layer) || !count) return 4;
  if (aexcompat::mask_runtime::fault() == aexcompat::mask_runtime::Fault::CountCrash) {
    if (host.raise_access_violation) host.raise_access_violation();
    return 4;
  }
  if (aexcompat::mask_runtime::fault() == aexcompat::mask_runtime::Fault::CountError) return 4;
  *count = static_cast<int32_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& mask) { return !mask.deleted; }));
  return 0;
}

int32_t __cdecl get_layer_mask_by_index(void* layer, int32_t index, void** mask) {
  if (!is_primary_mask_layer(layer) || index < 0 || !mask) return 4;
  const auto masks = ordered_active_masks();
  if (static_cast<std::size_t>(index) >= masks.size()) return 4;
  auto& record = *masks[static_cast<std::size_t>(index)];
  if (record.mask_live) return 4;
  record.mask_live = true;
  ++g_mask_lifetime.masks_acquired;
  *mask = &record.mask;
  return 0;
}

int32_t __cdecl dispose_mask(void* mask) {
  HostMask* record = find_mask(mask);
  if (!record || !record->mask_live) return 4;
  record->mask_live = false;
  ++g_mask_lifetime.masks_disposed;
  return 0;
}

bool usable_mask(const HostMask* mask) { return mask && mask->mask_live && !mask->deleted; }

int32_t __cdecl get_mask_invert(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->invert; return 0;
}
int32_t __cdecl set_mask_invert(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask)) return 4;
  mask->invert = value != 0; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_mode(void* handle, int32_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->mode; return 0;
}
int32_t __cdecl set_mask_mode(void* handle, int32_t value) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || value < 0 || value > 7) { ++g_invalid_mask_operations; return 4; }
  mask->mode = value; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_motion_blur(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->motion_blur; return 0;
}
int32_t __cdecl set_mask_motion_blur(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || value > 2) { ++g_invalid_mask_operations; return 4; }
  mask->motion_blur = value; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_feather_falloff(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->feather_falloff; return 0;
}
int32_t __cdecl set_mask_feather_falloff(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || value > 1) { ++g_invalid_mask_operations; return 4; }
  mask->feather_falloff = value; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_id(void* handle, int32_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->id; return 0;
}
int32_t __cdecl create_new_mask(void* layer, void** handle, int32_t* index) {
  if (!is_primary_mask_layer(layer) || !handle ||
      g_mask_scene.size() >= kMaxHostMasks) {
    ++g_invalid_mask_operations; return 4;
  }
  HostMask mask; mask.id = g_next_mask_id++; mask.outline_stream_id = g_next_stream_id++;
  mask.feather_stream_id = g_next_stream_id++; mask.opacity_stream_id = g_next_stream_id++;
  mask.expansion_stream_id = g_next_stream_id++;
  mask.dynamic_order = static_cast<int32_t>(active_mask_count());
  mask.mask_live = true;
  g_mask_scene.push_back(mask);
  ++g_mask_lifetime.masks_acquired; ++g_mask_mutations;
  *handle = &g_mask_scene.back().mask;
  if (index) *index = static_cast<int32_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& item) { return !item.deleted; }) - 1);
  return 0;
}
int32_t __cdecl delete_mask_from_layer(void* handle) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || mask->stream_live || mask->value_live) {
    ++g_invalid_mask_operations; return 4;
  }
  const int32_t deleted_order = mask->dynamic_order;
  mask->deleted = true;
  for (auto& candidate : g_mask_scene)
    if (!candidate.deleted && candidate.dynamic_order > deleted_order) --candidate.dynamic_order;
  ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_color(void* handle, double* color) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !color) return 4;
  std::copy(mask->color.begin(), mask->color.end(), color); return 0;
}
int32_t __cdecl set_mask_color(void* handle, const double* color) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || !color || !std::all_of(color, color + 4,
      [](double value) { return std::isfinite(value) && value >= 0 && value <= 1; })) {
    ++g_invalid_mask_operations; return 4;
  }
  std::copy(color, color + 4, mask->color.begin()); ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_lock(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->locked; return 0;
}
int32_t __cdecl set_mask_lock(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask)) return 4;
  mask->locked = value != 0; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_roto_bezier(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->roto_bezier; return 0;
}
int32_t __cdecl set_mask_roto_bezier(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask)) return 4;
  mask->roto_bezier = value != 0; ++g_mask_mutations; return 0;
}
int32_t __cdecl duplicate_mask(void* original_handle, void** duplicate_handle) {
  HostMask* original = find_mask(original_handle);
  if (!usable_mask(original) || !duplicate_handle || g_mask_scene.size() >= kMaxHostMasks) {
    ++g_invalid_mask_operations; return 4;
  }
  HostMask copy = *original;
  copy.mask_live = true; copy.stream_live = false; copy.value_live = false;
  copy.deleted = false; copy.id = g_next_mask_id++;
  copy.outline_stream_id = g_next_stream_id++;
  copy.feather_stream_id = g_next_stream_id++;
  copy.opacity_stream_id = g_next_stream_id++;
  copy.expansion_stream_id = g_next_stream_id++;
  copy.dynamic_order = static_cast<int32_t>(active_mask_count());
  g_mask_scene.push_back(std::move(copy));
  ++g_mask_lifetime.masks_acquired; ++g_mask_mutations;
  *duplicate_handle = &g_mask_scene.back().mask;
  return 0;
}

int32_t stream_identity(const HostMask* mask, DynamicNodeKind kind) {
  if (!mask) return kind == DynamicNodeKind::LayerRoot ? 0x70000001 : 0x70000002;
  switch (kind) {
    case DynamicNodeKind::MaskOutline: return mask->outline_stream_id;
    case DynamicNodeKind::MaskFeather: return mask->feather_stream_id;
    case DynamicNodeKind::MaskOpacity: return mask->opacity_stream_id;
    case DynamicNodeKind::MaskExpansion: return mask->expansion_stream_id;
    case DynamicNodeKind::MaskAtom: return 0x10000000 + mask->id;
    default: return 0;
  }
}
int32_t create_stream_ref(HostMask* mask, DynamicNodeKind kind, int32_t selector, void** stream) {
  if (!stream || g_stream_refs.size() >= 64) return 4;
  g_stream_refs.push_back({{}, mask, selector, stream_identity(mask, kind), 0, kind});
  auto& record = g_stream_refs.back();
  aexcompat::scene_model::Identity owner{};
  auto& registry = aexcompat::scene_model::registry();
  if (!registry.identity_for_object(
          1, 2001, aexcompat::scene_model::ObjectKind::layer, owner) ||
      !registry.create_child_borrowed(
          aexcompat::scene_model::ObjectKind::stream, owner,
          record.unique_id, &record, u"Mask Stream", 1,
          record.identity, record.handle)) {
    g_stream_refs.pop_back();
    return 4;
  }
  aexcompat::scene_model::StreamState stream_state{};
  using aexcompat::scene_model::StreamValueKind;
  stream_state.value_kind =
      kind == DynamicNodeKind::MaskOutline ? StreamValueKind::mask :
      (kind == DynamicNodeKind::LayerRoot ||
       kind == DynamicNodeKind::MaskParade ||
       kind == DynamicNodeKind::MaskAtom
           ? StreamValueKind::arbitrary : StreamValueKind::scalar);
  stream_state.dimensions =
      kind == DynamicNodeKind::MaskFeather ? 2 : 1;
  stream_state.temporal_dimensions = 1;
  if (!registry.initialize_stream_state(record.identity, stream_state)) {
    registry.erase_tree(record.identity);
    g_stream_refs.pop_back();
    return 4;
  }
  if (mask) mask->stream_live = true;
  ++g_mask_lifetime.streams_acquired;
  *stream = record.handle;
  return 0;
}
int32_t __cdecl get_new_mask_stream(int32_t plugin_id, void* mask, int32_t selector, void** stream) {
  HostMask* record = find_mask(mask);
  DynamicNodeKind kind{};
  if (selector == 400) kind = DynamicNodeKind::MaskOutline;
  else if (selector == 401) kind = DynamicNodeKind::MaskOpacity;
  else if (selector == 402) kind = DynamicNodeKind::MaskFeather;
  else if (selector == 403) kind = DynamicNodeKind::MaskExpansion;
  else kind = DynamicNodeKind::LayerRoot;
  if (plugin_id != 1 || !usable_mask(record) || selector < 400 || selector > 403 || !stream) {
    ++g_invalid_stream_operations;
    if (stream) *stream = nullptr;
    return 4;
  }
  const int32_t error = create_stream_ref(record, kind, selector, stream);
  if (error) ++g_invalid_stream_operations;
  return error;
}

int32_t __cdecl dispose_stream(void* stream) {
  HostStreamRef* record = find_stream(stream);
  if (!record || record->live_values != 0 ||
      std::any_of(g_add_keyframe_transactions.begin(), g_add_keyframe_transactions.end(),
          [record](const auto& transaction) { return transaction.stream == record; })) {
    ++g_invalid_stream_operations; return 4;
  }
  HostMask* mask = record->mask;
  if (!aexcompat::scene_model::registry().erase_tree(record->identity)) {
    ++g_invalid_stream_operations;
    return 4;
  }
  g_stream_refs.erase(std::find_if(g_stream_refs.begin(), g_stream_refs.end(),
      [record](auto& candidate) { return &candidate == record; }));
  if (mask) mask->stream_live = std::any_of(g_stream_refs.begin(), g_stream_refs.end(),
      [mask](const auto& candidate) { return candidate.mask == mask; });
  ++g_mask_lifetime.streams_disposed;
  return 0;
}

int32_t __cdecl get_new_stream_value(int32_t plugin_id, void* stream, int32_t,
                                     const HostTime* time, int32_t, StreamValue* value) {
  HostStreamRef* record = find_stream(stream);
  if (plugin_id != 1 || !record || !value ||
      g_stream_values.size() >= kMaxCheckedStreamValues ||
      g_stream_values.find(value) != g_stream_values.end()) {
    ++g_invalid_stream_operations; return 4;
  }
  std::unique_ptr<OutlineData> owned;
  OutlineData* outline = nullptr;
  if (record->kind == DynamicNodeKind::MaskOutline) {
    outline = sampled_outline(record, time, owned);
    if (!outline) { ++g_invalid_stream_operations; return 4; }
  } else if (!dynamic_leaf(record->kind)) { ++g_invalid_stream_operations; return 4; }
  aexcompat::scene_model::Identity value_identity{};
  if (!aexcompat::scene_model::registry().create_child(
          aexcompat::scene_model::ObjectKind::value, record->identity, 0,
          value, u"Stream Value", value_identity)) {
    ++g_invalid_stream_operations;
    return 4;
  }
  ++record->live_values;
  record->mask->value_live = true;
  g_stream_values.emplace(value, CheckedStreamValue{
      record, outline, nullptr, std::move(owned), value_identity});
  ++g_mask_lifetime.values_acquired;
  value->stream = record->handle;
  std::memset(value->raw_value, 0, sizeof(value->raw_value));
  if (outline) value->value = &outline->outline;
  else if (record->kind == DynamicNodeKind::MaskOpacity) value->one_d = record->mask->opacity;
  else if (record->kind == DynamicNodeKind::MaskExpansion) value->one_d = record->mask->expansion;
  else { value->two_d[0] = record->mask->feather[0]; value->two_d[1] = record->mask->feather[1]; }
  return 0;
}

int32_t __cdecl dispose_stream_value(StreamValue* value) {
  if (!value) return 4;
  const auto owned = g_stream_values.find(value);
  HostStreamRef* stream_record = find_stream(value->stream);
  OutlineData* outline_record = owned != g_stream_values.end() && owned->second.outline
      ? find_outline(value->value) : nullptr;
  if (owned == g_stream_values.end() || !stream_record || owned->second.stream != stream_record ||
      (owned->second.outline && owned->second.outline != outline_record) ||
      stream_record->live_values == 0 ||
      !aexcompat::scene_model::registry().erase_tree(
          owned->second.identity)) {
    ++g_invalid_stream_operations; return 4;
  }
  --stream_record->live_values;
  g_stream_values.erase(owned);
  stream_record->mask->value_live = std::any_of(g_stream_refs.begin(), g_stream_refs.end(),
      [mask = stream_record->mask](const auto& candidate) {
        return candidate.mask == mask && candidate.live_values != 0;
      });
  ++g_mask_lifetime.values_disposed;
  value->stream = nullptr;
  value->value = nullptr;
  return 0;
}

int32_t __cdecl is_stream_legal(void* layer, int32_t, uint8_t* legal) {
  if (layer != aexcompat::mask_runtime::host_context().layer || !legal) return 4;
  *legal = 0;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl can_vary_over_time(void* stream, uint8_t* can_vary) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !can_vary || !dynamic_leaf(record->kind)) return 4;
  *can_vary = 1;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl get_valid_interpolations(void* stream, int32_t* interpolations) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !interpolations || !dynamic_leaf(record->kind)) return 4;
  *interpolations = 0xffff;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl unsupported_new_layer_stream(int32_t, void*, int32_t, void** stream) {
  if (stream) *stream = nullptr;
  ++g_invalid_stream_operations;
  return 4;
}
int32_t __cdecl unsupported_effect_stream_count(void*, int32_t* count) {
  if (count) *count = 0;
  ++g_invalid_stream_operations;
  return 4;
}
int32_t __cdecl unsupported_new_effect_stream(int32_t, void*, int32_t, void** stream) {
  if (stream) *stream = nullptr;
  ++g_invalid_stream_operations;
  return 4;
}
std::u16string stream_display_name(const HostStreamRef& stream) {
  switch (stream.kind) {
    case DynamicNodeKind::LayerRoot: return u"Layer";
    case DynamicNodeKind::MaskParade: return u"Masks";
    case DynamicNodeKind::MaskAtom: return stream.mask ? stream.mask->dynamic_name : u"Mask";
    case DynamicNodeKind::MaskOutline: return u"Mask Path";
    case DynamicNodeKind::MaskFeather: return u"Mask Feather";
    case DynamicNodeKind::MaskOpacity: return u"Mask Opacity";
    case DynamicNodeKind::MaskExpansion: return u"Mask Expansion";
  }
  return {};
}
int32_t __cdecl unsupported_stream_name(int32_t plugin_id, void* stream, uint8_t, void** name) {
  if (name) *name = nullptr;
  HostStreamRef* record = find_stream(stream);
  if (plugin_id != 1 || !record || !name) return 4;
  return make_utf16_handle(stream_display_name(*record), "stream name", name);
}
int32_t __cdecl get_stream_units_text(void* stream, uint8_t, char* units) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !units || !dynamic_leaf(record->kind)) return 4;
  const char* text = record->kind == DynamicNodeKind::MaskOpacity ? "%" :
      record->kind == DynamicNodeKind::MaskOutline ? "" : "pixels";
  strcpy_s(units, 32, text);
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl get_stream_properties(void* stream, int32_t* flags, double* minimum,
                                      double* maximum) {
  if (!find_stream(stream) || !flags) return 4;
  HostStreamRef* record = find_stream(stream);
  if (!record || !dynamic_leaf(record->kind)) return 4;
  *flags = record->kind == DynamicNodeKind::MaskOpacity ? 3 : 0;
  if (minimum) *minimum = 0;
  if (maximum) *maximum = record->kind == DynamicNodeKind::MaskOpacity ? 100 : 0;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl is_stream_timevarying(void* stream, uint8_t* timevarying) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !timevarying || !dynamic_leaf(record->kind)) return 4;
  *timevarying = record->kind == DynamicNodeKind::MaskOutline &&
      !record->mask->keyframes.empty() ? 1 : 0;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl get_stream_type(void* stream, int32_t* type) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !type) return 4;
  *type = record->kind == DynamicNodeKind::MaskOutline ? 11 :
      record->kind == DynamicNodeKind::MaskFeather ? 4 :
      (record->kind == DynamicNodeKind::MaskOpacity ||
       record->kind == DynamicNodeKind::MaskExpansion) ? 5 : 0;
  ++g_stream_metadata_queries;
  return 0;
}
bool valid_outline_snapshot(const OutlineData& outline) {
  if (outline.vertices.size() > kMaxOutlineVertices + 1 ||
      outline.feathers.size() > kMaxOutlineFeathers) return false;
  for (const MaskVertex& vertex : outline.vertices) {
    if (!std::isfinite(vertex.x) || !std::isfinite(vertex.y) ||
        !std::isfinite(vertex.tangent_in_x) || !std::isfinite(vertex.tangent_in_y) ||
        !std::isfinite(vertex.tangent_out_x) || !std::isfinite(vertex.tangent_out_y)) return false;
  }
  if (!outline.open && !outline.vertices.empty()) {
    if (outline.vertices.size() - 1 > kMaxOutlineVertices ||
        std::memcmp(&outline.vertices.front(), &outline.vertices.back(),
                    sizeof(MaskVertex)) != 0) return false;
  } else if (outline.vertices.size() > kMaxOutlineVertices) return false;
  const std::size_t distinct = outline.vertices.size() -
      static_cast<std::size_t>(!outline.open && !outline.vertices.empty());
  const std::size_t segments = distinct == 0 ? 0 :
      distinct - static_cast<std::size_t>(outline.open);
  return std::all_of(outline.feathers.begin(), outline.feathers.end(),
      [segments](const MaskFeather& feather) {
        return feather.segment >= 0 && static_cast<std::size_t>(feather.segment) < segments &&
            std::isfinite(feather.segment_s) && feather.segment_s >= 0 && feather.segment_s <= 1 &&
            std::isfinite(feather.radius) && std::isfinite(feather.ui_corner_angle) &&
            feather.ui_corner_angle >= 0 && feather.ui_corner_angle <= 1 &&
            std::isfinite(feather.tension) && feather.tension >= 0 && feather.tension <= 1 &&
            feather.interp <= 1 && feather.type <= 1 && (feather.type == 1 || feather.radius >= 0);
      });
}

int32_t __cdecl set_stream_value(int32_t plugin_id, void* stream, StreamValue* value) {
  HostStreamRef* record = find_stream(stream);
  const auto checked = value ? g_stream_values.find(value) : g_stream_values.end();
  if (plugin_id != 1 || !record || !value || value->stream != record->handle ||
      !usable_mask(record->mask) || !dynamic_leaf(record->kind) ||
      checked == g_stream_values.end() || checked->second.stream != record ||
      !record->mask->keyframes.empty()) {
    ++g_invalid_stream_operations; return 4;
  }
  HostMask candidate = *record->mask;
  if (record->kind == DynamicNodeKind::MaskOpacity) {
    if (!std::isfinite(value->one_d) || value->one_d < 0 || value->one_d > 100) {
      ++g_invalid_stream_operations; return 4;
    }
    candidate.opacity = value->one_d;
  } else if (record->kind == DynamicNodeKind::MaskExpansion) {
    if (!std::isfinite(value->one_d)) { ++g_invalid_stream_operations; return 4; }
    candidate.expansion = value->one_d;
  } else if (record->kind == DynamicNodeKind::MaskFeather) {
    if (!std::isfinite(value->two_d[0]) || !std::isfinite(value->two_d[1])) {
      ++g_invalid_stream_operations; return 4;
    }
    candidate.feather = {value->two_d[0], value->two_d[1]};
  } else {
    OutlineData* outline_candidate = checked->second.outline;
    if (!outline_candidate || value->value != &outline_candidate->outline ||
        find_outline(value->value) != outline_candidate ||
        !valid_outline_snapshot(*outline_candidate)) {
      ++g_invalid_stream_operations; return 4;
    }
    static_cast<OutlineData&>(candidate) = *outline_candidate;
  }
  candidate.dynamic_modified = true;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      aexcompat::scene_model::registry(), record->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation);
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        *record->mask = std::move(candidate);
        ++g_dynamic_stream_mutations;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl unsupported_layer_stream_value(void*, int32_t, int32_t, const void*,
                                               uint8_t, void* value, int32_t* type) {
  if (value) std::memset(value, 0, sizeof(void*));
  if (type) *type = 0;
  ++g_invalid_stream_operations;
  return 4;
}
int32_t expression_index(DynamicNodeKind kind) {
  return kind == DynamicNodeKind::MaskOutline ? 0 : kind == DynamicNodeKind::MaskFeather ? 1 :
      kind == DynamicNodeKind::MaskOpacity ? 2 : kind == DynamicNodeKind::MaskExpansion ? 3 : -1;
}
int32_t __cdecl get_expression_state(int32_t plugin_id, void* stream, uint8_t* enabled) {
  HostStreamRef* record = find_stream(stream); const int32_t index = record ? expression_index(record->kind) : -1;
  if (plugin_id != 1 || !record || !record->mask || index < 0 || !enabled) return 4;
  *enabled = record->mask->expression_enabled[static_cast<std::size_t>(index)] ? 1 : 0;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl reject_expression_state(int32_t plugin_id, void* stream, uint8_t enabled) {
  HostStreamRef* record = find_stream(stream); const int32_t index = record ? expression_index(record->kind) : -1;
  if (plugin_id != 1 || !record || !record->mask || index < 0 ||
      (enabled && record->mask->expressions[static_cast<std::size_t>(index)].empty())) {
    ++g_invalid_stream_operations; return 4;
  }
  const bool candidate_enabled = enabled != 0;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      aexcompat::scene_model::registry(), record->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation);
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        record->mask->expression_enabled[static_cast<std::size_t>(index)] =
            candidate_enabled;
        record->mask->dynamic_modified = true;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl unsupported_get_expression(int32_t plugin_id, void* stream, void** expression) {
  if (expression) *expression = nullptr;
  HostStreamRef* record = find_stream(stream); const int32_t index = record ? expression_index(record->kind) : -1;
  if (plugin_id != 1 || !record || !record->mask || index < 0 || !expression) return 4;
  return make_utf16_handle(record->mask->expressions[static_cast<std::size_t>(index)],
                           "stream expression", expression);
}
int32_t __cdecl unsupported_set_expression(int32_t plugin_id, void* stream, const uint16_t* expression) {
  HostStreamRef* record = find_stream(stream); const int32_t index = record ? expression_index(record->kind) : -1;
  if (plugin_id != 1 || !record || !record->mask || index < 0 || !expression) return 4;
  std::size_t length = 0; while (length <= 4096 && expression[length]) ++length;
  if (length > 4096) { ++g_invalid_stream_operations; return 4; }
  std::u16string candidate(
      reinterpret_cast<const char16_t*>(expression), length);
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      aexcompat::scene_model::registry(), record->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation);
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        record->mask->expressions[static_cast<std::size_t>(index)] =
            std::move(candidate);
        record->mask->expression_enabled[static_cast<std::size_t>(index)] =
            !record->mask->expressions[
                static_cast<std::size_t>(index)].empty();
        record->mask->dynamic_modified = true;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl duplicate_stream_ref(int32_t plugin_id, void* stream, void** duplicate) {
  HostStreamRef* original = find_stream(stream);
  if (plugin_id != 1 || !original || !duplicate || g_stream_refs.size() >= 64) {
    ++g_invalid_stream_operations;
    return 4;
  }
  if (create_stream_ref(
          original->mask, original->kind, original->selector,
          duplicate) != 0) {
    ++g_invalid_stream_operations;
    return 4;
  }
  ++g_stream_duplicates;
  return 0;
}
int32_t __cdecl get_unique_stream_id(void* stream, int32_t* id) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !id) return 4;
  *id = record->unique_id;
  ++g_stream_metadata_queries;
  return 0;
}

bool valid_time(const HostTime* time) { return time && time->scale != 0; }
bool time_less(const HostTime& left, const HostTime& right) {
  return static_cast<int64_t>(left.value) * right.scale <
      static_cast<int64_t>(right.value) * left.scale;
}
bool time_equal(const HostTime& left, const HostTime& right) {
  return static_cast<int64_t>(left.value) * right.scale ==
      static_cast<int64_t>(right.value) * left.scale;
}
OutlineData* sampled_outline(HostStreamRef* stream, const HostTime* time,
                             std::unique_ptr<OutlineData>& owned) {
  if (!stream) return nullptr;
  auto& keys = stream->mask->keyframes;
  const auto snapshot = [&owned](const OutlineData& source) {
    owned = std::make_unique<OutlineData>(source);
    return owned.get();
  };
  if (keys.empty() || !time) return snapshot(*stream->mask);
  if (!valid_time(time)) return nullptr;
  auto upper = std::find_if(keys.begin(), keys.end(),
      [time](const auto& key) { return time_less(*time, key.time); });
  if (upper == keys.begin()) return snapshot(*upper);
  if (upper == keys.end()) return snapshot(keys.back());
  auto lower = std::prev(upper);
  if (time_equal(lower->time, *time) || lower->out_interpolation == 3)
    return snapshot(*lower);
  if (lower->open != upper->open || lower->vertices.size() != upper->vertices.size() ||
      lower->feathers.size() != upper->feathers.size()) return snapshot(*lower);
  const double lower_seconds = static_cast<double>(lower->time.value) / lower->time.scale;
  const double upper_seconds = static_cast<double>(upper->time.value) / upper->time.scale;
  const double requested_seconds = static_cast<double>(time->value) / time->scale;
  if (!(upper_seconds > lower_seconds)) return snapshot(*lower);
  const double amount = (requested_seconds - lower_seconds) / (upper_seconds - lower_seconds);
  owned = std::make_unique<OutlineData>(static_cast<const OutlineData&>(*lower));
  const auto blend = [amount](double left, double right) { return left + (right - left) * amount; };
  for (std::size_t index = 0; index < owned->vertices.size(); ++index) {
    const MaskVertex& left = lower->vertices[index]; const MaskVertex& right = upper->vertices[index];
    owned->vertices[index] = {blend(left.x, right.x), blend(left.y, right.y),
        blend(left.tangent_in_x, right.tangent_in_x),
        blend(left.tangent_in_y, right.tangent_in_y),
        blend(left.tangent_out_x, right.tangent_out_x),
        blend(left.tangent_out_y, right.tangent_out_y)};
  }
  for (std::size_t index = 0; index < owned->feathers.size(); ++index) {
    const MaskFeather& left = lower->feathers[index]; const MaskFeather& right = upper->feathers[index];
    if (left.segment != right.segment || left.interp != right.interp || left.type != right.type)
      return snapshot(*lower);
    owned->feathers[index].segment_s = blend(left.segment_s, right.segment_s);
    owned->feathers[index].radius = blend(left.radius, right.radius);
    owned->feathers[index].ui_corner_angle = static_cast<float>(
        blend(left.ui_corner_angle, right.ui_corner_angle));
    owned->feathers[index].tension = static_cast<float>(blend(left.tension, right.tension));
  }
  return owned.get();
}
HostKeyframe* keyframe_at(HostStreamRef* stream, int32_t index) {
  if (!stream || stream->kind != DynamicNodeKind::MaskOutline || index < 0 ||
      static_cast<std::size_t>(index) >= stream->mask->keyframes.size())
    return nullptr;
  auto item = stream->mask->keyframes.begin();
  std::advance(item, index);
  return &*item;
}

aexcompat::scene_model::KeyframeState keyframe_state(
    const HostKeyframe& key) {
  aexcompat::scene_model::KeyframeState state{};
  state.time_value = key.time.value;
  state.time_scale = key.time.scale;
  state.in_interpolation = key.in_interpolation;
  state.out_interpolation = key.out_interpolation;
  state.flags = static_cast<uint32_t>(key.flags);
  state.label = key.label;
  if (!key.spatial_in.vertices.empty()) {
    const auto& vertex = key.spatial_in.vertices.front();
    state.spatial_in = {
        vertex.x, vertex.y, vertex.tangent_in_x, vertex.tangent_in_y};
  }
  if (!key.spatial_out.vertices.empty()) {
    const auto& vertex = key.spatial_out.vertices.front();
    state.spatial_out = {
        vertex.x, vertex.y, vertex.tangent_out_x, vertex.tangent_out_y};
  }
  state.temporal_in[0] = {
      key.temporal_in[0].speed, key.temporal_in[0].influence};
  state.temporal_out[0] = {
      key.temporal_out[0].speed, key.temporal_out[0].influence};
  return state;
}

bool ensure_keyframe_identity(HostStreamRef* stream, HostKeyframe* key,
                              int32_t index) {
  if (!stream || !key || index < 0) return false;
  aexcompat::scene_model::ObjectSnapshot snapshot{};
  auto& registry = aexcompat::scene_model::registry();
  if (key->identity != aexcompat::scene_model::Identity{} &&
      registry.snapshot(key->identity, snapshot))
    return snapshot.owner == stream->identity;
  if (!registry.create_child(
          aexcompat::scene_model::ObjectKind::keyframe,
          stream->identity, index, key, u"Mask Keyframe",
          key->identity))
    return false;
  return registry.initialize_keyframe_state(
      key->identity, keyframe_state(*key));
}

bool commit_keyframe_candidate(HostStreamRef* stream, HostKeyframe* key,
                               HostKeyframe candidate) {
  if (!stream || !key) return false;
  aexcompat::scene_model::ObjectSnapshot identity{};
  auto& registry = aexcompat::scene_model::registry();
  if (!registry.snapshot(key->identity, identity)) return false;
  auto staged_identity = identity;
  staged_identity.keyframe = keyframe_state(candidate);
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      registry, stream->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation);
  if (!transaction.stage() || !transaction.validate(true)) return false;
  return transaction.commit(
      [&]() noexcept {
        aexcompat::scene_model::Identity replacement{};
        if (!registry.replace_snapshot(
                identity.identity, staged_identity, replacement))
          return false;
        candidate.identity = replacement;
        *key = std::move(candidate);
        ++g_keyframe_mutations;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); });
}

AddKeyframesTransaction* find_add_transaction(void* handle) {
  const auto found = std::find_if(g_add_keyframe_transactions.begin(),
      g_add_keyframe_transactions.end(), [handle](auto& item) { return handle == &item.opaque; });
  return found == g_add_keyframe_transactions.end() ? nullptr : &*found;
}
HostKeyframe snapshot_keyframe(const HostMask& mask, const HostTime& time) {
  HostKeyframe key;
  static_cast<OutlineData&>(key) = static_cast<const OutlineData&>(mask);
  key.spatial_in = static_cast<const OutlineData&>(mask);
  key.spatial_out = static_cast<const OutlineData&>(mask);
  key.time = time;
  return key;
}
int32_t __cdecl get_stream_num_keyframes(void* stream, int32_t* count) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !count) return 4;
  if (record->kind == DynamicNodeKind::MaskOutline) {
    int32_t index = 0;
    for (auto& key : record->mask->keyframes)
      if (!ensure_keyframe_identity(record, &key, index++)) return 4;
  }
  *count = record->kind == DynamicNodeKind::MaskOutline
      ? static_cast<int32_t>(record->mask->keyframes.size()) : 0;
  return 0;
}
int32_t __cdecl get_keyframe_time(void* stream, int32_t index, int16_t time_mode,
                                  HostTime* time) {
  HostKeyframe* key = keyframe_at(find_stream(stream), index);
  HostStreamRef* record = find_stream(stream);
  key = keyframe_at(record, index);
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      !time || time_mode < 0 || time_mode > 1)
    return 4;
  *time = key->time;
  return 0;
}
int32_t __cdecl insert_keyframe(void* stream, int16_t time_mode, const HostTime* time,
                                int32_t* index) {
  HostStreamRef* record = find_stream(stream);
  if (!record || time_mode < 0 || time_mode > 1 || !valid_time(time) || !index ||
      record->mask->keyframes.size() >= kMaxKeyframesPerStream) {
    ++g_invalid_keyframe_operations; return 4;
  }
  auto position = record->mask->keyframes.begin();
  int32_t found_index = 0;
  while (position != record->mask->keyframes.end() && time_less(position->time, *time)) {
    ++position; ++found_index;
  }
  if (position != record->mask->keyframes.end() && time_equal(position->time, *time)) {
    *index = found_index; return 0;
  }
  HostKeyframe candidate = snapshot_keyframe(*record->mask, *time);
  auto& registry = aexcompat::scene_model::registry();
  if (!registry.can_create_child(record->identity)) {
    ++g_invalid_keyframe_operations;
    return 4;
  }
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      registry, record->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation);
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  const bool committed = transaction.commit(
      [&]() noexcept {
        if (!registry.create_child(
                aexcompat::scene_model::ObjectKind::keyframe,
                record->identity, found_index, nullptr, u"Mask Keyframe",
                candidate.identity) ||
            !registry.initialize_keyframe_state(
                candidate.identity, keyframe_state(candidate)))
          return false;
        record->mask->keyframes.insert(position, std::move(candidate));
        ++g_keyframe_mutations;
        *index = found_index;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); });
  return committed ? 0 : 4;
}
void inject_keyframe_apply_failure_after(int32_t applied_count) noexcept {
  g_keyframe_apply_failure_after = applied_count > 0 ? applied_count : -1;
}
uint64_t mask_scene_fingerprint() noexcept {
  uint64_t hash = UINT64_C(14695981039346656037);
  const auto mix = [&hash](const auto& value) {
    const auto* bytes =
        reinterpret_cast<const unsigned char*>(&value);
    for (std::size_t index = 0; index < sizeof(value); ++index) {
      hash ^= bytes[index];
      hash *= UINT64_C(1099511628211);
    }
  };
  const auto mix_outline = [&](const OutlineData& outline) {
    mix(outline.open);
    const auto vertex_count = outline.vertices.size();
    const auto feather_count = outline.feathers.size();
    mix(vertex_count);
    mix(feather_count);
    for (const auto& vertex : outline.vertices) {
      mix(vertex.x);
      mix(vertex.y);
      mix(vertex.tangent_in_x);
      mix(vertex.tangent_in_y);
      mix(vertex.tangent_out_x);
      mix(vertex.tangent_out_y);
    }
    for (const auto& feather : outline.feathers) {
      mix(feather.segment);
      mix(feather.segment_s);
      mix(feather.radius);
      mix(feather.ui_corner_angle);
      mix(feather.tension);
      mix(feather.interp);
      mix(feather.type);
    }
  };
  const auto mask_count = g_mask_scene.size();
  mix(mask_count);
  for (const auto& mask : g_mask_scene) {
    mix_outline(mask);
    mix(mask.mask_live);
    mix(mask.stream_live);
    mix(mask.value_live);
    mix(mask.deleted);
    mix(mask.invert);
    mix(mask.locked);
    mix(mask.roto_bezier);
    mix(mask.motion_blur);
    mix(mask.feather_falloff);
    mix(mask.mode);
    mix(mask.id);
    mix(mask.opacity);
    for (const auto value : mask.feather) mix(value);
    mix(mask.expansion);
    for (const auto character : mask.dynamic_name) mix(character);
    for (const auto value : mask.dynamic_flags) mix(value);
    mix(mask.dynamic_modified);
    for (const auto& expression : mask.expressions)
      for (const auto character : expression) mix(character);
    for (const auto value : mask.expression_enabled) mix(value);
    for (const auto value : mask.color) mix(value);
    const auto key_count = mask.keyframes.size();
    mix(key_count);
    for (const auto& key : mask.keyframes) {
      mix_outline(key);
      mix(key.time.value);
      mix(key.time.scale);
      mix(key.flags);
      mix(key.in_interpolation);
      mix(key.out_interpolation);
      mix(key.label);
      mix_outline(key.spatial_in);
      mix_outline(key.spatial_out);
      for (const auto& ease : key.temporal_in) {
        mix(ease.speed);
        mix(ease.influence);
      }
      for (const auto& ease : key.temporal_out) {
        mix(ease.speed);
        mix(ease.influence);
      }
      mix(key.identity.project_id);
      mix(key.identity.object_id);
      mix(key.identity.generation);
      mix(key.identity.kind);
    }
  }
  return hash;
}
int32_t __cdecl delete_keyframe(void* stream, int32_t index) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      std::any_of(g_stream_values.begin(), g_stream_values.end(),
      [key](const auto& item) { return item.second.source_keyframe == key; })) {
    ++g_invalid_keyframe_operations; return 4;
  }
  auto position = record->mask->keyframes.begin(); std::advance(position, index);
  const auto identity = key->identity;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      aexcompat::scene_model::registry(), record->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation);
  if (!transaction.stage() || !transaction.validate(true)) return 4;
  return transaction.commit(
      [&]() noexcept {
        if (!aexcompat::scene_model::registry().erase_tree(identity))
          return false;
        record->mask->keyframes.erase(position);
        ++g_keyframe_mutations;
        return true;
      },
      []() noexcept { bump_render_project_timestamp(); }) ? 0 : 4;
}
int32_t __cdecl get_new_keyframe_value(int32_t plugin_id, void* stream, int32_t index,
                                       StreamValue* value) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (plugin_id != 1 || !record || !key ||
      !ensure_keyframe_identity(record, key, index) || !value ||
      g_stream_values.size() >= kMaxCheckedStreamValues ||
      g_stream_values.find(value) != g_stream_values.end()) {
    ++g_invalid_keyframe_operations; return 4;
  }
  auto owned = std::make_unique<OutlineData>(static_cast<const OutlineData&>(*key));
  OutlineData* outline = owned.get();
  aexcompat::scene_model::Identity value_identity{};
  if (!aexcompat::scene_model::registry().create_child(
          aexcompat::scene_model::ObjectKind::value, key->identity,
          0, value, u"Keyframe Value", value_identity)) {
    ++g_invalid_keyframe_operations;
    return 4;
  }
  ++record->live_values; record->mask->value_live = true;
  g_stream_values.emplace(value, CheckedStreamValue{
      record, outline, key, std::move(owned), value_identity});
  ++g_mask_lifetime.values_acquired;
  value->stream = record->handle; value->value = &outline->outline;
  return 0;
}
int32_t __cdecl set_keyframe_value(void* stream, int32_t index, const StreamValue* value) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  OutlineData* source = value ? find_outline(value->value) : nullptr;
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      !source || source == key) {
    ++g_invalid_keyframe_operations;
    return 4;
  }
  HostKeyframe candidate = *key;
  static_cast<OutlineData&>(candidate) = *source;
  return commit_keyframe_candidate(record, key, std::move(candidate))
      ? 0 : 4;
}
int32_t __cdecl get_stream_value_dimensionality(void* stream, int16_t* dimensions) {
  if (!find_stream(stream) || !dimensions) return 4;
  *dimensions = 0; return 0;
}
int32_t __cdecl get_stream_temporal_dimensionality(void* stream, int16_t* dimensions) {
  if (!find_stream(stream) || !dimensions) return 4;
  *dimensions = kHostTemporalDimensions; return 0;
}
bool register_keyframe_stream_values_atomic(
    HostStreamRef* stream, HostKeyframe* key,
    StreamValue* in_value, StreamValue* out_value) {
  std::array<StreamValue*, 2> values{};
  std::array<const OutlineData*, 2> sources{};
  std::size_t requested = 0;
  if (in_value) {
    values[requested] = in_value;
    sources[requested++] = &key->spatial_in;
  }
  if (out_value) {
    values[requested] = out_value;
    sources[requested++] = &key->spatial_out;
  }

  auto& registry = aexcompat::scene_model::registry();
  if (requested == 0 ||
      !registry.can_create_children(key->identity, requested))
    return false;

  std::size_t reserved_values = 0;
  try {
    g_stream_values.reserve(g_stream_values.size() + requested);
    for (; reserved_values < requested; ++reserved_values) {
      auto owned = std::make_unique<OutlineData>(*sources[reserved_values]);
      OutlineData* outline = owned.get();
      const auto inserted = g_stream_values.emplace(
          values[reserved_values],
          CheckedStreamValue{
              stream, outline, key, std::move(owned), {}});
      if (!inserted.second) break;
    }
  } catch (...) {
  }
  if (reserved_values != requested) {
    for (std::size_t index = 0; index < reserved_values; ++index)
      g_stream_values.erase(values[index]);
    return false;
  }

  std::array<aexcompat::scene_model::Identity, 2> identities{};
  const bool registered = requested == 2
      ? registry.create_child_pair(
            aexcompat::scene_model::ObjectKind::value, key->identity,
            {0, 1}, {values[0], values[1]},
            u"Keyframe Tangent", identities)
      : registry.create_child(
            aexcompat::scene_model::ObjectKind::value, key->identity,
            0, values[0], u"Keyframe Tangent", identities[0]);
  if (!registered) {
    for (std::size_t index = 0; index < requested; ++index)
      g_stream_values.erase(values[index]);
    return false;
  }

  for (std::size_t index = 0; index < requested; ++index) {
    auto& checked = g_stream_values.find(values[index])->second;
    checked.identity = identities[index];
    values[index]->stream = stream->handle;
    values[index]->value = &checked.outline->outline;
  }
  stream->live_values += static_cast<uint32_t>(requested);
  stream->mask->value_live = true;
  g_mask_lifetime.values_acquired += static_cast<uint32_t>(requested);
  return true;
}
int32_t __cdecl get_new_keyframe_spatial_tangents(int32_t plugin_id, void* stream,
                                                   int32_t index, StreamValue* in_value,
                                                   StreamValue* out_value) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  const std::size_t requested = (in_value ? 1u : 0u) + (out_value ? 1u : 0u);
  if (plugin_id != 1 || !record || !key ||
      requested == 0 || in_value == out_value ||
      g_stream_values.size() + requested > kMaxCheckedStreamValues ||
      (in_value && g_stream_values.find(in_value) != g_stream_values.end()) ||
      (out_value && g_stream_values.find(out_value) != g_stream_values.end())) {
    ++g_invalid_keyframe_operations; return 4;
  }
  auto& registry = aexcompat::scene_model::registry();
  aexcompat::scene_model::ObjectSnapshot registered_key{};
  if ((!registry.snapshot(key->identity, registered_key) &&
       !registry.can_create_children(
           record->identity, requested + 1)) ||
      !ensure_keyframe_identity(record, key, index)) {
    ++g_invalid_keyframe_operations;
    return 4;
  }
  if (!register_keyframe_stream_values_atomic(
          record, key, in_value, out_value)) {
    ++g_invalid_keyframe_operations;
    return 4;
  }
  return 0;
}
int32_t __cdecl set_keyframe_spatial_tangents(void* stream, int32_t index,
                                               const StreamValue* in_value,
                                               const StreamValue* out_value) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  const auto checked_for_stream = [record](const StreamValue* value) -> OutlineData* {
    if (!value) return nullptr;
    const auto found = g_stream_values.find(const_cast<StreamValue*>(value));
    return found != g_stream_values.end() && found->second.stream == record &&
        value->stream == record->handle && found->second.outline &&
        value->value == &found->second.outline->outline
        ? found->second.outline : nullptr;
  };
  OutlineData* in_outline = checked_for_stream(in_value);
  OutlineData* out_outline = checked_for_stream(out_value);
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      (!in_value && !out_value) || (in_value && !in_outline) ||
      (out_value && !out_outline)) {
    ++g_invalid_keyframe_operations; return 4;
  }
  HostKeyframe candidate = *key;
  if (in_outline) candidate.spatial_in = *in_outline;
  if (out_outline) candidate.spatial_out = *out_outline;
  return commit_keyframe_candidate(record, key, std::move(candidate))
      ? 0 : 4;
}
int32_t __cdecl get_keyframe_temporal_ease(void* stream, int32_t index, int32_t dimension,
                                            KeyframeEase* in_ease,
                                            KeyframeEase* out_ease) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      dimension < 0 || dimension >= kHostTemporalDimensions ||
      (!in_ease && !out_ease)) {
    ++g_invalid_keyframe_operations; return 4;
  }
  if (in_ease) *in_ease = key->temporal_in[dimension];
  if (out_ease) *out_ease = key->temporal_out[dimension];
  return 0;
}
int32_t __cdecl set_keyframe_temporal_ease(void* stream, int32_t index, int32_t dimension,
                                            const KeyframeEase* in_ease,
                                            const KeyframeEase* out_ease) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  const auto valid_ease = [](const KeyframeEase* ease) {
    return !ease || (std::isfinite(ease->speed) &&
        std::isfinite(ease->influence) && ease->influence >= 0.0 &&
        ease->influence <= 100.0);
  };
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      dimension < 0 || dimension >= kHostTemporalDimensions ||
      (!in_ease && !out_ease) || !valid_ease(in_ease) ||
      !valid_ease(out_ease)) {
    ++g_invalid_keyframe_operations; return 4;
  }
  HostKeyframe candidate = *key;
  if (in_ease) candidate.temporal_in[dimension] = *in_ease;
  if (out_ease) candidate.temporal_out[dimension] = *out_ease;
  return commit_keyframe_candidate(record, key, std::move(candidate))
      ? 0 : 4;
}
int32_t __cdecl get_keyframe_flags(void* stream, int32_t index, int32_t* flags) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (!key || !ensure_keyframe_identity(record, key, index) || !flags)
    return 4;
  *flags = key->flags; return 0;
}
int32_t __cdecl set_keyframe_flag(void* stream, int32_t index, int32_t flag, uint8_t enabled) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      flag == 0 || (flag & ~0x1f) != 0 ||
      (flag & (flag - 1)) != 0 || enabled > 1) {
    ++g_invalid_keyframe_operations; return 4;
  }
  HostKeyframe candidate = *key;
  if (enabled) candidate.flags |= flag;
  else candidate.flags &= ~flag;
  return commit_keyframe_candidate(record, key, std::move(candidate))
      ? 0 : 4;
}
int32_t __cdecl get_keyframe_interpolation(void* stream, int32_t index,
                                           int32_t* in_interp, int32_t* out_interp) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      (!in_interp && !out_interp))
    return 4;
  if (in_interp) *in_interp = key->in_interpolation;
  if (out_interp) *out_interp = key->out_interpolation;
  return 0;
}
int32_t __cdecl set_keyframe_interpolation(void* stream, int32_t index,
                                           int32_t in_interp, int32_t out_interp) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      in_interp < 0 || in_interp > 3 ||
      out_interp < 0 || out_interp > 3) {
    ++g_invalid_keyframe_operations; return 4;
  }
  HostKeyframe candidate = *key;
  candidate.in_interpolation = in_interp;
  candidate.out_interpolation = out_interp;
  return commit_keyframe_candidate(record, key, std::move(candidate))
      ? 0 : 4;
}
bool usable_keyframe_stream(const HostStreamRef* stream) {
  return stream && stream->kind == DynamicNodeKind::MaskOutline &&
      stream->mask && usable_mask(stream->mask);
}
int32_t __cdecl start_add_keyframes(void* stream, void** transaction) {
  HostStreamRef* record = find_stream(stream);
  if (!usable_keyframe_stream(record) || !transaction ||
      g_add_keyframe_transactions.size() >= 8) {
    ++g_invalid_keyframe_operations; return 4;
  }
  g_add_keyframe_transactions.push_back({{}, record, {}});
  *transaction = &g_add_keyframe_transactions.back().opaque;
  return 0;
}
int32_t __cdecl add_keyframes(void* handle, int16_t time_mode, const HostTime* time,
                              int32_t* index) {
  AddKeyframesTransaction* transaction = find_add_transaction(handle);
  if (!transaction || !usable_keyframe_stream(transaction->stream) ||
      time_mode < 0 || time_mode > 1 || !valid_time(time) || !index ||
      transaction->stream->mask->keyframes.size() + transaction->staged.size() >=
          kMaxKeyframesPerStream) {
    ++g_invalid_keyframe_operations; return 4;
  }
  const auto duplicate = std::find_if(transaction->staged.begin(), transaction->staged.end(),
      [time](const auto& key) { return time_equal(key.time, *time); });
  if (duplicate != transaction->staged.end()) {
    *index = static_cast<int32_t>(duplicate - transaction->staged.begin()); return 0;
  }
  transaction->staged.push_back(snapshot_keyframe(*transaction->stream->mask, *time));
  *index = static_cast<int32_t>(transaction->staged.size() - 1);
  return 0;
}
int32_t __cdecl set_add_keyframe(void* handle, int32_t index, const StreamValue* value) {
  AddKeyframesTransaction* transaction = find_add_transaction(handle);
  OutlineData* source = value ? find_outline(value->value) : nullptr;
  if (!transaction || !usable_keyframe_stream(transaction->stream) ||
      index < 0 ||
      static_cast<std::size_t>(index) >= transaction->staged.size() ||
      !source) { ++g_invalid_keyframe_operations; return 4; }
  static_cast<OutlineData&>(transaction->staged[index]) = *source;
  return 0;
}
int32_t __cdecl end_add_keyframes(uint8_t add, void* handle) {
  AddKeyframesTransaction* transaction = find_add_transaction(handle);
  if (!transaction) { ++g_invalid_keyframe_operations; return 4; }
  struct CleanupGuard {
    AddKeyframesTransaction* transaction;
    ~CleanupGuard() {
      const auto found = std::find_if(
          g_add_keyframe_transactions.begin(),
          g_add_keyframe_transactions.end(),
          [this](auto& item) { return &item == transaction; });
      if (found != g_add_keyframe_transactions.end())
        g_add_keyframe_transactions.erase(found);
    }
  } cleanup{transaction};
  if (add > 1) {
    ++g_invalid_keyframe_operations;
    return 4;
  }
  auto* stream = transaction->stream;
  if (!usable_keyframe_stream(stream)) {
    ++g_invalid_keyframe_operations;
    return 4;
  }
  auto staged = transaction->staged;
  auto& registry = aexcompat::scene_model::registry();
  aexcompat::scene_transaction::AtomicSceneTransaction atomic(
      registry, stream->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation);
  if (!atomic.stage()) return 4;
  if (!add) {
    atomic.cancel();
  } else {
    std::size_t new_count = 0;
    for (const auto& candidate : staged) {
      const auto duplicate = std::find_if(
          stream->mask->keyframes.begin(),
          stream->mask->keyframes.end(),
          [&](const auto& key) {
            return time_equal(key.time, candidate.time);
          });
      if (duplicate == stream->mask->keyframes.end()) ++new_count;
    }
    if (!atomic.validate(
            new_count == 0 ||
            registry.can_create_children(
                stream->identity, new_count))) {
      ++g_invalid_keyframe_operations;
      return 4;
    }
    aexcompat::scene_model::Registry::MutationCheckpoint
        registry_checkpoint{};
    if (!registry.capture_mutation_checkpoint(registry_checkpoint)) {
      ++g_invalid_keyframe_operations;
      return 4;
    }
    auto keyframes_before_apply = stream->mask->keyframes;
    const uint32_t mutations_before_apply = g_keyframe_mutations;
    std::size_t applied_count = 0;
    if (!atomic.commit(
            [&]() noexcept {
              for (auto& candidate : staged) {
                auto position = stream->mask->keyframes.begin();
                int32_t key_index = 0;
                while (position != stream->mask->keyframes.end() &&
                       time_less(position->time, candidate.time)) {
                  ++position;
                  ++key_index;
                }
                if (position != stream->mask->keyframes.end() &&
                    time_equal(position->time, candidate.time))
                  continue;
                if (!registry.create_child(
                        aexcompat::scene_model::ObjectKind::keyframe,
                        stream->identity, key_index, nullptr,
                        u"Mask Keyframe", candidate.identity) ||
                    !registry.initialize_keyframe_state(
                        candidate.identity,
                        keyframe_state(candidate)))
                  return false;
                stream->mask->keyframes.insert(
                    position, std::move(candidate));
                ++g_keyframe_mutations;
                ++applied_count;
                if (g_keyframe_apply_failure_after > 0 &&
                    applied_count >= static_cast<std::size_t>(
                        g_keyframe_apply_failure_after)) {
                  g_keyframe_apply_failure_after = -1;
                  return false;
                }
              }
              return true;
            },
            [&]() noexcept {
              stream->mask->keyframes.swap(keyframes_before_apply);
              g_keyframe_mutations = mutations_before_apply;
              return registry.restore_mutation_checkpoint(
                  registry_checkpoint);
            },
            []() noexcept { bump_render_project_timestamp(); })) {
      ++g_invalid_keyframe_operations;
      return 4;
    }
  }
  return 0;
}
int32_t __cdecl get_keyframe_label(void* stream, int32_t index, int32_t* label) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (!key || !ensure_keyframe_identity(record, key, index) || !label)
    return 4;
  *label = key->label; return 0;
}
int32_t __cdecl set_keyframe_label(void* stream, int32_t index, int32_t label) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (!key || !ensure_keyframe_identity(record, key, index) ||
      label < 0 || label > 16) {
    ++g_invalid_keyframe_operations;
    return 4;
  }
  HostKeyframe candidate = *key;
  candidate.label = label;
  return commit_keyframe_candidate(record, key, std::move(candidate))
      ? 0 : 4;
}


bool dynamic_leaf(DynamicNodeKind kind) {
  return kind == DynamicNodeKind::MaskOutline || kind == DynamicNodeKind::MaskFeather ||
      kind == DynamicNodeKind::MaskOpacity || kind == DynamicNodeKind::MaskExpansion;
}
const char* dynamic_match_name(DynamicNodeKind kind) {
  switch (kind) {
    case DynamicNodeKind::LayerRoot: return "ADBE Abstract Layer";
    case DynamicNodeKind::MaskParade: return "ADBE Mask Parade";
    case DynamicNodeKind::MaskAtom: return "ADBE Mask Atom";
    case DynamicNodeKind::MaskOutline: return "ADBE Mask Shape";
    case DynamicNodeKind::MaskFeather: return "ADBE Mask Feather";
    case DynamicNodeKind::MaskOpacity: return "ADBE Mask Opacity";
    case DynamicNodeKind::MaskExpansion: return "ADBE Mask Offset";
  }
  return "";
}
int32_t dynamic_depth(DynamicNodeKind kind) {
  if (kind == DynamicNodeKind::LayerRoot) return 0;
  if (kind == DynamicNodeKind::MaskParade) return 1;
  if (kind == DynamicNodeKind::MaskAtom) return 2;
  return 3;
}
uint32_t* dynamic_flags(HostStreamRef* stream) {
  if (!stream) return nullptr;
  if (stream->kind == DynamicNodeKind::LayerRoot) return &g_layer_dynamic_flags;
  if (stream->kind == DynamicNodeKind::MaskParade) return &g_mask_parade_dynamic_flags;
  if (!stream->mask) return nullptr;
  const std::size_t index = stream->kind == DynamicNodeKind::MaskOutline ? 0 :
      stream->kind == DynamicNodeKind::MaskFeather ? 1 :
      stream->kind == DynamicNodeKind::MaskOpacity ? 2 :
      stream->kind == DynamicNodeKind::MaskExpansion ? 3 : 4;
  return &stream->mask->dynamic_flags[index];
}
template <typename Apply>
bool commit_dynamic_stream_mutation(HostStreamRef* stream, Apply&& apply) {
  if (!stream || stream->identity.project_id == 0) return false;
  aexcompat::scene_transaction::AtomicSceneTransaction transaction(
      aexcompat::scene_model::registry(), stream->identity.project_id,
      &aexcompat::aegp_external_render_runtime::project_generation);
  if (!transaction.stage() || !transaction.validate(true)) return false;
  return transaction.commit(
      std::forward<Apply>(apply),
      []() noexcept { bump_render_project_timestamp(); });
}
int32_t __cdecl get_new_dynamic_stream_for_layer(int32_t plugin_id, void* layer, void** stream) {
  if (plugin_id != 1 || layer != &g_layer || !stream) return 4;
  return create_stream_ref(nullptr, DynamicNodeKind::LayerRoot, -1, stream);
}
int32_t __cdecl get_new_dynamic_stream_for_mask(int32_t plugin_id, void* mask, void** stream) {
  HostMask* record = find_mask(mask);
  if (plugin_id != 1 || !usable_mask(record) || !stream) return 4;
  return create_stream_ref(record, DynamicNodeKind::MaskAtom, -1, stream);
}
int32_t __cdecl get_dynamic_stream_depth(void* stream, int32_t* depth) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !depth) return 4;
  *depth = dynamic_depth(record->kind); ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_dynamic_stream_grouping_type(void* stream, int32_t* grouping) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !grouping) return 4;
  *grouping = dynamic_leaf(record->kind) ? 0 :
      record->kind == DynamicNodeKind::MaskParade ? 2 : 1;
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_num_streams_in_group(void* stream, int32_t* count) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !count || dynamic_leaf(record->kind)) return 4;
  *count = record->kind == DynamicNodeKind::LayerRoot ? 1 :
      record->kind == DynamicNodeKind::MaskParade ? static_cast<int32_t>(active_mask_count()) : 4;
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_dynamic_stream_flags(void* stream, uint32_t* flags) {
  uint32_t* stored = dynamic_flags(find_stream(stream));
  if (!stored || !flags) return 4;
  *flags = *stored; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl set_dynamic_stream_flag(void* stream, uint32_t flag, uint8_t undoable,
                                        uint8_t set) {
  HostStreamRef* record = find_stream(stream); uint32_t* stored = dynamic_flags(record);
  if (!stored || (flag != 1 && flag != 2) || undoable > 1 || set > 1 ||
      (!undoable && flag != 2)) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  uint32_t candidate = *stored;
  if (set) candidate |= flag; else candidate &= ~flag;
  return commit_dynamic_stream_mutation(
      record,
      [record, stored, candidate]() noexcept {
        *stored = candidate;
        if (record->mask) record->mask->dynamic_modified = true;
        ++g_dynamic_stream_mutations;
        return true;
      }) ? 0 : 4;
}
int32_t dynamic_child(HostStreamRef* parent, int32_t index, HostMask*& mask,
                      DynamicNodeKind& kind) {
  if (!parent || index < 0 || dynamic_leaf(parent->kind)) return 4;
  mask = parent->mask;
  if (parent->kind == DynamicNodeKind::LayerRoot) {
    if (index != 0) return 4; kind = DynamicNodeKind::MaskParade; mask = nullptr; return 0;
  }
  if (parent->kind == DynamicNodeKind::MaskParade) {
    const auto masks = ordered_active_masks();
    if (static_cast<std::size_t>(index) >= masks.size()) return 4;
    mask = masks[static_cast<std::size_t>(index)]; kind = DynamicNodeKind::MaskAtom; return 0;
  }
  static constexpr DynamicNodeKind children[4]{DynamicNodeKind::MaskOutline,
      DynamicNodeKind::MaskFeather, DynamicNodeKind::MaskOpacity,
      DynamicNodeKind::MaskExpansion};
  if (index >= 4) return 4; kind = children[index]; return 0;
}
int32_t __cdecl get_new_dynamic_stream_by_index(int32_t plugin_id, void* parent_stream,
                                                int32_t index, void** stream) {
  HostStreamRef* parent = find_stream(parent_stream); HostMask* mask{}; DynamicNodeKind kind{};
  if (plugin_id != 1 || !stream || dynamic_child(parent, index, mask, kind) != 0) {
    return 4;
  }
  ++g_dynamic_stream_queries;
  return create_stream_ref(mask, kind, kind == DynamicNodeKind::MaskOutline ? 400 :
      kind == DynamicNodeKind::MaskOpacity ? 401 : kind == DynamicNodeKind::MaskFeather ? 402 :
      kind == DynamicNodeKind::MaskExpansion ? 403 : -1, stream);
}
int32_t __cdecl get_new_dynamic_stream_by_match_name(int32_t plugin_id, void* parent_stream,
                                                     const char* match_name, void** stream) {
  HostStreamRef* parent = find_stream(parent_stream);
  if (plugin_id != 1 || !parent || !match_name || !stream ||
      std::strlen(match_name) >= 40 || dynamic_leaf(parent->kind) ||
      parent->kind == DynamicNodeKind::MaskParade) {
    return 4;
  }
  const int32_t count = parent->kind == DynamicNodeKind::LayerRoot ? 1 : 4;
  for (int32_t index = 0; index < count; ++index) {
    HostMask* mask{}; DynamicNodeKind kind{};
    if (dynamic_child(parent, index, mask, kind) == 0 &&
        std::strcmp(match_name, dynamic_match_name(kind)) == 0)
      return get_new_dynamic_stream_by_index(plugin_id, parent_stream, index, stream);
  }
  return 4;
}
int32_t __cdecl delete_dynamic_stream(void* stream) {
  HostStreamRef* record = find_stream(stream);
  if (!record || record->kind != DynamicNodeKind::MaskAtom || !record->mask ||
      record->mask->deleted || std::any_of(g_stream_refs.begin(), g_stream_refs.end(),
          [record](const auto& other) { return &other != record && other.mask == record->mask; })) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  const int32_t deleted_order = record->mask->dynamic_order;
  std::array<int32_t, kMaxHostMasks> staged_orders{};
  for (std::size_t index = 0; index < g_mask_scene.size(); ++index) {
    const auto& candidate = g_mask_scene[index];
    staged_orders[index] = !candidate.deleted &&
            candidate.dynamic_order > deleted_order
        ? candidate.dynamic_order - 1 : candidate.dynamic_order;
  }
  return commit_dynamic_stream_mutation(
      record,
      [record, staged_orders]() noexcept {
        record->mask->deleted = true;
        record->mask->dynamic_modified = true;
        for (std::size_t index = 0; index < g_mask_scene.size(); ++index)
          g_mask_scene[index].dynamic_order = staged_orders[index];
        ++g_dynamic_stream_mutations;
        return true;
      }) ? 0 : 4;
}
int32_t __cdecl reorder_dynamic_stream(void* stream, int32_t new_index) {
  HostStreamRef* record = find_stream(stream); const auto masks = ordered_active_masks();
  if (!record || record->kind != DynamicNodeKind::MaskAtom || !record->mask || new_index < 0 ||
      static_cast<std::size_t>(new_index) >= masks.size()) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  const int32_t old_index = record->mask->dynamic_order;
  std::array<int32_t, kMaxHostMasks> staged_orders{};
  for (std::size_t index = 0; index < g_mask_scene.size(); ++index) {
    const auto& mask = g_mask_scene[index];
    staged_orders[index] = mask.dynamic_order;
    if (old_index < new_index && mask.dynamic_order > old_index &&
        mask.dynamic_order <= new_index)
      --staged_orders[index];
    else if (old_index > new_index && mask.dynamic_order >= new_index &&
             mask.dynamic_order < old_index)
      ++staged_orders[index];
  }
  const std::size_t record_index =
      static_cast<std::size_t>(record->mask - g_mask_scene.data());
  staged_orders[record_index] = new_index;
  return commit_dynamic_stream_mutation(
      record,
      [record, staged_orders]() noexcept {
        for (std::size_t index = 0; index < g_mask_scene.size(); ++index)
          g_mask_scene[index].dynamic_order = staged_orders[index];
        record->mask->dynamic_modified = true;
        ++g_dynamic_stream_mutations;
        return true;
      }) ? 0 : 4;
}
int32_t __cdecl duplicate_dynamic_stream(int32_t plugin_id, void* stream, int32_t* new_index) {
  HostStreamRef* record = find_stream(stream);
  if (plugin_id != 1 || !record || record->kind != DynamicNodeKind::MaskAtom || !record->mask ||
      g_mask_scene.size() >= kMaxHostMasks ||
      g_mask_scene.capacity() < kMaxHostMasks ||
      g_next_mask_id <= 0 ||
      g_next_mask_id > std::numeric_limits<int32_t>::max() - 0x10000000 ||
      g_next_stream_id <= 0 ||
      g_next_stream_id > std::numeric_limits<int32_t>::max() - 3) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  HostMask copy = *record->mask;
  copy.mask_live = false; copy.stream_live = false; copy.value_live = false; copy.deleted = false;
  copy.id = g_next_mask_id; copy.outline_stream_id = g_next_stream_id;
  copy.feather_stream_id = g_next_stream_id + 1;
  copy.opacity_stream_id = g_next_stream_id + 2;
  copy.expansion_stream_id = g_next_stream_id + 3;
  copy.dynamic_order = static_cast<int32_t>(active_mask_count());
  copy.dynamic_modified = true;
  const int32_t candidate_index = copy.dynamic_order;
  return commit_dynamic_stream_mutation(
      record,
      [&copy, new_index, candidate_index]() noexcept {
        g_mask_scene.push_back(std::move(copy));
        ++g_next_mask_id;
        g_next_stream_id += 4;
        if (new_index) *new_index = candidate_index;
        ++g_dynamic_stream_mutations;
        return true;
      }) ? 0 : 4;
}
int32_t __cdecl set_dynamic_stream_name(void* stream, const uint16_t* name) {
  HostStreamRef* record = find_stream(stream);
  if (!record || record->kind != DynamicNodeKind::MaskAtom || !record->mask || !name) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  std::size_t length = 0; while (length <= 127 && name[length]) ++length;
  if (length > 127) { ++g_invalid_dynamic_stream_operations; return 4; }
  std::u16string candidate(
      reinterpret_cast<const char16_t*>(name), length);
  return commit_dynamic_stream_mutation(
      record,
      [record, candidate = std::move(candidate)]() mutable noexcept {
        record->mask->dynamic_name = std::move(candidate);
        record->mask->dynamic_modified = true;
        ++g_dynamic_stream_mutations;
        return true;
      }) ? 0 : 4;
}
int32_t __cdecl can_add_dynamic_stream(void* stream, const char* match_name, uint8_t* can_add) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !match_name || !can_add || std::strlen(match_name) >= 40) return 4;
  *can_add = record->kind == DynamicNodeKind::MaskParade &&
      std::strcmp(match_name, "ADBE Mask Atom") == 0 && g_mask_scene.size() < kMaxHostMasks;
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl add_dynamic_stream(int32_t plugin_id, void* stream, const char* match_name,
                                   void** added) {
  HostStreamRef* group = find_stream(stream); uint8_t can_add{};
  if (plugin_id != 1 || !added || can_add_dynamic_stream(stream, match_name, &can_add) != 0 ||
      !can_add || !group || group->kind != DynamicNodeKind::MaskParade ||
      g_stream_refs.size() >= 64 ||
      g_mask_scene.capacity() < kMaxHostMasks ||
      g_next_mask_id <= 0 ||
      g_next_mask_id > std::numeric_limits<int32_t>::max() - 0x10000000 ||
      g_next_stream_id <= 0 ||
      g_next_stream_id > std::numeric_limits<int32_t>::max() - 3) {
    ++g_invalid_dynamic_stream_operations;
    return 4;
  }
  auto& registry = aexcompat::scene_model::registry();
  aexcompat::scene_model::Identity layer{};
  if (!registry.identity_for_object(
          1, 2001, aexcompat::scene_model::ObjectKind::layer, layer) ||
      !registry.can_create_children(layer, 1, 1)) {
    ++g_invalid_dynamic_stream_operations;
    return 4;
  }
  HostMask mask;
  mask.id = g_next_mask_id;
  mask.outline_stream_id = g_next_stream_id;
  mask.feather_stream_id = g_next_stream_id + 1;
  mask.opacity_stream_id = g_next_stream_id + 2;
  mask.expansion_stream_id = g_next_stream_id + 3;
  mask.dynamic_order = static_cast<int32_t>(active_mask_count());
  mask.dynamic_modified = true;
  return commit_dynamic_stream_mutation(
      group,
      [&mask, added]() noexcept {
        const int32_t saved_mask_id = g_next_mask_id;
        const int32_t saved_stream_id = g_next_stream_id;
        g_mask_scene.push_back(std::move(mask));
        ++g_next_mask_id;
        g_next_stream_id += 4;
        void* created = nullptr;
        if (create_stream_ref(
                &g_mask_scene.back(), DynamicNodeKind::MaskAtom,
                -1, &created) != 0) {
          g_mask_scene.pop_back();
          g_next_mask_id = saved_mask_id;
          g_next_stream_id = saved_stream_id;
          return false;
        }
        *added = created;
        ++g_dynamic_stream_mutations;
        return true;
      }) ? 0 : 4;
}
int32_t __cdecl get_dynamic_match_name(void* stream, char* match_name) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !match_name) return 4;
  strcpy_s(match_name, 40, dynamic_match_name(record->kind));
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_new_parent_dynamic_stream(int32_t plugin_id, void* stream, void** parent) {
  HostStreamRef* record = find_stream(stream);
  if (plugin_id != 1 || !record || !parent || record->kind == DynamicNodeKind::LayerRoot) {
    return 4;
  }
  DynamicNodeKind kind = record->kind == DynamicNodeKind::MaskParade ? DynamicNodeKind::LayerRoot :
      record->kind == DynamicNodeKind::MaskAtom ? DynamicNodeKind::MaskParade : DynamicNodeKind::MaskAtom;
  HostMask* mask = kind == DynamicNodeKind::MaskAtom ? record->mask : nullptr;
  ++g_dynamic_stream_queries; return create_stream_ref(mask, kind, -1, parent);
}
int32_t __cdecl get_dynamic_stream_modified(void* stream, uint8_t* modified) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !modified) return 4;
  *modified = record->mask && record->mask->dynamic_modified ? 1 : 0;
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_dynamic_stream_index(void* stream, int32_t* index) {
  HostStreamRef* record = find_stream(stream);
  if (!record || record->kind != DynamicNodeKind::MaskAtom || !record->mask || !index) return 4;
  *index = record->mask->dynamic_order; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl is_separation_leader(void* stream, uint8_t* leader) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !dynamic_leaf(record->kind) || !leader) return 4;
  *leader = 0; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl are_dimensions_separated(void* stream, uint8_t* separated) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !dynamic_leaf(record->kind) || !separated) return 4;
  *separated = 0; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl reject_set_dimensions_separated(void* stream, uint8_t) {
  if (!find_stream(stream)) return 4; ++g_invalid_dynamic_stream_operations; return 4;
}
int32_t __cdecl reject_get_separation_follower(void* stream, int16_t, void** follower) {
  if (follower) *follower = nullptr; if (!find_stream(stream)) return 4;
  ++g_invalid_dynamic_stream_operations; return 4;
}
int32_t __cdecl is_separation_follower(void* stream, uint8_t* follower) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !dynamic_leaf(record->kind) || !follower) return 4;
  *follower = 0; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl reject_get_separation_leader(void* stream, void** leader) {
  if (leader) *leader = nullptr; if (!find_stream(stream)) return 4;
  ++g_invalid_dynamic_stream_operations; return 4;
}
int32_t __cdecl reject_get_separation_dimension(void* stream, int16_t* dimension) {
  if (dimension) *dimension = 0; if (!find_stream(stream)) return 4;
  ++g_invalid_dynamic_stream_operations; return 4;
}


int32_t __cdecl is_mask_outline_open(void* outline, uint8_t* open) {
  OutlineData* record = find_outline(outline);
  if (!record || !open) return 4;
  *open = record->open ? 1 : 0;
  return 0;
}

int32_t __cdecl set_mask_outline_open(void* outline, uint8_t open) {
  OutlineData* record = find_outline(outline);
  if (!record) { ++g_invalid_outline_operations; return 4; }
  const bool requested = open != 0;
  if (record->open == requested) return 0;
  if (requested) {
    if (!record->vertices.empty()) record->vertices.pop_back();
  } else if (!record->vertices.empty()) {
    record->vertices.push_back(record->vertices.front());
  }
  record->open = requested;
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl get_mask_outline_num_segments(void* outline, int32_t* count) {
  OutlineData* record = find_outline(outline);
  if (!record || !count) return 4;
  const std::size_t vertices = distinct_vertex_count(*record);
  *count = static_cast<int32_t>(vertices == 0 ? 0 :
      vertices - static_cast<std::size_t>(record->open));
  return 0;
}

int32_t __cdecl get_mask_outline_vertex_info(void* outline, int32_t index,
                                             MaskVertex* vertex) {
  OutlineData* record = find_outline(outline);
  if (!record || !vertex || index < 0 ||
      static_cast<std::size_t>(index) >= record->vertices.size()) return 4;
  *vertex = record->vertices[static_cast<std::size_t>(index)];
  return 0;
}

bool finite_vertex(const MaskVertex& vertex) {
  return std::isfinite(vertex.x) && std::isfinite(vertex.y) &&
      std::isfinite(vertex.tangent_in_x) && std::isfinite(vertex.tangent_in_y) &&
      std::isfinite(vertex.tangent_out_x) && std::isfinite(vertex.tangent_out_y);
}

int32_t __cdecl set_mask_outline_vertex_info(void* outline, int32_t index,
                                             const MaskVertex* vertex) {
  OutlineData* record = find_outline(outline);
  const std::size_t count = record ? distinct_vertex_count(*record) : 0;
  if (!record || !vertex || !finite_vertex(*vertex) || index < 0 ||
      static_cast<std::size_t>(index) > count ||
      (record->open && static_cast<std::size_t>(index) == count)) {
    ++g_invalid_outline_operations;
    return 4;
  }
  const std::size_t target = static_cast<std::size_t>(index) == count ? 0 :
      static_cast<std::size_t>(index);
  record->vertices[target] = *vertex;
  sync_closed_vertex(*record);
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl create_mask_outline_vertex(void* outline, int32_t position) {
  OutlineData* record = find_outline(outline);
  const std::size_t count = record ? distinct_vertex_count(*record) : 0;
  if (!record || count >= kMaxOutlineVertices) {
    ++g_invalid_outline_operations;
    return 4;
  }
  if (position == 10922) position = static_cast<int32_t>(count);
  if (position < 0 || static_cast<std::size_t>(position) > count) {
    ++g_invalid_outline_operations;
    return 4;
  }
  if (!record->open && count == 0) {
    record->vertices = {MaskVertex{}, MaskVertex{}};
  } else {
    record->vertices.insert(record->vertices.begin() + position, MaskVertex{});
  }
  for (auto& feather : record->feathers)
    if (feather.segment >= position) ++feather.segment;
  sync_closed_vertex(*record);
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl delete_mask_outline_vertex(void* outline, int32_t index) {
  OutlineData* record = find_outline(outline);
  const std::size_t count = record ? distinct_vertex_count(*record) : 0;
  if (!record || index < 0 || static_cast<std::size_t>(index) >= count) {
    ++g_invalid_outline_operations;
    return 4;
  }
  if (!record->open && count == 1) record->vertices.clear();
  else record->vertices.erase(record->vertices.begin() + index);
  record->feathers.erase(std::remove_if(record->feathers.begin(), record->feathers.end(),
      [index](const auto& feather) { return feather.segment == index; }), record->feathers.end());
  for (auto& feather : record->feathers)
    if (feather.segment > index) --feather.segment;
  sync_closed_vertex(*record);
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl get_mask_outline_num_feathers(void* outline, int32_t* count) {
  OutlineData* record = find_outline(outline);
  if (!record || !count) return 4;
  *count = static_cast<int32_t>(record->feathers.size());
  return 0;
}

bool valid_feather(const OutlineData& mask, const MaskFeather& feather) {
  const std::size_t vertices = distinct_vertex_count(mask);
  const std::size_t segments = vertices == 0 ? 0 : vertices - static_cast<std::size_t>(mask.open);
  return feather.segment >= 0 && static_cast<std::size_t>(feather.segment) < segments &&
      std::isfinite(feather.segment_s) && feather.segment_s >= 0 && feather.segment_s <= 1 &&
      std::isfinite(feather.radius) && std::isfinite(feather.ui_corner_angle) &&
      feather.ui_corner_angle >= 0 && feather.ui_corner_angle <= 1 &&
      std::isfinite(feather.tension) && feather.tension >= 0 && feather.tension <= 1 &&
      feather.interp <= 1 && feather.type <= 1 && (feather.type == 1 || feather.radius >= 0);
}

int32_t __cdecl get_mask_outline_feather_info(void* outline, int32_t index,
                                              MaskFeather* feather) {
  OutlineData* record = find_outline(outline);
  if (!record || !feather || index < 0 ||
      static_cast<std::size_t>(index) >= record->feathers.size()) return 4;
  *feather = record->feathers[static_cast<std::size_t>(index)];
  return 0;
}

int32_t __cdecl set_mask_outline_feather_info(void* outline, int32_t index,
                                              const MaskFeather* feather) {
  OutlineData* record = find_outline(outline);
  if (!record || !feather || !valid_feather(*record, *feather) || index < 0 ||
      static_cast<std::size_t>(index) >= record->feathers.size()) {
    ++g_invalid_outline_operations;
    return 4;
  }
  record->feathers[static_cast<std::size_t>(index)] = *feather;
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl create_mask_outline_feather(void* outline, const MaskFeather* feather,
                                            int32_t* position) {
  OutlineData* record = find_outline(outline);
  if (!record || !feather || !position || !valid_feather(*record, *feather) ||
      record->feathers.size() >= kMaxOutlineFeathers) {
    ++g_invalid_outline_operations;
    return 4;
  }
  record->feathers.push_back(*feather);
  *position = static_cast<int32_t>(record->feathers.size() - 1);
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl delete_mask_outline_feather(void* outline, int32_t index) {
  OutlineData* record = find_outline(outline);
  if (!record || index < 0 || static_cast<std::size_t>(index) >= record->feathers.size()) {
    ++g_invalid_outline_operations;
    return 4;
  }
  record->feathers.erase(record->feathers.begin() + index);
  ++g_outline_mutations;
  return 0;
}

}  // namespace aexcompat::l2_detail


// Mask scene helpers, PF path bridge, and configure_mask_scene moved from
// worker_main (issue #170); the scene state they read is defined above and
// the effect/layer identities stay in l2_main.
namespace aexcompat::l2_detail {

extern OpaqueHostObject g_effect;
extern OpaqueHostObject g_layer;
using aexcompat::world_safety::bounded_typed_world;

void raise_mask_access_violation() {
  RaiseException(EXCEPTION_ACCESS_VIOLATION, 0, 0, nullptr);
}

aexcompat::mask_runtime::Snapshot mask_runtime_snapshot() {
  aexcompat::mask_runtime::Snapshot snapshot;
  snapshot.active_masks = static_cast<uint32_t>(std::count_if(
      g_mask_scene.begin(), g_mask_scene.end(), [](const auto& mask) { return !mask.deleted; }));
  snapshot.masks_acquired = g_mask_lifetime.masks_acquired;
  snapshot.masks_disposed = g_mask_lifetime.masks_disposed;
  snapshot.streams_acquired = g_mask_lifetime.streams_acquired;
  snapshot.streams_disposed = g_mask_lifetime.streams_disposed;
  snapshot.values_acquired = g_mask_lifetime.values_acquired;
  snapshot.values_disposed = g_mask_lifetime.values_disposed;
  snapshot.mask_mutations = g_mask_mutations;
  snapshot.invalid_mask_operations = g_invalid_mask_operations;
  snapshot.outline_mutations = g_outline_mutations;
  snapshot.invalid_outline_operations = g_invalid_outline_operations;
  snapshot.keyframe_mutations = g_keyframe_mutations;
  snapshot.invalid_keyframe_operations = g_invalid_keyframe_operations;
  snapshot.stream_metadata_queries = g_stream_metadata_queries;
  snapshot.stream_duplicates = g_stream_duplicates;
  snapshot.invalid_stream_operations = g_invalid_stream_operations;
  snapshot.dynamic_stream_mutations = g_dynamic_stream_mutations;
  snapshot.invalid_dynamic_stream_operations = g_invalid_dynamic_stream_operations;
  return snapshot;
}

bool snapshot_mask_curve(void* handle, aexcompat::mask_runtime::CurveSnapshot& curve) {
  HostMask* mask = find_mask(handle);
  if (!mask || mask->deleted) return false;
  aexcompat::mask_runtime::CurveSnapshot candidate;
  candidate.id = mask->id;
  candidate.open = mask->open;
  candidate.vertices.reserve(mask->vertices.size());
  for (const auto& vertex : mask->vertices) {
    candidate.vertices.push_back({vertex.x, vertex.y, vertex.tangent_in_x,
                                  vertex.tangent_in_y, vertex.tangent_out_x,
                                  vertex.tangent_out_y});
  }
  curve = std::move(candidate);
  return true;
}

bool install_synthetic_mask_scene(
    const std::vector<aexcompat::mask_runtime::CurveSnapshot>& curves) {
  if (!g_stream_refs.empty() || !g_stream_values.empty() || !g_add_keyframe_transactions.empty())
    return false;
  g_mask_scene.clear();
  g_mask_scene.reserve(kMaxHostMasks);
  if (curves.size() > kMaxHostMasks) return false;
  for (const auto& curve : curves) {
    HostMask mask;
    mask.id = curve.id;
    mask.open = curve.open;
    mask.vertices.reserve(curve.vertices.size());
    for (const auto& vertex : curve.vertices)
      mask.vertices.push_back({vertex.x, vertex.y, vertex.tangent_in_x,
          vertex.tangent_in_y, vertex.tangent_out_x, vertex.tangent_out_y});
    g_mask_scene.push_back(std::move(mask));
  }
  return true;
}

std::vector<HostMask*> ordered_active_masks();
std::vector<aexcompat::pf_path_runtime::PathInfo> enumerate_pf_paths() {
  std::vector<aexcompat::pf_path_runtime::PathInfo> result;
  for (auto* mask : ordered_active_masks())
    result.push_back({mask, mask->id, mask->dynamic_order, mask->open,
                      mask->invert, mask->mode, mask->opacity});
  return result;
}

bool snapshot_pf_path(void* handle, aexcompat::mask_runtime::CurveSnapshot& curve) {
  auto* mask = static_cast<HostMask*>(handle);
  if (!mask || mask->deleted) return false;
  aexcompat::mask_runtime::CurveSnapshot candidate;
  candidate.id = mask->id;
  candidate.open = mask->open;
  candidate.vertices.reserve(mask->vertices.size());
  for (const auto& vertex : mask->vertices)
    candidate.vertices.push_back({vertex.x, vertex.y, vertex.tangent_in_x,
        vertex.tangent_in_y, vertex.tangent_out_x, vertex.tangent_out_y});
  curve = std::move(candidate);
  return true;
}

bool bounded_pf_path_world(void* world,
    aexcompat::pf_path_runtime::WorldView& view) {
  if (!world) return false;
  aexcompat::world_safety::DispatchWorldFormat resolved{};
  if (!aexcompat::world_registry::resolve_dispatch_world_format(world, resolved))
    return false;
  int32_t pixel_bytes{};
  switch (resolved.pixel_format) {
    case aexcompat::world_registry::kPixelFormatArgb32:
      pixel_bytes = 4;
      break;
    case aexcompat::world_registry::kPixelFormatArgb64:
      pixel_bytes = 8;
      break;
    case aexcompat::world_registry::kPixelFormatArgb128:
      pixel_bytes = 16;
      break;
    default:
      return false;
  }
  unsigned char* pixels{};
  int32_t rowbytes{}, width{}, height{};
  if (!bounded_typed_world(world, pixel_bytes, pixels, rowbytes, width, height) ||
      resolved.data != pixels || resolved.rowbytes != rowbytes ||
      resolved.width != width || resolved.height != height)
    return false;
  view = {pixels, rowbytes, width, height, pixel_bytes, resolved.pixel_format};
  return true;
}

std::size_t distinct_vertex_count(const OutlineData& mask) {
  return mask.vertices.size() - static_cast<std::size_t>(!mask.open && !mask.vertices.empty());
}

void sync_closed_vertex(OutlineData& mask) {
  if (!mask.open && !mask.vertices.empty()) mask.vertices.back() = mask.vertices.front();
}

bool mask_lifetimes_balanced() {
  return g_mask_lifetime.masks_acquired == g_mask_lifetime.masks_disposed &&
      g_mask_lifetime.streams_acquired == g_mask_lifetime.streams_disposed &&
      g_mask_lifetime.values_acquired == g_mask_lifetime.values_disposed &&
      g_stream_refs.empty() && g_stream_values.empty() && g_add_keyframe_transactions.empty() &&
      std::none_of(g_mask_scene.begin(), g_mask_scene.end(), [](const auto& mask) {
        return mask.mask_live || mask.stream_live || mask.value_live;
      });
}

bool configure_mask_scene(const std::string& scene_id) {
  aexcompat::mask_runtime::configure_host_context(
      {&g_layer, &raise_mask_access_violation, &mask_runtime_snapshot,
       &snapshot_mask_curve, &mask_lifetimes_balanced, &install_synthetic_mask_scene});
  aexcompat::pf_path_runtime::configure(
      {&g_effect, &enumerate_pf_paths, &snapshot_pf_path, &bounded_pf_path_world});
  if (!g_stream_refs.empty() || !g_stream_values.empty() ||
      !g_add_keyframe_transactions.empty()) return false;
  try {
    g_stream_values.reserve(kMaxCheckedStreamValues);
  } catch (...) {
    return false;
  }
  aexcompat::mask_runtime::SceneSeed seed;
  if (!aexcompat::mask_runtime::build_scene_seed(scene_id, seed)) return false;
  g_mask_scene.clear();
  g_mask_scene.reserve(kMaxHostMasks);
  g_mask_lifetime = {};
  aexcompat::mask_runtime::set_mask_scene_id(seed.id);
  for (auto& source : seed.masks) {
    HostMask mask;
    mask.id = g_next_mask_id++;
    mask.outline_stream_id = g_next_stream_id++;
    mask.feather_stream_id = g_next_stream_id++;
    mask.opacity_stream_id = g_next_stream_id++;
    mask.expansion_stream_id = g_next_stream_id++;
    mask.open = source.open;
    mask.dynamic_order = source.dynamic_order;
    mask.vertices.reserve(source.vertices.size());
    for (const auto& vertex : source.vertices) {
      mask.vertices.push_back({vertex.x, vertex.y, vertex.tangent_in_x,
          vertex.tangent_in_y, vertex.tangent_out_x, vertex.tangent_out_y});
    }
    g_mask_scene.push_back(std::move(mask));
  }
  return true;
}

std::vector<HostMask*> ordered_active_masks() {
  std::vector<HostMask*> masks;
  for (auto& mask : g_mask_scene) if (!mask.deleted) masks.push_back(&mask);
  std::sort(masks.begin(), masks.end(), [](const HostMask* left, const HostMask* right) {
    return left->dynamic_order < right->dynamic_order;
  });
  return masks;
}

HostMask* find_mask(void* handle) {
  const auto found = std::find_if(g_mask_scene.begin(), g_mask_scene.end(),
      [handle](auto& mask) { return handle == &mask.mask; });
  return found == g_mask_scene.end() ? nullptr : &*found;
}
HostStreamRef* find_stream(void* handle) {
  aexcompat::scene_model::ObjectSnapshot resolved{};
  int32_t possession_id = 0;
  auto& registry = aexcompat::scene_model::registry();
  if (!registry.resolve(handle, aexcompat::scene_model::ObjectKind::stream,
                        resolved) ||
      !registry.possession(
          handle, aexcompat::scene_model::ObjectKind::stream,
          possession_id) ||
      possession_id != 1)
    return nullptr;
  auto* record = static_cast<HostStreamRef*>(resolved.legacy_handle);
  const auto found = std::find_if(
      g_stream_refs.begin(), g_stream_refs.end(),
      [record](auto& stream) { return record == &stream; });
  return found == g_stream_refs.end() ||
      found->identity != resolved.identity ||
      found->handle != handle ? nullptr : &*found;
}
OutlineData* find_outline(void* handle) {
  const auto found = std::find_if(g_mask_scene.begin(), g_mask_scene.end(),
      [handle](auto& mask) { return handle == &mask.outline; });
  if (found != g_mask_scene.end()) return &*found;
  for (auto& mask : g_mask_scene) {
    const auto key = std::find_if(mask.keyframes.begin(), mask.keyframes.end(),
        [handle](auto& item) { return handle == &item.outline; });
    if (key != mask.keyframes.end()) return &*key;
  }
  for (auto& item : g_stream_values) {
    if (item.second.outline && handle == &item.second.outline->outline)
      return item.second.outline;
  }
  return nullptr;
}

std::size_t mask_open_count() {
  return static_cast<std::size_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& mask) { return !mask.deleted && mask.open; }));
}

std::size_t active_mask_count() {
  return static_cast<std::size_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& mask) { return !mask.deleted; }));
}

std::size_t mask_tangent_vertex_count() {
  std::size_t count = 0;
  for (const auto& mask : g_mask_scene) {
    if (mask.deleted) continue;
    const auto end = !mask.open && !mask.vertices.empty()
        ? mask.vertices.end() - 1 : mask.vertices.end();
    count += static_cast<std::size_t>(std::count_if(mask.vertices.begin(), end,
        [](const auto& vertex) {
          return vertex.tangent_in_x != 0 || vertex.tangent_in_y != 0 ||
                 vertex.tangent_out_x != 0 || vertex.tangent_out_y != 0;
        }));
  }
  return count;
}

}  // namespace aexcompat::l2_detail
