#pragma once

#include "worker_parameter_runtime.hpp"

#include <cstdint>
#include <vector>

namespace aexcompat::pf_color_selftests {

using ParamRecord = worker_runtime::parameters::ParamRecord;

struct PixelFloat {
  float alpha;
  float red;
  float green;
  float blue;
};
static_assert(sizeof(PixelFloat) == 4 * sizeof(float));
static_assert(offsetof(PixelFloat, alpha) == 0 * sizeof(float));
static_assert(offsetof(PixelFloat, red) == 1 * sizeof(float));
static_assert(offsetof(PixelFloat, green) == 2 * sizeof(float));
static_assert(offsetof(PixelFloat, blue) == 3 * sizeof(float));

struct Hooks {
  int32_t (*acquire_suite)(const char*, int32_t, const void**){};
  int32_t (*release_suite)(const char*, int32_t){};
  void* effect{};
  const void* color_param_suite1{};
  std::vector<ParamRecord>* params{};
  int32_t (__cdecl* floating_point_from_color)(void*, const void*, PixelFloat*){};
};

void configure(Hooks hooks);
bool verify_pf_color_suite();
bool verify_pf_color_param_suite();

}  // namespace aexcompat::pf_color_selftests
