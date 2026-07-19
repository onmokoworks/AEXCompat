#include "worker_aegp_command_suites.hpp"

namespace aexcompat::l2_detail {

AegpCommandSuite g_aegp_command_suite{
    &aegp_get_unique_command, &aegp_insert_menu_command, &aegp_remove_menu_command,
    &aegp_set_menu_command_name, &aegp_command_state, &aegp_command_state,
    &aegp_check_menu_command, &aegp_command_state};

AegpRegisterSuite g_aegp_register_suite{
    &aegp_register_command_hook, &aegp_register_update_menu_hook,
    &aegp_register_death_hook, nullptr, nullptr, nullptr, nullptr, nullptr,
    &aegp_register_idle_hook, nullptr, nullptr, nullptr};

}  // namespace aexcompat::l2_detail
