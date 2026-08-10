#pragma once

extern "C" {
int32_t __cdecl set_options_button_name(void*, const char*);
int32_t __cdecl get_layer_channel_count(void*, int32_t, int32_t*);
int32_t __cdecl get_layer_channel_indexed(void*, int32_t, int32_t, uint8_t*, void*, void*);
int32_t __cdecl get_layer_channel_typed(void*, int32_t, int32_t, uint8_t*, void*, void*);
int32_t __cdecl checkout_layer_channel(void*, void*, int32_t, int32_t, uint32_t, int32_t, void*);
int32_t __cdecl checkin_layer_channel(void*, void*, void*);
void reclaim_layer_channels();
int32_t __cdecl duck_quack(uint16_t);
int32_t __cdecl abort_render(void*);
int32_t __cdecl report_progress(void*, int32_t, int32_t);
int32_t __cdecl register_custom_ui(void*, const void*);
int32_t __cdecl adv_app_info_text(const char*, const char*);
int32_t __cdecl adv_app_info_text3(const char*, const char*, const char*);
int32_t __cdecl adv_app_info_text3_plus(const char*, const char*, const char*,
                                        const char*, const char*);
}
double __cdecl ansi_atan(double);
double __cdecl ansi_atan2(double, double);
double __cdecl ansi_ceil(double);
double __cdecl ansi_cos(double);
double __cdecl ansi_exp(double);
double __cdecl ansi_fabs(double);
double __cdecl ansi_floor(double);
double __cdecl ansi_fmod(double, double);
double __cdecl ansi_hypot(double, double);
double __cdecl ansi_log(double);
double __cdecl ansi_log10(double);
double __cdecl ansi_pow(double, double);
double __cdecl ansi_sin(double);
double __cdecl ansi_sqrt(double);
double __cdecl ansi_tan(double);
int __cdecl ansi_sprintf(char*, const char*, ...);
char* __cdecl ansi_strcpy(char*, const char*);
double __cdecl ansi_asin(double);
double __cdecl ansi_acos(double);

// L2-owned AEGP callback table declarations.  The PF callback implementations
// live in worker_pf_suites.cpp; these types remain beside the L2 scene model
// because their function signatures use its private callback declarations.
struct PfMaskSuite1 {
  decltype(&pf_mask_world_with_path) mask_world_with_path;
};

struct MaskSuite {
  decltype(&get_layer_num_masks) get_layer_num_masks;
  decltype(&get_layer_mask_by_index) get_layer_mask_by_index;
  decltype(&dispose_mask) dispose_mask;
  decltype(&get_mask_invert) get_invert;
  decltype(&set_mask_invert) set_invert;
  decltype(&get_mask_mode) get_mode;
  decltype(&set_mask_mode) set_mode;
  decltype(&get_mask_motion_blur) get_motion_blur;
  decltype(&set_mask_motion_blur) set_motion_blur;
  decltype(&get_mask_feather_falloff) get_feather_falloff;
  decltype(&set_mask_feather_falloff) set_feather_falloff;
  decltype(&get_mask_id) get_id;
  decltype(&create_new_mask) create_new;
  decltype(&delete_mask_from_layer) delete_from_layer;
  decltype(&get_mask_color) get_color;
  decltype(&set_mask_color) set_color;
  decltype(&get_mask_lock) get_lock;
  decltype(&set_mask_lock) set_lock;
  decltype(&get_mask_roto_bezier) get_roto_bezier;
  decltype(&set_mask_roto_bezier) set_roto_bezier;
  decltype(&duplicate_mask) duplicate;
};

struct MaskSuite5 {
  decltype(&get_layer_num_masks) get_layer_num_masks;
  decltype(&get_layer_mask_by_index) get_layer_mask_by_index;
  decltype(&dispose_mask) dispose_mask;
  decltype(&get_mask_invert) get_invert;
  decltype(&set_mask_invert) set_invert;
  decltype(&get_mask_mode) get_mode;
  decltype(&set_mask_mode) set_mode;
  decltype(&get_mask_motion_blur) get_motion_blur;
  decltype(&set_mask_motion_blur) set_motion_blur;
  decltype(&get_mask_id) get_id;
  decltype(&create_new_mask) create_new;
  decltype(&delete_mask_from_layer) delete_from_layer;
  decltype(&get_mask_color) get_color;
  decltype(&set_mask_color) set_color;
  decltype(&get_mask_lock) get_lock;
  decltype(&set_mask_lock) set_lock;
  decltype(&get_mask_roto_bezier) get_roto_bezier;
  decltype(&set_mask_roto_bezier) set_roto_bezier;
  decltype(&duplicate_mask) duplicate;
};

