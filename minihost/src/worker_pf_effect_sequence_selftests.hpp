#pragma once

#include "worker_pf_state_runtime.hpp"

namespace aexcompat::pf_effect_sequence_selftests {

struct Hooks {
  int32_t (*acquire_suite)(const char*, int32_t, const void**){};
  int32_t (*release_suite)(const char*, int32_t){};
  pf_state_runtime::PfEffectSequenceDataSuite1* suite{};
  int32_t bad_callback_param{};
};

bool verify_suite1(void* effect_ref, const Hooks&);

}  // namespace aexcompat::pf_effect_sequence_selftests
