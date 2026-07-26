#pragma once
#include "worker_aegp_scene.hpp"
namespace aexcompat::l2_detail {
struct AegpCompatColor { double alpha, red, green, blue; };
struct AegpSceneModelSelftestReport {
  bool passed{};
  bool two_projects{};
  bool mask_fixture{};
  bool parent_camera_zoom_fixture{};
  bool typed_identity{};
  bool pointer_id_mismatch_rejected{};
  bool duplicate_stable_id_rejected{};
  bool direct_cycle_rejected{};
  bool indirect_cycle_rejected{};
  bool cross_project_cycle_rejected{};
  bool effect_order{};
  bool stage_invalidated{};
  bool receipt_invalidated{};
  bool invalid_handle_distinguished{};
  bool cleanup_balanced{};
  bool fixture_lookup{};
  bool effect_suite_acquired{};
  bool effect_applied{};
  uint32_t project_generation_before{};
  uint32_t project_generation_after{};
  uint32_t effect_count{};
  uint64_t stage_identity_hash{};
  uint64_t trace_hash{};
  uint64_t dependency_identity_hash{};
  uint64_t effect_order_hash{};
};
struct AegpCompatSelftestHooks {
  int32_t (*acquire_suite)(const char*, int32_t, const void**){};
  int32_t (*release_suite)(const char*, int32_t){};
  const void* comp_suite{};
  const void* interface_suite{};
  const void* helper_suite{};
  void* comp{};
  void* comp_item{};
  void* effect{};
  int32_t (*get_comp_bg_color)(void*, AegpCompatColor*){};
  int32_t (*convert_effect_time)(void*, int32_t, uint32_t,
                                 suite_abi::AegpTime*){};
  int32_t (*get_camera)(void*, const suite_abi::AegpTime*, void**){};
  int32_t (*get_camera_matrix)(void*, const suite_abi::AegpTime*,
                               AegpMatrix4*, double*, int16_t*, int16_t*){};
  void (*set_camera_index)(int32_t){};
  int32_t (*camera_index)(){};
  void* (*layer_at)(int32_t){};
  int32_t (*layer_index)(void*){};
  void (*set_dimensions)(int32_t, int32_t){};
  void (*get_dimensions)(int32_t*, int32_t*){};
  bool (*suite_leases_balanced)(){};
  void* pf_layer{};
  bool* comp_idle_roundtrip_mode{};
  std::size_t* active_ui_param_count{};
  int32_t (*get_new_effect_stream_v2)(int32_t, void*, int32_t, void**){};
  int32_t (*get_stream_name_v2)(void*, uint8_t, char*){};
  int32_t (*get_stream_type_v2)(void*, int32_t*){};
  int32_t (*get_new_stream_value_v2)(int32_t, void*, int32_t,
      const suite_abi::AegpTime*, uint8_t, scene_runtime::AegpStreamValue*){};
  int32_t (*set_stream_value_v2)(int32_t, void*, scene_runtime::AegpStreamValue*){};
  int32_t (*dispose_stream_value_v2)(scene_runtime::AegpStreamValue*){};
  int32_t (*dispose_stream_v2)(void*){};
  int32_t (*get_layer_source_item)(void*, void**){};
  int32_t (*get_item_type)(void*, int16_t*){};
  void* item_suite{};
  uint32_t* layer_source_item_calls{};
  uint32_t* item_type_calls{};
  int32_t (__cdecl* get_effect_param_union_v3)(int32_t, void*, int32_t,
                                               int32_t*, void*){};
};
void configure_aegp_compat_selftests(AegpCompatSelftestHooks hooks);
bool verify_legacy_effect_compat_suites();
bool verify_aegp_get_effect_camera();
bool verify_aegp_resizer_3d_chain();
bool verify_aegp_apply_effect();
bool verify_aegp_effect_stack();
bool verify_aegp_projector_levels();
bool verify_aegp_layer_source_item();
bool verify_aegp_scene_registry_suites();
bool verify_aegp_scene_mutation_transactions();
AegpSceneModelSelftestReport verify_aegp_scene_model();
bool verify_aegp_effect_param_union_suite4();
bool verify_aegp_installed_effect_catalog_suite4();
}