struct StreamSuite {
  decltype(&is_stream_legal) is_stream_legal;
  decltype(&can_vary_over_time) can_vary_over_time;
  decltype(&get_valid_interpolations) get_valid_interpolations;
  decltype(&unsupported_new_layer_stream) get_new_layer_stream;
  decltype(&unsupported_effect_stream_count) get_effect_num_param_streams;
  decltype(&unsupported_new_effect_stream) get_new_effect_stream_by_index;
  decltype(&get_new_mask_stream) get_new_mask_stream;
  decltype(&dispose_stream) dispose_stream;
  decltype(&unsupported_stream_name) get_stream_name;
  decltype(&get_stream_units_text) get_stream_units_text;
  decltype(&get_stream_properties) get_stream_properties;
  decltype(&is_stream_timevarying) is_stream_timevarying;
  decltype(&get_stream_type) get_stream_type;
  decltype(&get_new_stream_value) get_new_stream_value;
  decltype(&dispose_stream_value) dispose_stream_value;
  decltype(&set_stream_value) set_stream_value;
  decltype(&unsupported_layer_stream_value) get_layer_stream_value;
  decltype(&get_expression_state) get_expression_state;
  decltype(&reject_expression_state) set_expression_state;
  decltype(&unsupported_get_expression) get_expression;
  decltype(&unsupported_set_expression) set_expression;
  decltype(&duplicate_stream_ref) duplicate_stream_ref;
  decltype(&get_unique_stream_id) get_unique_stream_id;
};
static_assert(sizeof(StreamSuite) == 23 * sizeof(void*));

// AEGP_StreamSuite4, acquired as numeric version 9 (frozen in AE 9).  Its
// first 19 slots match the current suite.  Expression text is the legacy
// A_char ABI at slots 19/20, and DuplicateStreamRef is its final slot.
struct StreamSuite4 {
  decltype(&is_stream_legal) is_stream_legal;
  decltype(&can_vary_over_time) can_vary_over_time;
  decltype(&get_valid_interpolations) get_valid_interpolations;
  decltype(&unsupported_new_layer_stream) get_new_layer_stream;
  decltype(&unsupported_effect_stream_count) get_effect_num_param_streams;
  decltype(&unsupported_new_effect_stream) get_new_effect_stream_by_index;
  decltype(&get_new_mask_stream) get_new_mask_stream;
  decltype(&dispose_stream) dispose_stream;
  decltype(&unsupported_stream_name) get_stream_name;
  decltype(&get_stream_units_text) get_stream_units_text;
  decltype(&get_stream_properties) get_stream_properties;
  decltype(&is_stream_timevarying) is_stream_timevarying;
  decltype(&get_stream_type) get_stream_type;
  decltype(&get_new_stream_value) get_new_stream_value;
  decltype(&dispose_stream_value) dispose_stream_value;
  decltype(&set_stream_value) set_stream_value;
  decltype(&unsupported_layer_stream_value) get_layer_stream_value;
  decltype(&get_expression_state) get_expression_state;
  decltype(&reject_expression_state) set_expression_state;
  decltype(&reject_get_expression_ansi) get_expression;
  decltype(&reject_set_expression_ansi) set_expression;
  decltype(&duplicate_stream_ref) duplicate_stream_ref;
};
static_assert(sizeof(StreamSuite4) == 22 * sizeof(void*));

