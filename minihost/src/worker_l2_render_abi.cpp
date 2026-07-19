#include "worker_l2_render_abi.hpp"

namespace aexcompat::l2_detail {

PfAdvItemSuite1 g_adv_item_suite1{&adv_item_move_time_step,
    &adv_item_move_time_step_active, &adv_item_touch_active,
    &adv_item_force_rerender, &adv_item_effect_is_active};

BasicSuite g_basic_suite{&acquire_suite, &release_suite};

}  // namespace aexcompat::l2_detail
