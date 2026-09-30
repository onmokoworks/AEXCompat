#pragma once

#include <cstdint>

namespace aexcompat::host_guard_selftests {

struct Hooks {
  int32_t (*acquire_suite)(const char*, int32_t, const void**){};
  int32_t (*release_suite)(const char*, int32_t){};
  uint32_t (*suite_acquire_count)(){};
  uint32_t (*suite_release_count)(){};
  bool (*suite_leases_balanced)(){};
};

void configure(Hooks hooks);
bool verify_pf_adv_app_suite_versions();
bool verify_pf_pixel_format_suite_versions();
bool verify_render_output_safety();
uint32_t cleanup_safety_selftest_calls();

}  // namespace aexcompat::host_guard_selftests
