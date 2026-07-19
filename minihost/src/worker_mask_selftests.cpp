#include "worker_mask_selftests.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"

#include <algorithm>
#include <cstring>
#include <limits>

namespace aexcompat::l2_detail {

namespace {
bool lifetimes_balanced() {
  const auto hook = aexcompat::mask_runtime::host_context().lifetimes_balanced;
  return hook && hook();
}
}  // namespace

bool verify_keyframe_ownership_rejection() {
  const auto before = aexcompat::mask_runtime::snapshot();
  void* mask = nullptr; void* stream = nullptr;
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_mask_stream(1, mask, 400, &stream) != 0) return false;
  const HostTime later{20, 1}, earlier{10, 1}, batched{30, 1}, cancelled{40, 1};
  int32_t later_index = -1, earlier_index = -1, duplicate_index = -1, count = -1;
  HostTime observed{};
  bool passed = insert_keyframe(stream, 0, &later, &later_index) == 0 && later_index == 0 &&
      insert_keyframe(stream, 0, &earlier, &earlier_index) == 0 && earlier_index == 0 &&
      insert_keyframe(stream, 0, &earlier, &duplicate_index) == 0 && duplicate_index == 0 &&
      get_stream_num_keyframes(stream, &count) == 0 && count == 2 &&
      get_keyframe_time(stream, 0, 0, &observed) == 0 && time_equal(observed, earlier) &&
      set_keyframe_flag(stream, 0, 1, 1) == 0 &&
      set_keyframe_interpolation(stream, 0, 2, 3) == 0 &&
      set_keyframe_label(stream, 0, 7) == 0;
  int32_t flags{}, in_interp{}, out_interp{}, label{};
  int16_t value_dimensions{-1}, temporal_dimensions{-1};
  passed = passed && get_keyframe_flags(stream, 0, &flags) == 0 && flags == 1 &&
      get_keyframe_interpolation(stream, 0, &in_interp, &out_interp) == 0 &&
      in_interp == 2 && out_interp == 3 &&
      get_keyframe_label(stream, 0, &label) == 0 && label == 7 &&
      get_stream_value_dimensionality(stream, &value_dimensions) == 0 &&
      value_dimensions == 0 &&
      get_stream_temporal_dimensionality(stream, &temporal_dimensions) == 0 &&
      temporal_dimensions == kHostTemporalDimensions;
  StreamValue spatial_in{}, spatial_out{}, spatial_check_in{}, spatial_check_out{};
  MaskVertex spatial_vertex{};
  passed = passed && get_new_keyframe_spatial_tangents(
      1, stream, 0, &spatial_in, &spatial_out) == 0 &&
      get_mask_outline_vertex_info(spatial_in.value, 0, &spatial_vertex) == 0;
  spatial_vertex.tangent_in_x = -12.5;
  spatial_vertex.tangent_out_y = 19.25;
  passed = passed && set_mask_outline_vertex_info(
      spatial_in.value, 0, &spatial_vertex) == 0 &&
      set_keyframe_spatial_tangents(stream, 0, &spatial_in, &spatial_out) == 0;
  const StreamValue stale_spatial = spatial_in;
  passed = passed && dispose_stream_value(&spatial_in) == 0 &&
      set_keyframe_spatial_tangents(stream, 0, &stale_spatial, &spatial_out) != 0 &&
      dispose_stream_value(&spatial_out) == 0 &&
      get_new_keyframe_spatial_tangents(
          1, stream, 0, &spatial_check_in, &spatial_check_out) == 0 &&
      get_mask_outline_vertex_info(spatial_check_in.value, 0, &spatial_vertex) == 0 &&
      spatial_vertex.tangent_in_x == -12.5 && spatial_vertex.tangent_out_y == 19.25 &&
      dispose_stream_value(&spatial_check_out) == 0 &&
      dispose_stream_value(&spatial_check_in) == 0;
  const KeyframeEase ease_in{23.5, 67.0}, ease_out{31.25, 72.5};
  KeyframeEase observed_in{-1.0, -1.0}, observed_out{-1.0, -1.0};
  passed = passed && set_keyframe_temporal_ease(
      stream, 0, 0, &ease_in, &ease_out) == 0 &&
      get_keyframe_temporal_ease(stream, 0, 0, &observed_in, &observed_out) == 0 &&
      observed_in.speed == ease_in.speed && observed_in.influence == ease_in.influence &&
      observed_out.speed == ease_out.speed && observed_out.influence == ease_out.influence;
  StreamValue unchanged_in{};
  unchanged_in.stream = reinterpret_cast<void*>(0x1111);
  unchanged_in.value = reinterpret_cast<void*>(0x2222);
  const StreamValue unchanged_before = unchanged_in;
  KeyframeEase unchanged_ease{41.0, 42.0};
  passed = passed && get_new_keyframe_spatial_tangents(
      2, stream, 0, &unchanged_in, nullptr) != 0 &&
      unchanged_in.stream == unchanged_before.stream && unchanged_in.value == unchanged_before.value &&
      set_keyframe_spatial_tangents(stream, 0, &unchanged_in, nullptr) != 0 &&
      get_keyframe_temporal_ease(stream, 0, 1, &unchanged_ease, nullptr) != 0 &&
      unchanged_ease.speed == 41.0 && unchanged_ease.influence == 42.0 &&
      set_keyframe_temporal_ease(stream, -1, 0, &ease_in, &ease_out) != 0 &&
      get_new_keyframe_spatial_tangents(1, stream, 0, nullptr, nullptr) != 0 &&
      set_keyframe_temporal_ease(stream, 0, 0, nullptr, nullptr) != 0;
  StreamValue later_value{}, hold_value{}, midpoint_value{};
  MaskVertex later_vertex{}, hold_vertex{}, midpoint_vertex{};
  const HostTime midpoint{15, 1};
  passed = passed && get_new_keyframe_value(1, stream, 1, &later_value) == 0 &&
      get_mask_outline_vertex_info(later_value.value, 0, &later_vertex) == 0;
  later_vertex.x += 10;
  passed = passed && set_mask_outline_vertex_info(later_value.value, 0, &later_vertex) == 0 &&
      set_keyframe_value(stream, 1, &later_value) == 0 &&
      dispose_stream_value(&later_value) == 0 &&
      get_new_stream_value(1, stream, 0, &midpoint, 0, &hold_value) == 0 &&
      get_mask_outline_vertex_info(hold_value.value, 0, &hold_vertex) == 0 &&
      hold_vertex.x == later_vertex.x - 10 && dispose_stream_value(&hold_value) == 0 &&
      set_keyframe_interpolation(stream, 0, 2, 1) == 0 &&
      get_new_stream_value(1, stream, 0, &midpoint, 0, &midpoint_value) == 0 &&
      get_mask_outline_vertex_info(midpoint_value.value, 0, &midpoint_vertex) == 0 &&
      midpoint_vertex.x == later_vertex.x - 5 && dispose_stream_value(&midpoint_value) == 0;
  StreamValue source{}, checked{};
  passed = passed && get_new_stream_value(1, stream, 0, nullptr, 0, &source) == 0 &&
      set_keyframe_value(stream, 0, &source) == 0 &&
      get_new_keyframe_value(1, stream, 0, &checked) == 0 &&
      checked.value != source.value && delete_keyframe(stream, 0) != 0 &&
      dispose_stream_value(&checked) == 0 && dispose_stream_value(&source) == 0;
  StreamValue batch_value{};
  void* transaction = nullptr; int32_t staged_index = -1;
  passed = passed && start_add_keyframes(stream, &transaction) == 0 &&
      add_keyframes(transaction, 0, &cancelled, &staged_index) == 0 && staged_index == 0 &&
      end_add_keyframes(0, transaction) == 0 &&
      get_stream_num_keyframes(stream, &count) == 0 && count == 2 &&
      get_new_stream_value(1, stream, 0, nullptr, 0, &batch_value) == 0 &&
      start_add_keyframes(stream, &transaction) == 0 &&
      add_keyframes(transaction, 0, &batched, &staged_index) == 0 &&
      set_add_keyframe(transaction, staged_index, &batch_value) == 0 &&
      end_add_keyframes(1, transaction) == 0 &&
      dispose_stream_value(&batch_value) == 0 &&
      get_stream_num_keyframes(stream, &count) == 0 && count == 3 &&
      delete_keyframe(stream, 2) == 0 && delete_keyframe(stream, 1) == 0 &&
      delete_keyframe(stream, 0) == 0 && dispose_stream(stream) == 0 && dispose_mask(mask) == 0;
  const auto after = aexcompat::mask_runtime::snapshot();
  return passed && after.invalid_keyframe_operations == before.invalid_keyframe_operations + 8 &&
      after.keyframe_mutations == before.keyframe_mutations + 14 && lifetimes_balanced();
}

