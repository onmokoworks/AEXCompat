#include "worker_aegp_init_runtime.hpp"

using namespace aexcompat::worker_runtime::aegp_init;

namespace {
void* expected_global = reinterpret_cast<void*>(0x1234);
void* expected_refcon = reinterpret_cast<void*>(0x5678);
int32_t __cdecl update(void* global, void* refcon, int32_t window) {
  return global == expected_global && refcon == expected_refcon && window == 3 ? 0 : 4;
}
int32_t __cdecl idle(void* global, void* refcon, int32_t* sleep) {
  if (global != expected_global || refcon != expected_refcon || !sleep) return 4;
  *sleep = 25; return 0;
}
int32_t __cdecl command(void* global, void* refcon, int32_t value,
                        uint32_t priority, uint8_t already, uint8_t* handled) {
  if (global != expected_global || refcon != expected_refcon || value != 42 ||
      priority != 1 || already || !handled) return 4;
  *handled = 1; return 0;
}
int32_t __cdecl death(void* global, void* refcon) {
  return global == expected_global && refcon == expected_refcon ? 0 : 4;
}
}  // namespace

int main() {
  if (register_update_menu_hook(1, reinterpret_cast<void*>(&update), expected_refcon) ||
      register_idle_hook(1, reinterpret_cast<void*>(&idle), expected_refcon) ||
      register_command_hook(1, 1, 42, reinterpret_cast<void*>(&command), expected_refcon) ||
      register_death_hook(1, reinterpret_cast<void*>(&death), expected_refcon)) return 1;
  const auto menu = dispatch_update_menu(expected_global, 3);
  const auto idle_result = dispatch_idle(expected_global);
  const auto command_result = dispatch_command(expected_global, 42, 0, 0);
  const auto death_result = dispatch_death(expected_global);
  return menu.error == 0 && menu.invoked == 1 && idle_result.error == 0 &&
      idle_result.idle_max_sleep == 25 && command_result.error == 0 &&
      command_result.handled && command_result.handled_count == 1 &&
      death_result.error == 0 && death_result.invoked == 1 ? 0 : 2;
}
