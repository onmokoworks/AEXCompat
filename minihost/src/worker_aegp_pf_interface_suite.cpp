#include "worker_aegp_pf_interface_suite.hpp"

namespace aexcompat::l2_detail {

PfInterfaceSuite g_pf_interface_suite{&get_effect_layer, &get_new_effect_for_effect,
    &convert_effect_to_comp_time, &get_effect_camera,
    &get_effect_camera_matrix};

}  // namespace aexcompat::l2_detail