bool verify_dynamic_stream_tree_rejection() {
  const auto original_scene = g_mask_scene;
  const auto before = aexcompat::mask_runtime::snapshot();
  void* mask = nullptr; void* mask_root = nullptr; void* layer_root = nullptr;
  void* parade = nullptr; void* atom = nullptr; void* outline = nullptr;
  void* opacity = nullptr; void* parent = nullptr; void* added = nullptr;
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_dynamic_stream_for_mask(1, mask, &mask_root) != 0 ||
      get_new_dynamic_stream_for_layer(1, &g_layer, &layer_root) != 0 ||
      get_new_dynamic_stream_by_match_name(1, layer_root, "ADBE Mask Parade", &parade) != 0 ||
      get_new_dynamic_stream_by_index(1, parade, 0, &atom) != 0 ||
      get_new_dynamic_stream_by_match_name(1, atom, "ADBE Mask Shape", &outline) != 0 ||
      get_new_dynamic_stream_by_match_name(1, atom, "ADBE Mask Opacity", &opacity) != 0)
    return false;
  int32_t depth{}, grouping{}, count{}, index{}; char match_name[40]{};
  uint32_t flags{}; uint8_t boolean{};
  bool passed = get_dynamic_stream_depth(layer_root, &depth) == 0 && depth == 0 &&
      get_dynamic_stream_grouping_type(parade, &grouping) == 0 && grouping == 2 &&
      get_num_streams_in_group(atom, &count) == 0 && count == 4 &&
      get_dynamic_match_name(outline, match_name) == 0 &&
      std::strcmp(match_name, "ADBE Mask Shape") == 0 &&
      get_dynamic_stream_index(atom, &index) == 0 && index == 0 &&
      get_new_parent_dynamic_stream(1, outline, &parent) == 0 &&
      get_dynamic_stream_grouping_type(parent, &grouping) == 0 && grouping == 1 &&
      set_dynamic_stream_flag(outline, 2, 0, 1) == 0 &&
      get_dynamic_stream_flags(outline, &flags) == 0 && flags == 2 &&
      set_dynamic_stream_flag(outline, 1, 0, 1) != 0 &&
      is_separation_leader(opacity, &boolean) == 0 && boolean == 0;
  StreamValue opacity_value{};
  passed = passed && get_new_stream_value(1, opacity, 0, nullptr, 0, &opacity_value) == 0 &&
      opacity_value.one_d == 100.0;
  opacity_value.one_d = 75.0;
  passed = passed && set_stream_value(1, opacity, &opacity_value) == 0 &&
      dispose_stream_value(&opacity_value) == 0 &&
      can_add_dynamic_stream(parade, "ADBE Mask Atom", &boolean) == 0 && boolean == 1 &&
      add_dynamic_stream(1, parade, "ADBE Mask Atom", &added) == 0;
  int32_t duplicate_index = -1;
  passed = passed && duplicate_dynamic_stream(1, atom, &duplicate_index) == 0 &&
      duplicate_index == 2 && reorder_dynamic_stream(atom, 2) == 0 &&
      get_dynamic_stream_index(atom, &index) == 0 && index == 2;
  const uint16_t renamed[]{'R','e','n','a','m','e','d',0};
  passed = passed && set_dynamic_stream_name(atom, renamed) == 0 &&
      get_dynamic_stream_modified(atom, &boolean) == 0 && boolean == 1;
  void* duplicate = nullptr;
  passed = passed && get_new_dynamic_stream_by_index(1, parade, 1, &duplicate) == 0 &&
      delete_dynamic_stream(duplicate) == 0 && dispose_stream(duplicate) == 0 &&
      delete_dynamic_stream(added) == 0 && dispose_stream(added) == 0 &&
      get_num_streams_in_group(parade, &count) == 0 && count == 1 &&
      get_dynamic_stream_index(atom, &index) == 0 && index == 0;
  passed = passed && dispose_stream(parent) == 0 && dispose_stream(opacity) == 0 &&
      dispose_stream(outline) == 0 && dispose_stream(atom) == 0 &&
      dispose_stream(parade) == 0 && dispose_stream(layer_root) == 0 &&
      dispose_stream(mask_root) == 0 && dispose_mask(mask) == 0;
  const bool balanced = lifetimes_balanced();
  g_mask_scene = original_scene; g_mask_scene.reserve(kMaxHostMasks);
  const auto after = aexcompat::mask_runtime::snapshot();
  return passed && balanced && after.dynamic_stream_mutations == before.dynamic_stream_mutations + 8 &&
      after.invalid_dynamic_stream_operations == before.invalid_dynamic_stream_operations + 1;
}

