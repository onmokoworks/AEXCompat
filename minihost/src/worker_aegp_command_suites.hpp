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
int32_t __cdecl aegp_register_version_hook(int32_t plugin_id, void* hook,
                                           void* refcon);
int32_t __cdecl aegp_register_about_string_hook(int32_t plugin_id, void* hook,
                                                void* refcon);
int32_t __cdecl aegp_register_about_hook(int32_t plugin_id, void* hook,
                                         void* refcon);
struct AegpVersion {
  int16_t major{};
  int16_t minor{};
};
static_assert(sizeof(AegpVersion) == 4);

int32_t __cdecl aegp_register_artisan(AegpVersion api_version,
                                      AegpVersion artisan_version,
                                      int32_t plugin_id, void* refcon,
                                      const char* match_name,
                                      const char* artisan_name,
                                      void* entry_points);
int32_t __cdecl aegp_register_io(int32_t plugin_id, void* refcon,
                                const void* module_info,
                                const void* function_block);
int32_t __cdecl aegp_register_tracker(AegpVersion api_version,
                                      AegpVersion tracker_version,
                                      int32_t plugin_id, void* refcon,
                                      const char* match_name,
                                      const char* tracker_name,
                                      const void* entry_points);
int32_t __cdecl aegp_register_interactive_artisan(
    AegpVersion api_version, AegpVersion artisan_version, int32_t plugin_id,
    void* refcon, const char* match_name, const char* artisan_name,
    void* entry_points);
int32_t __cdecl aegp_register_preset_localization_string(
    const char* english_name, const char* localized_name);

struct AegpRegisterSuiteStatistics {
  uint32_t preset_localization_calls{};
  uint32_t unsupported_registration_calls{};
};

AegpRegisterSuiteStatistics aegp_register_suite_statistics();
void reset_aegp_register_suite_statistics();

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
  decltype(&aegp_register_version_hook) register_version_hook;
  decltype(&aegp_register_about_string_hook) register_about_string_hook;
  decltype(&aegp_register_about_hook) register_about_hook;
  decltype(&aegp_register_artisan) register_artisan;
  decltype(&aegp_register_io) register_io;
  decltype(&aegp_register_idle_hook) register_idle_hook;
  decltype(&aegp_register_tracker) register_tracker;
  decltype(&aegp_register_interactive_artisan) register_interactive_artisan;
  decltype(&aegp_register_preset_localization_string)
      register_preset_localization;
};

static_assert(sizeof(AegpRegisterSuite) == 12 * sizeof(void*));

extern AegpCommandSuite g_aegp_command_suite;
extern AegpRegisterSuite g_aegp_register_suite;
extern AegpRegisterSuite g_pf_safe_aegp_register_suite;

const AegpRegisterSuite* aegp_register_suite_for_mode(bool aegp_init_mode);

}  // namespace aexcompat::l2_detail
