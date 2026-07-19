#pragma once

#include "worker_pf_state_runtime.hpp"

#include <cstddef>
#include <cstdint>

namespace aexcompat::l2_detail {

using pf_state_runtime::PfState;
using pf_state_runtime::are_param_states_identical;
using pf_state_runtime::get_current_param_state;
using pf_state_runtime::get_current_param_state_obsolete;
using pf_state_runtime::has_param_changed_obsolete;
using pf_state_runtime::have_inputs_changed_over_time_span_obsolete;

// Production parameter callbacks stay in l2_main with the parameter state
// they guard; the tables here only publish their frozen slot layout.
int32_t __cdecl floating_point_from_point(void*, const void* definition, void* output);
int32_t __cdecl floating_point_from_angle(void*, const void* definition, double* output);

struct PfColorParamPixelFloat {
  float alpha;
  float red;
  float green;
  float blue;
};

int32_t __cdecl floating_point_from_color(void* effect_ref, const void* definition,
                                          PfColorParamPixelFloat* output);
int32_t __cdecl update_param_ui(void* effect_ref, int32_t index, const void* definition);
int32_t __cdecl is_identical_param_checkout(void* effect_ref, int32_t index,
                                            int32_t time1, int32_t step1,
                                            uint32_t scale1, int32_t time2,
                                            int32_t step2, uint32_t scale2,
                                            uint8_t* identical);
int32_t __cdecl find_param_keyframe_time(void* effect_ref, int32_t index,
                                         int32_t time, uint32_t scale,
                                         int32_t direction, uint8_t* found,
                                         int32_t* key_index, int32_t* key_time,
                                         uint32_t* key_scale);
int32_t __cdecl get_param_keyframe_count(void* effect_ref, int32_t index,
                                         int32_t* count);
int32_t __cdecl checkout_param_keyframe(void* effect_ref, int32_t index,
                                        int32_t key_index, int32_t* key_time,
                                        uint32_t* key_scale, void* definition);
int32_t __cdecl checkin_param_keyframe(void* effect_ref, void* definition);
int32_t __cdecl param_key_index_to_time(void* effect_ref, int32_t index, int32_t key_index,
                                        int32_t* key_time, uint32_t* key_scale);

struct PointParamSuite { decltype(&floating_point_from_point) get_floating_point_value; };
struct AngleParamSuite { decltype(&floating_point_from_angle) get_floating_point_value; };
struct PfColorParamSuite1 {
  decltype(&floating_point_from_color) PF_GetFloatingPointColorFromColorDef;
};
static_assert(sizeof(PfColorParamPixelFloat) == 4 * sizeof(float));
static_assert(offsetof(PfColorParamPixelFloat, alpha) == 0 * sizeof(float));
static_assert(offsetof(PfColorParamPixelFloat, red) == 1 * sizeof(float));
static_assert(offsetof(PfColorParamPixelFloat, green) == 2 * sizeof(float));
static_assert(offsetof(PfColorParamPixelFloat, blue) == 3 * sizeof(float));
static_assert(sizeof(PfColorParamSuite1) == 1 * sizeof(void*));
static_assert(offsetof(PfColorParamSuite1, PF_GetFloatingPointColorFromColorDef) ==
              0 * sizeof(void*));

struct ParamUtilsSuite1 {
  decltype(&update_param_ui) PF_UpdateParamUI;
  decltype(&get_current_param_state_obsolete) PF_GetCurrentStateObsolete;
  decltype(&has_param_changed_obsolete) PF_HasParamChangedObsolete;
  decltype(&have_inputs_changed_over_time_span_obsolete)
      PF_HaveInputsChangedOverTimeSpanObsolete;
  decltype(&is_identical_param_checkout) PF_IsIdenticalCheckout;
  decltype(&find_param_keyframe_time) PF_FindKeyframeTime;
  decltype(&get_param_keyframe_count) PF_GetKeyframeCount;
  decltype(&checkout_param_keyframe) PF_CheckoutKeyframe;
  decltype(&checkin_param_keyframe) PF_CheckinKeyframe;
  decltype(&param_key_index_to_time) PF_KeyIndexToTime;
};

struct ParamUtilsSuite3 {
  decltype(&update_param_ui) PF_UpdateParamUI;
  decltype(&get_current_param_state) PF_GetCurrentState;
  decltype(&are_param_states_identical) PF_AreStatesIdentical;
  decltype(&is_identical_param_checkout) PF_IsIdenticalCheckout;
  decltype(&find_param_keyframe_time) PF_FindKeyframeTime;
  decltype(&get_param_keyframe_count) PF_GetKeyframeCount;
  decltype(&checkout_param_keyframe) PF_CheckoutKeyframe;
  decltype(&checkin_param_keyframe) PF_CheckinKeyframe;
  decltype(&param_key_index_to_time) PF_KeyIndexToTime;
};
static_assert(sizeof(PfState) == 16);
static_assert(sizeof(ParamUtilsSuite1) == 10 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_UpdateParamUI) == 0 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_GetCurrentStateObsolete) == 1 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_HasParamChangedObsolete) == 2 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_HaveInputsChangedOverTimeSpanObsolete) ==
              3 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_KeyIndexToTime) == 9 * sizeof(void*));
static_assert(sizeof(ParamUtilsSuite3) == 9 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite3, PF_UpdateParamUI) == 0 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite3, PF_GetCurrentState) == 1 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite3, PF_AreStatesIdentical) == 2 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite3, PF_KeyIndexToTime) == 8 * sizeof(void*));

extern PointParamSuite g_point_param_suite;
extern AngleParamSuite g_angle_param_suite;
extern PfColorParamSuite1 g_color_param_suite1;
extern ParamUtilsSuite1 g_param_utils_suite1;
extern ParamUtilsSuite3 g_param_utils_suite;

}  // namespace aexcompat::l2_detail