bool verify_mask_double_dispose_rejected() {
  void* mask = nullptr;
  return get_layer_mask_by_index(&g_layer, 0, &mask) == 0 &&
      dispose_mask(mask) == 0 && dispose_mask(mask) == 4 &&
      lifetimes_balanced();
}

bool verify_stream_dispose_with_live_value_rejected() {
  void* mask = nullptr;
  void* stream = nullptr;
  StreamValue value{};
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_mask_stream(1, mask, 400, &stream) != 0 ||
      get_new_stream_value(1, stream, 0, nullptr, 0, &value) != 0)
    return false;
  const int32_t premature_error = dispose_stream(stream);
  return premature_error == 4 && dispose_stream_value(&value) == 0 &&
      dispose_stream(stream) == 0 && dispose_mask(mask) == 0 &&
      lifetimes_balanced();
}

bool verify_stream_metadata_and_ownership_rejection() {
  const auto before = aexcompat::mask_runtime::snapshot();
  void* mask = nullptr;
  void* stream = nullptr;
  void* duplicate = nullptr;
  void* rejected = reinterpret_cast<void*>(1);
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_mask_stream(1, mask, 400, &stream) != 0 ||
      get_new_mask_stream(1, mask, 999, &rejected) == 0 || rejected != nullptr ||
      duplicate_stream_ref(1, stream, &duplicate) != 0)
    return false;
  uint8_t boolean{};
  int32_t interpolations{}, flags{}, type{}, id{}, duplicate_id{};
  double minimum = -1, maximum = -1;
  char units[32]{'x'};
  StreamValue first{}, second{};
  bool passed = can_vary_over_time(stream, &boolean) == 0 && boolean == 1 &&
      get_valid_interpolations(stream, &interpolations) == 0 && interpolations == 0xffff &&
      get_stream_units_text(stream, 0, units) == 0 && units[0] == '\0' &&
      get_stream_properties(stream, &flags, &minimum, &maximum) == 0 && flags == 0 &&
      minimum == 0 && maximum == 0 && is_stream_timevarying(stream, &boolean) == 0 &&
      boolean == 0 && get_stream_type(stream, &type) == 0 && type == 11 &&
      get_unique_stream_id(stream, &id) == 0 &&
      get_unique_stream_id(duplicate, &duplicate_id) == 0 && duplicate_id == id &&
      get_expression_state(1, stream, &boolean) == 0 && boolean == 0 &&
      get_new_stream_value(1, stream, 0, nullptr, 0, &first) == 0 &&
      get_new_stream_value(1, duplicate, 0, nullptr, 0, &second) == 0 &&
      first.value != second.value;
  HostMask* host_mask = find_mask(mask);
  OutlineData* first_outline = find_outline(first.value);
  OutlineData* second_outline = find_outline(second.value);
  const double original_x = host_mask && !host_mask->vertices.empty() ? host_mask->vertices[0].x : 0;
  MaskVertex edited_vertex{};
  if (first_outline && !first_outline->vertices.empty()) {
    edited_vertex = first_outline->vertices[0];
    edited_vertex.x += 3;
  }
  passed = passed && host_mask && first_outline && second_outline &&
      set_mask_outline_vertex_info(first.value, 0, &edited_vertex) == 0 &&
      host_mask->vertices[0].x == original_x &&
      set_stream_value(1, stream, &first) == 0 &&
      host_mask->vertices[0].x == original_x + 3 &&
      second_outline->vertices[0].x == original_x;
  const OutlineData committed = static_cast<const OutlineData&>(*host_mask);
  if (second_outline && !second_outline->vertices.empty())
    second_outline->vertices[0].x = std::numeric_limits<double>::quiet_NaN();
  StreamValue forged = second;
  passed = passed && set_stream_value(1, duplicate, &second) != 0 &&
      static_cast<const OutlineData&>(*host_mask).vertices[0].x == committed.vertices[0].x &&
      set_stream_value(1, stream, &second) != 0 &&
      set_stream_value(1, stream, &forged) != 0 &&
      set_stream_value(1, nullptr, &first) != 0 &&
      set_stream_value(2, stream, &first) != 0 &&
      dispose_stream_value(&first) == 0 &&
      set_stream_value(1, stream, &first) != 0 &&
      dispose_stream_value(&first) != 0;
  host_mask->keyframes.push_back(snapshot_keyframe(*host_mask, HostTime{0, 1}));
  passed = passed && set_stream_value(1, duplicate, &second) != 0;
  host_mask->keyframes.clear();
  passed = passed && dispose_stream_value(&second) == 0 &&
      dispose_stream_value(&second) != 0 &&
      dispose_stream(duplicate) == 0 && dispose_stream(stream) == 0 &&
      dispose_mask(mask) == 0;
  const auto after = aexcompat::mask_runtime::snapshot();
  return passed && after.invalid_stream_operations >= before.invalid_stream_operations + 9 &&
      after.stream_metadata_queries == before.stream_metadata_queries + 9 &&
      after.stream_duplicates == before.stream_duplicates + 1 && lifetimes_balanced();
}

