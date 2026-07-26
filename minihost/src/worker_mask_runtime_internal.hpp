#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <list>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

#include "worker_aegp_scene_model.hpp"

namespace aexcompat::l2_detail {

struct OpaqueHostObject { uint32_t tag; };
struct MaskVertex { double x, y, tangent_in_x, tangent_in_y, tangent_out_x, tangent_out_y; };
struct MaskFeather {
  int32_t segment{}; double segment_s{}; double radius{};
  float ui_corner_angle{}; float tension{}; uint8_t interp{}; uint8_t type{};
};
struct OutlineData {
  OpaqueHostObject outline{0x4f55544c}; bool open{};
  std::vector<MaskVertex> vertices; std::vector<MaskFeather> feathers;
};
struct HostTime { int32_t value{}; uint32_t scale{1}; };
struct KeyframeEase { double speed; double influence; };
inline constexpr int16_t kHostTemporalDimensions = 1;
struct HostKeyframe : OutlineData {
  HostTime time{}; int32_t flags{}; int32_t in_interpolation{1};
  int32_t out_interpolation{1}; int32_t label{};
  OutlineData spatial_in; OutlineData spatial_out;
  std::array<KeyframeEase, kHostTemporalDimensions> temporal_in{};
  std::array<KeyframeEase, kHostTemporalDimensions> temporal_out{};
  aexcompat::scene_model::Identity identity{};
};
struct HostMask : OutlineData {
  OpaqueHostObject mask{0x4d41534b};
  bool mask_live{};
  bool stream_live{};
  bool value_live{};
  bool deleted{}, invert{}, locked{}, roto_bezier{};
  uint8_t motion_blur{}, feather_falloff{}; int32_t mode{1}, id{}, outline_stream_id{},
      feather_stream_id{}, opacity_stream_id{}, expansion_stream_id{}, dynamic_order{};
  double opacity{100.0}; std::array<double, 2> feather{}; double expansion{};
  std::u16string dynamic_name{u"Mask"}; std::array<uint32_t, 5> dynamic_flags{};
  bool dynamic_modified{}; std::array<std::u16string, 4> expressions;
  std::array<bool, 4> expression_enabled{};
  std::array<double, 4> color{1.0, 1.0, 0.0, 0.0}; std::list<HostKeyframe> keyframes;
};
enum class DynamicNodeKind { MaskOutline, LayerRoot, MaskParade, MaskAtom,
  MaskFeather, MaskOpacity, MaskExpansion };
struct HostStreamRef {
  OpaqueHostObject opaque{0x5354524d}; HostMask* mask{}; int32_t selector{}, unique_id{};
  uint32_t live_values{}; DynamicNodeKind kind{DynamicNodeKind::MaskOutline};
  int32_t owner_plugin_id{1};
  aexcompat::scene_model::Identity identity{};
  void* handle{};
};
struct StreamValue {
  void* stream; union { void* value; double one_d; double two_d[2]; std::byte raw_value[32]; };
};
struct CheckedStreamValue {
  HostStreamRef* stream{}; OutlineData* outline{}; HostKeyframe* source_keyframe{};
  std::unique_ptr<OutlineData> owned_outline;
  aexcompat::scene_model::Identity identity{};
};
struct MaskLifetimeCounts {
  uint32_t masks_acquired{}, masks_disposed{}, streams_acquired{}, streams_disposed{},
      values_acquired{}, values_disposed{};
};
struct AddKeyframesTransaction {
  OpaqueHostObject opaque{0x41444b46}; HostStreamRef* stream{}; std::vector<HostKeyframe> staged;
};

extern std::vector<HostMask> g_mask_scene;
extern OpaqueHostObject g_layer;
extern std::list<HostStreamRef> g_stream_refs;
extern std::unordered_map<StreamValue*, CheckedStreamValue> g_stream_values;
extern MaskLifetimeCounts g_mask_lifetime;
extern uint32_t g_invalid_outline_operations, g_outline_mutations, g_mask_mutations,
    g_invalid_mask_operations, g_invalid_stream_operations, g_stream_metadata_queries,
    g_stream_duplicates, g_keyframe_mutations, g_invalid_keyframe_operations,
    g_dynamic_stream_queries, g_dynamic_stream_mutations, g_invalid_dynamic_stream_operations,
    g_layer_dynamic_flags, g_mask_parade_dynamic_flags;
extern int32_t g_next_mask_id, g_next_stream_id;
inline constexpr std::size_t kMaxHostMasks = 8, kMaxOutlineVertices = 64,
    kMaxOutlineFeathers = 64, kMaxKeyframesPerStream = 64, kMaxCheckedStreamValues = 256;
extern std::list<AddKeyframesTransaction> g_add_keyframe_transactions;

HostMask* find_mask(void* handle);
HostStreamRef* find_stream(void* handle);
OutlineData* find_outline(void* handle);
std::vector<HostMask*> ordered_active_masks();
std::size_t active_mask_count();
void bump_render_project_timestamp();
std::size_t distinct_vertex_count(const OutlineData& mask);
void sync_closed_vertex(OutlineData& mask);
bool dynamic_leaf(DynamicNodeKind kind);
OutlineData* sampled_outline(HostStreamRef*, const HostTime*, std::unique_ptr<OutlineData>&);
int32_t create_stream_ref(HostMask*, DynamicNodeKind, int32_t, void**);
HostKeyframe* keyframe_at(HostStreamRef*, int32_t);
bool ensure_keyframe_identity(HostStreamRef*, HostKeyframe*, int32_t);
AddKeyframesTransaction* find_add_transaction(void*);
bool valid_time_mode(int16_t mode);
bool valid_stream_plugin(int32_t plugin_id);
int32_t __cdecl get_mask_outline_vertex_info(void*, int32_t, MaskVertex*);
int32_t __cdecl set_mask_outline_vertex_info(void*, int32_t, const MaskVertex*);
int32_t __cdecl is_mask_outline_open(void*, uint8_t*);
int32_t __cdecl set_mask_outline_open(void*, uint8_t);
int32_t __cdecl get_mask_outline_num_segments(void*, int32_t*);
int32_t __cdecl create_mask_outline_vertex(void*, int32_t);
int32_t __cdecl delete_mask_outline_vertex(void*, int32_t);
int32_t __cdecl get_mask_outline_num_feathers(void*, int32_t*);
int32_t __cdecl get_mask_outline_feather_info(void*, int32_t, MaskFeather*);
int32_t __cdecl set_mask_outline_feather_info(void*, int32_t, const MaskFeather*);
int32_t __cdecl create_mask_outline_feather(void*, const MaskFeather*, int32_t*);
int32_t __cdecl delete_mask_outline_feather(void*, int32_t);

int32_t __cdecl get_new_dynamic_stream_for_layer(int32_t, void*, void**);
int32_t __cdecl get_new_dynamic_stream_for_mask(int32_t, void*, void**);
int32_t __cdecl get_dynamic_stream_depth(void*, int32_t*);
int32_t __cdecl get_dynamic_stream_grouping_type(void*, int32_t*);
int32_t __cdecl get_num_streams_in_group(void*, int32_t*);
int32_t __cdecl get_dynamic_stream_flags(void*, uint32_t*);
int32_t __cdecl set_dynamic_stream_flag(void*, uint32_t, uint8_t, uint8_t);
int32_t __cdecl get_new_dynamic_stream_by_index(int32_t, void*, int32_t, void**);
int32_t __cdecl get_new_dynamic_stream_by_match_name(int32_t, void*, const char*, void**);
int32_t __cdecl delete_dynamic_stream(void*);
int32_t __cdecl reorder_dynamic_stream(void*, int32_t);
int32_t __cdecl duplicate_dynamic_stream(int32_t, void*, int32_t*);
int32_t __cdecl set_dynamic_stream_name(void*, const uint16_t*);
int32_t __cdecl can_add_dynamic_stream(void*, const char*, uint8_t*);
int32_t __cdecl add_dynamic_stream(int32_t, void*, const char*, void**);
int32_t __cdecl get_dynamic_match_name(void*, char*);
int32_t __cdecl get_new_parent_dynamic_stream(int32_t, void*, void**);
int32_t __cdecl get_dynamic_stream_modified(void*, uint8_t*);
int32_t __cdecl get_dynamic_stream_index(void*, int32_t*);
int32_t __cdecl is_separation_leader(void*, uint8_t*);
int32_t __cdecl are_dimensions_separated(void*, uint8_t*);
int32_t __cdecl reject_set_dimensions_separated(void*, uint8_t);
int32_t __cdecl reject_get_separation_follower(void*, int16_t, void**);
int32_t __cdecl is_separation_follower(void*, uint8_t*);
int32_t __cdecl reject_get_separation_leader(void*, void**);
int32_t __cdecl reject_get_separation_dimension(void*, int16_t*);

int32_t __cdecl get_layer_num_masks(void*, int32_t*);
int32_t __cdecl get_layer_mask_by_index(void*, int32_t, void**);
int32_t __cdecl dispose_mask(void*);
bool usable_mask(const HostMask*);
int32_t __cdecl get_mask_invert(void*, uint8_t*);
int32_t __cdecl set_mask_invert(void*, uint8_t);
int32_t __cdecl get_mask_mode(void*, int32_t*);
int32_t __cdecl set_mask_mode(void*, int32_t);
int32_t __cdecl get_mask_motion_blur(void*, uint8_t*);
int32_t __cdecl set_mask_motion_blur(void*, uint8_t);
int32_t __cdecl get_mask_feather_falloff(void*, uint8_t*);
int32_t __cdecl set_mask_feather_falloff(void*, uint8_t);
int32_t __cdecl get_mask_id(void*, int32_t*);
int32_t __cdecl create_new_mask(void*, void**, int32_t*);
int32_t __cdecl delete_mask_from_layer(void*);
int32_t __cdecl get_mask_color(void*, double*);
int32_t __cdecl set_mask_color(void*, const double*);
int32_t __cdecl get_mask_lock(void*, uint8_t*);
int32_t __cdecl set_mask_lock(void*, uint8_t);
int32_t __cdecl get_mask_roto_bezier(void*, uint8_t*);
int32_t __cdecl set_mask_roto_bezier(void*, uint8_t);
int32_t __cdecl duplicate_mask(void*, void**);
int32_t __cdecl get_new_mask_stream(int32_t, void*, int32_t, void**);
int32_t __cdecl dispose_stream(void*);
int32_t __cdecl get_new_stream_value(int32_t, void*, int32_t, const HostTime*, int32_t, StreamValue*);
int32_t __cdecl dispose_stream_value(StreamValue*);
int32_t __cdecl is_stream_legal(void*, int32_t, uint8_t*);
int32_t __cdecl can_vary_over_time(void*, uint8_t*);
int32_t __cdecl get_valid_interpolations(void*, int32_t*);
int32_t __cdecl unsupported_new_layer_stream(int32_t, void*, int32_t, void**);
int32_t __cdecl unsupported_effect_stream_count(void*, int32_t*);
int32_t __cdecl unsupported_new_effect_stream(int32_t, void*, int32_t, void**);
int32_t __cdecl unsupported_stream_name(int32_t, void*, uint8_t, void**);
int32_t __cdecl get_stream_units_text(void*, uint8_t, char*);
int32_t __cdecl get_stream_properties(void*, int32_t*, double*, double*);
int32_t __cdecl is_stream_timevarying(void*, uint8_t*);
int32_t __cdecl get_stream_type(void*, int32_t*);
int32_t __cdecl set_stream_value(int32_t, void*, StreamValue*);
int32_t __cdecl unsupported_layer_stream_value(void*, int32_t, int32_t, const void*, uint8_t, void*, int32_t*);
int32_t __cdecl get_expression_state(int32_t, void*, uint8_t*);
int32_t __cdecl reject_expression_state(int32_t, void*, uint8_t);
int32_t __cdecl unsupported_get_expression(int32_t, void*, void**);
int32_t __cdecl unsupported_set_expression(int32_t, void*, const uint16_t*);
int32_t __cdecl duplicate_stream_ref(int32_t, void*, void**);
int32_t __cdecl get_unique_stream_id(void*, int32_t*);
bool time_equal(const HostTime&, const HostTime&);
HostKeyframe snapshot_keyframe(const HostMask&, const HostTime&);
int32_t __cdecl get_stream_num_keyframes(void*, int32_t*);
int32_t __cdecl get_keyframe_time(void*, int32_t, int16_t, HostTime*);
int32_t __cdecl insert_keyframe(void*, int16_t, const HostTime*, int32_t*);
int32_t __cdecl delete_keyframe(void*, int32_t);
int32_t __cdecl get_new_keyframe_value(int32_t, void*, int32_t, StreamValue*);
int32_t __cdecl set_keyframe_value(void*, int32_t, const StreamValue*);
int32_t __cdecl get_stream_value_dimensionality(void*, int16_t*);
int32_t __cdecl get_stream_temporal_dimensionality(void*, int16_t*);
int32_t __cdecl get_new_keyframe_spatial_tangents(int32_t, void*, int32_t, StreamValue*, StreamValue*);
int32_t __cdecl set_keyframe_spatial_tangents(void*, int32_t, const StreamValue*, const StreamValue*);
int32_t __cdecl get_keyframe_temporal_ease(void*, int32_t, int32_t, KeyframeEase*, KeyframeEase*);
int32_t __cdecl set_keyframe_temporal_ease(void*, int32_t, int32_t, const KeyframeEase*, const KeyframeEase*);
int32_t __cdecl get_keyframe_flags(void*, int32_t, int32_t*);
int32_t __cdecl set_keyframe_flag(void*, int32_t, int32_t, uint8_t);
int32_t __cdecl get_keyframe_interpolation(void*, int32_t, int32_t*, int32_t*);
int32_t __cdecl set_keyframe_interpolation(void*, int32_t, int32_t, int32_t);
int32_t __cdecl start_add_keyframes(void*, void**);
int32_t __cdecl add_keyframes(void*, int16_t, const HostTime*, int32_t*);
int32_t __cdecl set_add_keyframe(void*, int32_t, const StreamValue*);
int32_t __cdecl end_add_keyframes(uint8_t, void*);
int32_t __cdecl get_keyframe_label(void*, int32_t, int32_t*);
int32_t __cdecl set_keyframe_label(void*, int32_t, int32_t);

}  // namespace aexcompat::l2_detail
