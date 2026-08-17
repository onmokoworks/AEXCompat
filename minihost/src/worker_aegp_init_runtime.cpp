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
  g_state.command_registrations.push_back({plugin_id, static_cast<uint32_t>(priority), command,
      reinterpret_cast<CommandHook>(hook), refcon});
  ++g_state.command_hooks;
  return 0;
}

int32_t register_update_menu_hook(int32_t plugin_id, void* hook, void* refcon) {
  if (plugin_id <= 0 || !hook || g_state.update_menu_registrations.size() >= kMaxHooks)
    return 4;
  g_state.update_menu_registrations.push_back(
      {plugin_id, reinterpret_cast<UpdateMenuHook>(hook), refcon});
  ++g_state.update_menu_hooks;
  return 0;
}

int32_t register_idle_hook(int32_t plugin_id, void* hook, void* refcon) {
  if (plugin_id <= 0 || !hook || g_state.idle_registrations.size() >= kMaxHooks) return 4;
  g_state.idle_registrations.push_back(
      {plugin_id, reinterpret_cast<IdleHook>(hook), refcon});
  ++g_state.idle_hooks;
  return 0;
}

int32_t register_death_hook(int32_t plugin_id, void* hook, void* refcon) {
  if (plugin_id <= 0 || !hook || g_state.death_registrations.size() >= kMaxHooks) return 4;
  g_state.death_registrations.push_back(
      {plugin_id, reinterpret_cast<DeathHook>(hook), refcon});
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

EventResult dispatch_death_for_plugin(int32_t plugin_id, void* global_refcon) {
  EventResult result;
  if (plugin_id <= 0) {
    result.error = 4;
    return result;
  }
  for (const auto& registration : g_state.death_registrations) {
    if (registration.plugin_id != plugin_id) continue;
    const int32_t error = registration.hook(global_refcon, registration.refcon);
    ++result.invoked;
    if (error && !result.error) result.error = error;
  }
  return result;
}

void forget_plugin_registrations(int32_t plugin_id) noexcept {
  if (plugin_id <= 0) return;
  const auto erase_plugin = [plugin_id](auto& registrations) {
    registrations.erase(
        std::remove_if(registrations.begin(), registrations.end(),
                       [plugin_id](const auto& registration) {
                         return registration.plugin_id == plugin_id;
                       }),
        registrations.end());
  };
  erase_plugin(g_state.command_registrations);
  erase_plugin(g_state.update_menu_registrations);
  erase_plugin(g_state.idle_registrations);
  erase_plugin(g_state.death_registrations);
}

BasicDispatchResult dispatch_basic_events(void* global_refcon,
                                          bool update_menu, bool idle,
                                          bool command, int32_t command_id) {
  BasicDispatchResult result;
  if (update_menu) {
    const auto event = dispatch_update_menu(global_refcon, 0);
    result.hooks_invoked += event.invoked;
    if (event.error != 0 && result.error == 0) result.error = event.error;
  }
  if (idle) {
    const auto event = dispatch_idle(global_refcon);
    result.hooks_invoked += event.invoked;
    result.idle_max_sleep = event.idle_max_sleep;
    if (event.error != 0 && result.error == 0) result.error = event.error;
  }
  if (command) {
    for (int pass = 0; pass < 2; ++pass) {
      const auto event = dispatch_command(global_refcon, command_id, 0, 0);
      result.command_hooks_invoked += event.invoked;
      result.command_handled_count += event.handled_count;
      if (event.error != 0 && result.error == 0) result.error = event.error;
    }
  }
  return result;
}

}  // namespace aexcompat::worker_runtime::aegp_init