bool verify_outline_mutation_rejection() {
  if (g_mask_scene.empty()) return false;
  HostMask& mask = g_mask_scene.front();
  const HostMask original = mask;
  const auto before = aexcompat::mask_runtime::snapshot();
  void* outline = &mask.outline;
  int32_t segments = -1;
  MaskVertex replacement{11, 2, -1, 0, 1, 0};
  MaskVertex observed{};
  MaskFeather feather{1, 0.25, 3.0, 0.5f, 0.75f, 0, 0};
  int32_t feather_index = -1;
  int32_t feather_count = -1;
  bool passed = set_mask_outline_open(outline, 1) == 0 &&
      get_mask_outline_num_segments(outline, &segments) == 0 && segments == 3 &&
      set_mask_outline_open(outline, 0) == 0 &&
      get_mask_outline_num_segments(outline, &segments) == 0 && segments == 4 &&
      set_mask_outline_vertex_info(outline, 1, &replacement) == 0 &&
      get_mask_outline_vertex_info(outline, 1, &observed) == 0 && observed.x == 11 &&
      create_mask_outline_vertex(outline, 2) == 0 &&
      get_mask_outline_num_segments(outline, &segments) == 0 && segments == 5 &&
      delete_mask_outline_vertex(outline, 2) == 0 &&
      create_mask_outline_feather(outline, &feather, &feather_index) == 0 &&
      feather_index == 0 && get_mask_outline_num_feathers(outline, &feather_count) == 0 &&
      feather_count == 1;
  feather.radius = 2.0;
  passed = passed && set_mask_outline_feather_info(outline, 0, &feather) == 0 &&
      get_mask_outline_feather_info(outline, 0, &feather) == 0 && feather.radius == 2.0;
  MaskFeather invalid = feather;
  invalid.radius = -1.0;
  passed = passed && set_mask_outline_feather_info(outline, 0, &invalid) != 0 &&
      delete_mask_outline_feather(outline, 0) == 0;
  mask = original;
  const auto after = aexcompat::mask_runtime::snapshot();
  return passed && after.invalid_outline_operations == before.invalid_outline_operations + 1 &&
      after.outline_mutations == before.outline_mutations + 8;
}

