#pragma once

#include <cstdint>
#include <vector>

namespace aexcompat::worker_runtime::aegp_init {

using UpdateMenuHook = int32_t(__cdecl*)(void*, void*, int32_t);
using IdleHook = int32_t(__cdecl*)(void*, void*, int32_t*);
using DeathHook = int32_t(__cdecl*)(void*, void*);
using CommandHook = int32_t(__cdecl*)(void*, void*, int32_t, uint32_t,
                                      uint8_t, uint8_t*);

struct UpdateMenuRegistration {
  int32_t plugin_id{};
  UpdateMenuHook hook{};
  void* refcon{};
};
struct IdleRegistration {
  int32_t plugin_id{};
  IdleHook hook{};
  void* refcon{};
};
struct DeathRegistration {
  int32_t plugin_id{};
  DeathHook hook{};
  void* refcon{};
};
struct CommandRegistration {
  int32_t plugin_id{};
  uint32_t priority{};
  int32_t command{};
  CommandHook hook{};
  void* refcon{};
};

struct State {
  bool init_mode{};
  bool idle_mode{};
  uint32_t command_hooks{};
  uint32_t update_menu_hooks{};
  uint32_t idle_hooks{};
  uint32_t death_hooks{};
  std::vector<CommandRegistration> command_registrations;
  std::vector<UpdateMenuRegistration> update_menu_registrations;
  std::vector<IdleRegistration> idle_registrations;
  std::vector<DeathRegistration> death_registrations;
  // AEGP command/menu bookkeeping and the event-mode latches worker_main
  // derives from the invocation (issue #126 Phase D). The AEGP suite
  // callbacks in worker_main write them and the aegp_init completion report
  // reads them back. Lifetime: process-lifetime, never torn down.
  bool keyframe_roundtrip_mode{};
  bool seek_roundtrip_mode{};
  bool trim_roundtrip_mode{};
  bool switch_roundtrip_mode{};
  bool skip_about{};
  uint32_t commands_created{};
  uint32_t menu_commands_inserted{};
  int32_t next_command{10000};
  uint32_t command_enable_calls{};
  uint32_t command_check_calls{};
  uint32_t command_checked_true_calls{};
  uint32_t command_checked_false_calls{};
  std::vector<int32_t> inserted_commands;
};

struct EventResult {
  int32_t error{};
  uint32_t invoked{};
  int32_t idle_max_sleep{-1};
  bool handled{};
  uint32_t handled_count{};
};

struct BasicDispatchResult {
  int32_t error{};
  uint32_t hooks_invoked{};
  uint32_t menu_hooks_invoked{};
  uint32_t command_hooks_invoked{};
  uint32_t command_handled_count{};
  int32_t idle_max_sleep{-1};
};

State& state() noexcept;
int32_t register_command_hook(int32_t plugin_id, int32_t priority,
                              int32_t command, void* hook, void* refcon);
int32_t register_update_menu_hook(int32_t plugin_id, void* hook, void* refcon);
int32_t register_idle_hook(int32_t plugin_id, void* hook, void* refcon);
int32_t register_death_hook(int32_t plugin_id, void* hook, void* refcon);
EventResult dispatch_update_menu(void* global_refcon, int32_t active_window_type);
EventResult dispatch_idle(void* global_refcon);
EventResult dispatch_command(void* global_refcon, int32_t command,
                             uint32_t hook_priority, uint8_t already_handled);
EventResult dispatch_death(void* global_refcon);
EventResult dispatch_death_for_plugin(int32_t plugin_id, void* global_refcon);
void forget_plugin_registrations(int32_t plugin_id) noexcept;
BasicDispatchResult dispatch_basic_events(void* global_refcon,
    bool update_menu, bool idle, bool command, int32_t command_id);

}  // namespace aexcompat::worker_runtime::aegp_init
