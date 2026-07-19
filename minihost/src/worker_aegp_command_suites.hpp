#pragma once

#include <cstdint>

namespace aexcompat::l2_detail {

// Production callbacks stay in l2_main with the menu/command accounting and
// the aegp_init hook registration they guard.
int32_t __cdecl aegp_get_unique_command(int32_t* command);
int32_t __cdecl aegp_insert_menu_command(int32_t command, const char* name,
                                         int32_t menu, int32_t);
int32_t __cdecl aegp_remove_menu_command(int32_t);
int32_t __cdecl aegp_set_menu_command_name(int32_t, const char* name);
int32_t __cdecl aegp_command_state(int32_t command);
int32_t __cdecl aegp_check_menu_command(int32_t command, uint8_t checked);
int32_t __cdecl aegp_register_command_hook(int32_t plugin_id, int32_t priority,
                                           int32_t command, void* hook, void* refcon);
int32_t __cdecl aegp_register_update_menu_hook(int32_t plugin_id, void* hook, void* refcon);
int32_t __cdecl aegp_register_death_hook(int32_t plugin_id, void* hook, void* refcon);
int32_t __cdecl aegp_register_idle_hook(int32_t plugin_id, void* hook, void* refcon);

struct AegpCommandSuite {
  decltype(&aegp_get_unique_command) get_unique_command;
  decltype(&aegp_insert_menu_command) insert_menu_command;
  decltype(&aegp_remove_menu_command) remove_menu_command;
  decltype(&aegp_set_menu_command_name) set_menu_command_name;
  decltype(&aegp_command_state) enable_command;
  decltype(&aegp_command_state) disable_command;
  decltype(&aegp_check_menu_command) check_menu_command;
  decltype(&aegp_command_state) do_command;
};

struct AegpRegisterSuite {
  decltype(&aegp_register_command_hook) register_command_hook;
  decltype(&aegp_register_update_menu_hook) register_update_menu_hook;
  decltype(&aegp_register_death_hook) register_death_hook;
  void* register_version_hook{};
  void* register_about_string_hook{};
  void* register_about_hook{};
  void* register_artisan{};
  void* register_io{};
  decltype(&aegp_register_idle_hook) register_idle_hook;
  void* register_tracker{};
  void* register_interactive_artisan{};
  void* register_preset_localization{};
};

extern AegpCommandSuite g_aegp_command_suite;
extern AegpRegisterSuite g_aegp_register_suite;

}  // namespace aexcompat::l2_detail
