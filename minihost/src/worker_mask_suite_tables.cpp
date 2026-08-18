#include "worker_mask_suite_tables.hpp"

#include "worker_mask_runtime_internal.hpp"
#include "worker_pf_path_runtime.hpp"

#include <cstdint>
#include <cstring>

namespace aexcompat::l2_detail {

// Mirrors the l2_main.cpp preamble that worker_l2_suite_abi.hpp relies on:
// the ABI header captures these signatures with decltype before the tables
// are defined.
struct LegacyRect { int32_t left, top, right, bottom; };
int32_t __cdecl pf_mask_world_with_path(void* effect_ref, void** path, double feather_x,
                                        double feather_y, int32_t invert, double opacity,
                                        int32_t quality, void* world, LegacyRect* bounds);
#include "worker_l2_suite_abi.hpp"

PfMaskSuite1 g_pf_mask_suite1{
    reinterpret_cast<decltype(PfMaskSuite1::mask_world_with_path)>(
        &aexcompat::pf_path_runtime::mask_world_with_path)};

MaskSuite g_mask_suite{&get_layer_num_masks, &get_layer_mask_by_index, &dispose_mask,
    &get_mask_invert, &set_mask_invert, &get_mask_mode, &set_mask_mode,
    &get_mask_motion_blur, &set_mask_motion_blur,
    &get_mask_feather_falloff, &set_mask_feather_falloff, &get_mask_id,
    &create_new_mask, &delete_mask_from_layer, &get_mask_color, &set_mask_color,
    &get_mask_lock, &set_mask_lock, &get_mask_roto_bezier, &set_mask_roto_bezier,
    &duplicate_mask};
MaskSuite5 g_mask_suite5{&get_layer_num_masks, &get_layer_mask_by_index, &dispose_mask,
    &get_mask_invert, &set_mask_invert, &get_mask_mode, &set_mask_mode,
    &get_mask_motion_blur, &set_mask_motion_blur, &get_mask_id,
    &create_new_mask, &delete_mask_from_layer, &get_mask_color, &set_mask_color,
    &get_mask_lock, &set_mask_lock, &get_mask_roto_bezier, &set_mask_roto_bezier,
    &duplicate_mask};
StreamSuite g_stream_suite{&is_stream_legal, &can_vary_over_time,
    &get_valid_interpolations, &unsupported_new_layer_stream,
    &unsupported_effect_stream_count, &unsupported_new_effect_stream,
    &get_new_mask_stream, &dispose_stream, &unsupported_stream_name,
    &get_stream_units_text, &get_stream_properties, &is_stream_timevarying,
    &get_stream_type, &get_new_stream_value, &dispose_stream_value,
    &set_stream_value, &unsupported_layer_stream_value,
    &get_expression_state, &reject_expression_state, &unsupported_get_expression,
    &unsupported_set_expression, &duplicate_stream_ref, &get_unique_stream_id};
StreamSuite4 g_stream_suite4{&is_stream_legal, &can_vary_over_time,
    &get_valid_interpolations, &unsupported_new_layer_stream,
    &unsupported_effect_stream_count, &unsupported_new_effect_stream,
    &get_new_mask_stream, &dispose_stream, &unsupported_stream_name,
    &get_stream_units_text, &get_stream_properties, &is_stream_timevarying,
    &get_stream_type, &get_new_stream_value, &dispose_stream_value,
    &set_stream_value, &unsupported_layer_stream_value,
    &get_expression_state, &reject_expression_state,
    &reject_get_expression_ansi, &reject_set_expression_ansi,
    &duplicate_stream_ref};
KeyframeSuite g_keyframe_suite{&get_stream_num_keyframes, &get_keyframe_time,
    &insert_keyframe, &delete_keyframe, &get_new_keyframe_value,
    &set_keyframe_value, &get_stream_value_dimensionality,
    &get_stream_temporal_dimensionality, &get_new_keyframe_spatial_tangents,
    &set_keyframe_spatial_tangents, &get_keyframe_temporal_ease,
    &set_keyframe_temporal_ease, &get_keyframe_flags, &set_keyframe_flag,
    &get_keyframe_interpolation, &set_keyframe_interpolation,
    &start_add_keyframes, &add_keyframes, &set_add_keyframe,
    &end_add_keyframes, &get_keyframe_label, &set_keyframe_label};
KeyframeSuite4 g_keyframe_suite4{&get_stream_num_keyframes, &get_keyframe_time,
    &insert_keyframe, &delete_keyframe, &get_new_keyframe_value,
    &set_keyframe_value, &get_stream_value_dimensionality,
    &get_stream_temporal_dimensionality, &get_new_keyframe_spatial_tangents,
    &set_keyframe_spatial_tangents, &get_keyframe_temporal_ease,
    &set_keyframe_temporal_ease, &get_keyframe_flags, &set_keyframe_flag,
    &get_keyframe_interpolation, &set_keyframe_interpolation,
    &start_add_keyframes, &add_keyframes, &set_add_keyframe,
    &end_add_keyframes};
// Same TU, defined after the v4 table it copies, so the initialization order is
// fixed. See the header for why v3 and v4 are the same table on x64.
KeyframeSuite4 g_keyframe_suite3 = g_keyframe_suite4;

bool keyframe_suite5_abi_wiring_valid() {
  const KeyframeSuite expected{&get_stream_num_keyframes, &get_keyframe_time,
      &insert_keyframe, &delete_keyframe, &get_new_keyframe_value,
      &set_keyframe_value, &get_stream_value_dimensionality,
      &get_stream_temporal_dimensionality, &get_new_keyframe_spatial_tangents,
      &set_keyframe_spatial_tangents, &get_keyframe_temporal_ease,
      &set_keyframe_temporal_ease, &get_keyframe_flags, &set_keyframe_flag,
      &get_keyframe_interpolation, &set_keyframe_interpolation,
      &start_add_keyframes, &add_keyframes, &set_add_keyframe,
      &end_add_keyframes, &get_keyframe_label, &set_keyframe_label};
  return std::memcmp(&g_keyframe_suite, &expected, sizeof(expected)) == 0;
}

DynamicStreamSuite g_dynamic_stream_suite{&get_new_dynamic_stream_for_layer,
    &get_new_dynamic_stream_for_mask, &get_dynamic_stream_depth,
    &get_dynamic_stream_grouping_type, &get_num_streams_in_group,
    &get_dynamic_stream_flags, &set_dynamic_stream_flag,
    &get_new_dynamic_stream_by_index, &get_new_dynamic_stream_by_match_name,
    &delete_dynamic_stream, &reorder_dynamic_stream, &duplicate_dynamic_stream,
    &set_dynamic_stream_name, &can_add_dynamic_stream, &add_dynamic_stream,
    &get_dynamic_match_name, &get_new_parent_dynamic_stream,
    &get_dynamic_stream_modified, &get_dynamic_stream_index,
    &is_separation_leader, &are_dimensions_separated,
    &reject_set_dimensions_separated, &reject_get_separation_follower,
    &is_separation_follower, &reject_get_separation_leader,
    &reject_get_separation_dimension};
MaskOutlineSuite g_mask_outline_suite{&is_mask_outline_open, &set_mask_outline_open,
                                      &get_mask_outline_num_segments,
                                      &get_mask_outline_vertex_info,
                                      &set_mask_outline_vertex_info,
                                      &create_mask_outline_vertex,
                                      &delete_mask_outline_vertex,
                                      &get_mask_outline_num_feathers,
                                      &get_mask_outline_feather_info,
                                      &set_mask_outline_feather_info,
                                      &create_mask_outline_feather,
                                      &delete_mask_outline_feather};

}  // namespace aexcompat::l2_detail
