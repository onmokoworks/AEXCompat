#pragma once
#include "worker_aegp_scene.hpp"
namespace aexcompat::l2_detail {
struct AegpCompatColor { double alpha, red, green, blue; };
struct AegpCompatSelftestHooks {
  bool (*resizer_3d)(){};
  bool (*apply_effect)(){};
  bool (*effect_stack)(){};
  bool (*projector_levels)(){};
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
};
void configure_aegp_compat_selftests(AegpCompatSelftestHooks hooks);
bool verify_legacy_effect_compat_suites();
bool verify_aegp_get_effect_camera();
bool verify_aegp_resizer_3d_chain();
bool verify_aegp_apply_effect();
bool verify_aegp_effect_stack();
bool verify_aegp_projector_levels();
}
