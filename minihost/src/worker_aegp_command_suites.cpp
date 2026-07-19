#include "worker_aegp_command_suites.hpp"

#include "worker_aegp_init_runtime.hpp"

#include <cstring>

namespace aexcompat::l2_detail {

// AEGP command/menu bookkeeping read and written through its owner,
// aexcompat::worker_runtime::aegp_init::state() (issue #126 Phase D); these
// references keep the g_* spellings.
namespace {
auto& g_aegp_init_runtime = aexcompat::worker_runtime::aegp_init::state();
auto& g_aegp_commands_created = g_aegp_init_runtime.commands_created;
auto& g_aegp_menu_commands_inserted = g_aegp_init_runtime.menu_commands_inserted;
auto& g_next_aegp_command = g_aegp_init_runtime.next_command;
auto& g_aegp_command_enable_calls = g_aegp_init_runtime.command_enable_calls;
auto& g_aegp_command_check_calls = g_aegp_init_runtime.command_check_calls;
auto& g_aegp_command_checked_true_calls = g_aegp_init_runtime.command_checked_true_calls;
auto& g_aegp_command_checked_false_calls = g_aegp_init_runtime.command_checked_false_calls;
auto& g_aegp_inserted_commands = g_aegp_init_runtime.inserted_commands;
}  // namespace

int32_t __cdecl aegp_get_unique_command(int32_t* command) {
  if (!command || g_aegp_commands_created >= 64) return 4;
  *command = g_next_aegp_command++;
  ++g_aegp_commands_created;
  return 0;
}
int32_t __cdecl aegp_insert_menu_command(int32_t command, const char* name,
                                         int32_t menu, int32_t) {
  if (command < 10000 || !name || strnlen_s(name, 1024) == 0 || menu < 0 || menu > 16 ||
      g_aegp_menu_commands_inserted >= 64) return 4;
  ++g_aegp_menu_commands_inserted;
  g_aegp_inserted_commands.push_back(command);
  return 0;
}
int32_t __cdecl aegp_remove_menu_command(int32_t) { return 0; }
int32_t __cdecl aegp_set_menu_command_name(int32_t, const char* name) {
  return name && strnlen_s(name, 1024) > 0 ? 0 : 4;
}
int32_t __cdecl aegp_command_state(int32_t command) {
  if (command < 10000) return 4;
  ++g_aegp_command_enable_calls;
  return 0;
}
int32_t __cdecl aegp_check_menu_command(int32_t command, uint8_t checked) {
  if (command < 10000) return 4;
  ++g_aegp_command_check_calls;
  if (checked) ++g_aegp_command_checked_true_calls;
  else ++g_aegp_command_checked_false_calls;
  return 0;
}
// The AEGP Command Suite table lives in worker_aegp_command_suites.cpp.

int32_t __cdecl aegp_register_command_hook(int32_t plugin_id, int32_t priority,
                                           int32_t command, void* hook, void* refcon) {
  return aexcompat::worker_runtime::aegp_init::register_command_hook(
      plugin_id, priority, command, hook, refcon);
}
int32_t __cdecl aegp_register_update_menu_hook(int32_t plugin_id, void* hook, void* refcon) {
  return aexcompat::worker_runtime::aegp_init::register_update_menu_hook(
      plugin_id, hook, refcon);
}
int32_t __cdecl aegp_register_death_hook(int32_t plugin_id, void* hook, void* refcon) {
  return aexcompat::worker_runtime::aegp_init::register_death_hook(plugin_id, hook, refcon);
}
int32_t __cdecl aegp_register_idle_hook(int32_t plugin_id, void* hook, void* refcon) {
  return aexcompat::worker_runtime::aegp_init::register_idle_hook(plugin_id, hook, refcon);
}


AegpCommandSuite g_aegp_command_suite{
    &aegp_get_unique_command, &aegp_insert_menu_command, &aegp_remove_menu_command,
    &aegp_set_menu_command_name, &aegp_command_state, &aegp_command_state,
    &aegp_check_menu_command, &aegp_command_state};

AegpRegisterSuite g_aegp_register_suite{
    &aegp_register_command_hook, &aegp_register_update_menu_hook,
    &aegp_register_death_hook, nullptr, nullptr, nullptr, nullptr, nullptr,
    &aegp_register_idle_hook, nullptr, nullptr, nullptr};

}  // namespace aexcompat::l2_detail