bool verify_mask_attribute_and_ownership_rejection() {
  const auto original_scene = g_mask_scene;
  const auto before = aexcompat::mask_runtime::snapshot();
  void* original = nullptr;
  if (get_layer_mask_by_index(&g_layer, 0, &original) != 0) return false;
  const double color[4]{1.0, 0.2, 0.4, 0.6};
  double observed_color[4]{};
  uint8_t byte_value{};
  int32_t long_value{};
  bool passed = set_mask_invert(original, 1) == 0 &&
      get_mask_invert(original, &byte_value) == 0 && byte_value == 1 &&
      set_mask_mode(original, 3) == 0 && get_mask_mode(original, &long_value) == 0 &&
      long_value == 3 && set_mask_motion_blur(original, 2) == 0 &&
      get_mask_motion_blur(original, &byte_value) == 0 && byte_value == 2 &&
      set_mask_feather_falloff(original, 1) == 0 &&
      get_mask_feather_falloff(original, &byte_value) == 0 && byte_value == 1 &&
      set_mask_color(original, color) == 0 && get_mask_color(original, observed_color) == 0 &&
      std::equal(std::begin(color), std::end(color), std::begin(observed_color)) &&
      set_mask_lock(original, 1) == 0 && get_mask_lock(original, &byte_value) == 0 &&
      byte_value == 1 && set_mask_roto_bezier(original, 1) == 0 &&
      get_mask_roto_bezier(original, &byte_value) == 0 && byte_value == 1 &&
      set_mask_mode(original, 99) != 0;
  int32_t original_id{}, duplicate_id{}, count{};
  void* duplicate = nullptr;
  passed = passed && get_mask_id(original, &original_id) == 0 &&
      duplicate_mask(original, &duplicate) == 0 &&
      get_mask_id(duplicate, &duplicate_id) == 0 && duplicate_id != original_id &&
      get_layer_num_masks(&g_layer, &count) == 0 && count == 2 &&
      delete_mask_from_layer(duplicate) == 0 && dispose_mask(duplicate) == 0 &&
      get_layer_num_masks(&g_layer, &count) == 0 && count == 1;
  void* created = nullptr;
  int32_t created_index = -1;
  passed = passed && create_new_mask(&g_layer, &created, &created_index) == 0 &&
      created_index == 1 && delete_mask_from_layer(created) == 0 &&
      dispose_mask(created) == 0 && dispose_mask(original) == 0;
  const bool balanced = lifetimes_balanced();
  g_mask_scene = original_scene;
  g_mask_scene.reserve(kMaxHostMasks);
  const auto after = aexcompat::mask_runtime::snapshot();
  return passed && after.invalid_mask_operations == before.invalid_mask_operations + 1 &&
      after.mask_mutations == before.mask_mutations + 11 && balanced;
}

}  // namespace aexcompat::l2_detail
