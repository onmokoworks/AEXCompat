#pragma once
#include <cstdint>
namespace aexcompat::color_settings::selftests {
struct Hooks {
  int32_t (*acquire_suite)(const char*, int32_t, const void**){};
  int32_t (*release_suite)(const char*, int32_t){};
  void* composition{};
};
void configure(Hooks hooks);
bool verify_pf_color_settings_suite6();
}

