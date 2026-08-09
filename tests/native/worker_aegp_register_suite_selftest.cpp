#include "worker_aegp_command_suites.hpp"

#include <cassert>
#include <cstddef>
#include <string>

namespace {
int32_t __cdecl idle_hook(void*, void*, int32_t*) { return 0; }
}

int main() {
  using namespace aexcompat::l2_detail;

  const auto* aegp_suite = aegp_register_suite_for_mode(true);
  const auto* pf_suite = aegp_register_suite_for_mode(false);
  assert(aegp_suite == &g_aegp_register_suite);
  assert(pf_suite == &g_pf_safe_aegp_register_suite);
  assert(aegp_suite != pf_suite);
  for (const auto* suite : {aegp_suite, pf_suite}) {
    const auto* slots = reinterpret_cast<void* const*>(suite);
    for (std::size_t index = 0; index < 12; ++index) assert(slots[index]);
  }

  reset_aegp_register_suite_statistics();
  assert(pf_suite->register_preset_localization(
             "English Name", "Localized Name") == 0);
  auto statistics = aegp_register_suite_statistics();
  assert(statistics.preset_localization_calls == 1);
  assert(statistics.unsupported_registration_calls == 0);

  assert(pf_suite->register_preset_localization(nullptr, "x") == 4);
  assert(pf_suite->register_preset_localization("x", nullptr) == 4);
  assert(pf_suite->register_preset_localization("", "x") == 4);
  assert(pf_suite->register_preset_localization("x", "") == 4);
  const std::string overlong(4097, 'x');
  assert(pf_suite->register_preset_localization(
             overlong.c_str(), "x") == 4);
  statistics = aegp_register_suite_statistics();
  assert(statistics.preset_localization_calls == 1);

  reset_aegp_register_suite_statistics();
  for (std::size_t index = 0; index < 1024; ++index)
    assert(pf_suite->register_preset_localization("a", "b") == 0);
  assert(pf_suite->register_preset_localization("a", "b") == 4);
  statistics = aegp_register_suite_statistics();
  assert(statistics.preset_localization_calls == 1024);

  assert(pf_suite->register_command_hook(1, 1, 0, nullptr, nullptr) == 4);
  assert(pf_suite->register_update_menu_hook(1, nullptr, nullptr) == 4);
  assert(pf_suite->register_death_hook(1, nullptr, nullptr) == 4);
  assert(pf_suite->register_idle_hook(1, nullptr, nullptr) == 4);
  assert(pf_suite->register_idle_hook(1, reinterpret_cast<void*>(&idle_hook),
                                      nullptr) == 0);
  assert(pf_suite->register_idle_hook(0, reinterpret_cast<void*>(&idle_hook),
                                      nullptr) == 4);
  assert(pf_suite->register_idle_hook(-1, reinterpret_cast<void*>(&idle_hook),
                                      nullptr) == 4);
  assert(pf_suite->register_version_hook(1, nullptr, nullptr) == 4);
  assert(pf_suite->register_about_string_hook(1, nullptr, nullptr) == 4);
  assert(pf_suite->register_about_hook(1, nullptr, nullptr) == 4);
  assert(pf_suite->register_artisan(
             {}, {}, 1, nullptr, nullptr, nullptr, nullptr) == 4);
  assert(pf_suite->register_io(1, nullptr, nullptr, nullptr) == 4);
  assert(pf_suite->register_tracker(
             {}, {}, 1, nullptr, nullptr, nullptr, nullptr) == 4);
  assert(pf_suite->register_interactive_artisan(
             {}, {}, 1, nullptr, nullptr, nullptr, nullptr) == 4);
  statistics = aegp_register_suite_statistics();
  assert(statistics.transient_idle_hook_registrations == 1);
  assert(statistics.unsupported_registration_calls == 10);
  return 0;
}
