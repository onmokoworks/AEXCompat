#pragma once

#include "worker_aegp_scene.hpp"

#include <cstddef>
#include <cstdint>

namespace aexcompat::l2_detail {

// Production callbacks stay in l2_main with the host identities they guard.
int32_t __cdecl get_effect_layer(void* effect, void** layer);
int32_t __cdecl convert_effect_to_comp_time(
    void* effect, int32_t what_time, uint32_t time_scale, AegpTime* comp_time);
int32_t __cdecl get_effect_camera(
    void* effect, const AegpTime* comp_time, void** camera_layer);
int32_t __cdecl get_effect_camera_matrix(void* effect, const AegpTime* comp_time,
    AegpMatrix4* camera_matrix, double* distance_to_image_plane,
    int16_t* image_plane_width, int16_t* image_plane_height);

struct PfInterfaceSuite {
  decltype(&get_effect_layer) get_effect_layer;
  decltype(&get_new_effect_for_effect) get_new_effect_for_effect;
  decltype(&convert_effect_to_comp_time) convert_effect_to_comp_time;
  decltype(&get_effect_camera) get_effect_camera;
  decltype(&get_effect_camera_matrix) get_effect_camera_matrix;
};
static_assert(sizeof(PfInterfaceSuite) == 5 * sizeof(void*));
static_assert(offsetof(PfInterfaceSuite, convert_effect_to_comp_time) == 2 * sizeof(void*));
static_assert(offsetof(PfInterfaceSuite, get_effect_camera) == 3 * sizeof(void*));
static_assert(offsetof(PfInterfaceSuite, get_effect_camera) == 24);
static_assert(offsetof(PfInterfaceSuite, get_effect_camera_matrix) == 32);

extern PfInterfaceSuite g_pf_interface_suite;

}  // namespace aexcompat::l2_detail
