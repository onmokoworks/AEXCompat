#include "worker_pf_param_suites.hpp"

namespace aexcompat::l2_detail {

PointParamSuite g_point_param_suite{&floating_point_from_point};
AngleParamSuite g_angle_param_suite{&floating_point_from_angle};
PfColorParamSuite1 g_color_param_suite1{&floating_point_from_color};
ParamUtilsSuite1 g_param_utils_suite1{&update_param_ui, &get_current_param_state_obsolete,
    &has_param_changed_obsolete, &have_inputs_changed_over_time_span_obsolete,
    &is_identical_param_checkout, &find_param_keyframe_time, &get_param_keyframe_count,
    &checkout_param_keyframe, &checkin_param_keyframe, &param_key_index_to_time};
ParamUtilsSuite3 g_param_utils_suite{&update_param_ui, &get_current_param_state,
    &are_param_states_identical, &is_identical_param_checkout, &find_param_keyframe_time,
    &get_param_keyframe_count, &checkout_param_keyframe, &checkin_param_keyframe,
    &param_key_index_to_time};

}  // namespace aexcompat::l2_detail
