#include "worker_aegp_init_runtime.hpp"

#include <algorithm>

namespace aexcompat::worker_runtime::aegp_init {
namespace {
constexpr std::size_t kMaxHooks = 64;
State g_state;
}

State& state() noexcept { return g_state; }

int32_t register_command_hook(int32_t plugin_id, int32_t priority,
                              int32_t command, void* hook, void* refcon) {
  if (plugin_id <= 0 || (priority != 1 && priority != 2) || command < 0 || !hook ||
      g_state.command_registrations.size() >= kMaxHooks) return 4;
  g_state.command_registrations.push_back({static_cast<uint32_t>(priority), command,
      reinterpret_cast<CommandHook>(hook), refcon});
  ++g_state.command_hooks;
  return 0;
}

int32_t register_update_menu_hook(int32_t plugin_id, void* hook, void* refcon) {
  if (plugin_id <= 0 || !hook || g_state.update_menu_registrations.size() >= kMaxHooks)
    return 4;
  g_state.update_menu_registrations.push_back(
      {reinterpret_cast<UpdateMenuHook>(hook), refcon});
  ++g_state.update_menu_hooks;
  return 0;
}

int32_t register_idle_hook(int32_t plugin_id, void* hook, void* refcon) {
  if (plugin_id <= 0 || !hook || g_state.idle_registrations.size() >= kMaxHooks) return 4;
  g_state.idle_registrations.push_back({reinterpret_cast<IdleHook>(hook), refcon});
  ++g_state.idle_hooks;
  return 0;
}

int32_t register_death_hook(int32_t plugin_id, void* hook, void* refcon) {
  if (plugin_id <= 0 || !hook || g_state.death_registrations.size() >= kMaxHooks) return 4;
  g_state.death_registrations.push_back({reinterpret_cast<DeathHook>(hook), refcon});
  ++g_state.death_hooks;
  return 0;
}

EventResult dispatch_update_menu(void* global_refcon, int32_t active_window_type) {
  EventResult result;
  if (g_state.update_menu_registrations.empty()) result.error = 4;
  for (const auto& registration : g_state.update_menu_registrations) {
    const int32_t error = registration.hook(global_refcon, registration.refcon,
                                             active_window_type);
    ++result.invoked;
    if (error && !result.error) result.error = error;
  }
  return result;
}

EventResult dispatch_idle(void* global_refcon) {
  EventResult result;
  if (g_state.idle_registrations.empty()) result.error = 4;
  for (const auto& registration : g_state.idle_registrations) {
    int32_t requested_sleep{};
    const int32_t error = registration.hook(global_refcon, registration.refcon,
                                             &requested_sleep);
    ++result.invoked;
    if (error && !result.error) result.error = error;
    if (requested_sleep < 0 || requested_sleep > 3600) {
      if (!result.error) result.error = 4;
    } else if (result.idle_max_sleep < 0 || requested_sleep < result.idle_max_sleep) {
      result.idle_max_sleep = requested_sleep;
    }
  }
  return result;
}

EventResult dispatch_command(void* global_refcon, int32_t command,
                             uint32_t hook_priority, uint8_t already_handled) {
  (void)hook_priority;
  EventResult result;
  uint8_t handled = already_handled;
  for (const auto& registration : g_state.command_registrations) {
    if (registration.command != 0 && registration.command != command) continue;
    uint8_t hook_handled{};
    const int32_t error = registration.hook(global_refcon, registration.refcon,
        command, registration.priority, handled, &hook_handled);
    ++result.invoked;
    if (error && !result.error) result.error = error;
    if (hook_handled > 1 && !result.error) result.error = 4;
    if (hook_handled) {
      handled = 1;
      ++result.handled_count;
    }
  }
  result.handled = handled != 0;
  if (!result.handled && !result.error) result.error = 4;
  return result;
}

EventResult dispatch_death(void* global_refcon) {
  EventResult result;
  for (const auto& registration : g_state.death_registrations) {
    const int32_t error = registration.hook(global_refcon, registration.refcon);
    ++result.invoked;
    if (error && !result.error) result.error = error;
  }
  return result;
}

}  // namespace aexcompat::worker_runtime::aegp_init