struct KeyframeSuite {
  decltype(&get_stream_num_keyframes) get_stream_num_keyframes;
  decltype(&get_keyframe_time) get_keyframe_time;
  decltype(&insert_keyframe) insert_keyframe;
  decltype(&delete_keyframe) delete_keyframe;
  decltype(&get_new_keyframe_value) get_new_keyframe_value;
  decltype(&set_keyframe_value) set_keyframe_value;
  decltype(&get_stream_value_dimensionality) get_stream_value_dimensionality;
  decltype(&get_stream_temporal_dimensionality) get_stream_temporal_dimensionality;
  decltype(&get_new_keyframe_spatial_tangents) get_new_keyframe_spatial_tangents;
  decltype(&set_keyframe_spatial_tangents) set_keyframe_spatial_tangents;
  decltype(&get_keyframe_temporal_ease) get_keyframe_temporal_ease;
  decltype(&set_keyframe_temporal_ease) set_keyframe_temporal_ease;
  decltype(&get_keyframe_flags) get_keyframe_flags;
  decltype(&set_keyframe_flag) set_keyframe_flag;
  decltype(&get_keyframe_interpolation) get_keyframe_interpolation;
  decltype(&set_keyframe_interpolation) set_keyframe_interpolation;
  decltype(&start_add_keyframes) start_add_keyframes;
  decltype(&add_keyframes) add_keyframes;
  decltype(&set_add_keyframe) set_add_keyframe;
  decltype(&end_add_keyframes) end_add_keyframes;
  decltype(&get_keyframe_label) get_keyframe_label_color_index;
  decltype(&set_keyframe_label) set_keyframe_label_color_index;
};
static_assert(sizeof(KeyframeSuite) == 22 * sizeof(void*));

struct DynamicStreamSuite {
  decltype(&get_new_dynamic_stream_for_layer) get_new_stream_ref_for_layer;
  decltype(&get_new_dynamic_stream_for_mask) get_new_stream_ref_for_mask;
  decltype(&get_dynamic_stream_depth) get_stream_depth;
  decltype(&get_dynamic_stream_grouping_type) get_stream_grouping_type;
  decltype(&get_num_streams_in_group) get_num_streams_in_group;
  decltype(&get_dynamic_stream_flags) get_dynamic_stream_flags;
  decltype(&set_dynamic_stream_flag) set_dynamic_stream_flag;
  decltype(&get_new_dynamic_stream_by_index) get_new_stream_ref_by_index;
  decltype(&get_new_dynamic_stream_by_match_name) get_new_stream_ref_by_match_name;
  decltype(&delete_dynamic_stream) delete_stream;
  decltype(&reorder_dynamic_stream) reorder_stream;
  decltype(&duplicate_dynamic_stream) duplicate_stream;
  decltype(&set_dynamic_stream_name) set_stream_name;
  decltype(&can_add_dynamic_stream) can_add_stream;
  decltype(&add_dynamic_stream) add_stream;
  decltype(&get_dynamic_match_name) get_match_name;
  decltype(&get_new_parent_dynamic_stream) get_new_parent_stream_ref;
  decltype(&get_dynamic_stream_modified) get_stream_is_modified;
  decltype(&get_dynamic_stream_index) get_stream_index_in_parent;
  decltype(&is_separation_leader) is_separation_leader;
  decltype(&are_dimensions_separated) are_dimensions_separated;
  decltype(&reject_set_dimensions_separated) set_dimensions_separated;
  decltype(&reject_get_separation_follower) get_separation_follower;
  decltype(&is_separation_follower) is_separation_follower;
  decltype(&reject_get_separation_leader) get_separation_leader;
  decltype(&reject_get_separation_dimension) get_separation_dimension;
};
static_assert(sizeof(DynamicStreamSuite) == 26 * sizeof(void*));

struct MaskOutlineSuite {
  decltype(&is_mask_outline_open) is_open;
  decltype(&set_mask_outline_open) set_open;
  decltype(&get_mask_outline_num_segments) get_num_segments;
  decltype(&get_mask_outline_vertex_info) get_vertex_info;
  decltype(&set_mask_outline_vertex_info) set_vertex_info;
  decltype(&create_mask_outline_vertex) create_vertex;
  decltype(&delete_mask_outline_vertex) delete_vertex;
  decltype(&get_mask_outline_num_feathers) get_num_feathers;
  decltype(&get_mask_outline_feather_info) get_feather_info;
  decltype(&set_mask_outline_feather_info) set_feather_info;
  decltype(&create_mask_outline_feather) create_feather;
  decltype(&delete_mask_outline_feather) delete_feather;
};
