#pragma once
#include "worker_parameter_runtime.hpp"
#include "worker_pf_state_runtime.hpp"
#include <cstdint>
#include <vector>
namespace aexcompat::parameter_selftests {
using Definition = worker_runtime::parameters::Definition;
using PfState = pf_state_runtime::PfState;
using PfTime = pf_state_runtime::PfTime;
struct Hooks {
  int32_t (*acquire_suite)(const char*, int32_t, const void**){};
  int32_t (*release_suite)(const char*, int32_t){};
  void* effect{}; void* layer{}; const void* param_utils_suite1{}; const void* param_utils_suite3{};
  int32_t (*update_param_ui)(void*, int32_t, const void*){};
  int32_t (*is_identical_checkout)(void*, int32_t, int32_t, int32_t, uint32_t, int32_t, int32_t, uint32_t, uint8_t*){};
  int32_t (*find_keyframe_time)(void*, int32_t, int32_t, uint32_t, int32_t, uint8_t*, int32_t*, int32_t*, uint32_t*){};
  int32_t (*get_keyframe_count)(void*, int32_t, int32_t*){};
  int32_t (*checkout_keyframe)(void*, int32_t, int32_t, int32_t*, uint32_t*, void*){};
  int32_t (*checkin_keyframe)(void*, void*){};
  int32_t (*key_index_to_time)(void*, int32_t, int32_t, int32_t*, uint32_t*){};
  int32_t (*get_current_obsolete)(void*, PfState*){};
  int32_t (*has_changed_obsolete)(void*, const PfState*, int32_t, uint8_t*){};
  int32_t (*inputs_changed_obsolete)(void*, const PfState*, const PfTime*, const PfTime*, uint8_t*){};
  bool (*apply_animation)(std::vector<Definition>&, int32_t, uint32_t){};
};
void configure(Hooks hooks);
bool verify_pf_param_utils_suite3();
bool verify_parameter_animation_transport();
}

