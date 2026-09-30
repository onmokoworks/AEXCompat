#include "worker_effect_bootstrap.hpp"

#include <cstdint>
#include <cstring>
#include <iostream>

namespace boot = aexcompat::worker_runtime::effect_bootstrap;
namespace contract = aexcompat::abi::x86_64_windows;

namespace {
bool timeline_seen_on_global{};
bool timeline_seen_on_params{};

template <typename T>
T read(const void* input, std::size_t offset) {
  T value{};
  std::memcpy(&value, static_cast<const std::byte*>(input) + offset,
              sizeof(value));
  return value;
}

bool timeline_matches(const void* input) {
  return read<int32_t>(input, contract::IN_CURRENT_TIME_OFFSET) == 6 &&
      read<int32_t>(input, contract::IN_TIME_STEP_OFFSET) == 2 &&
      read<int32_t>(input, contract::IN_TOTAL_TIME_OFFSET) == 240 &&
      read<int32_t>(input, contract::IN_LOCAL_TIME_STEP_OFFSET) == 2 &&
      read<uint32_t>(input, contract::IN_TIME_SCALE_OFFSET) == 60;
}

int32_t __cdecl effect_entry(int32_t, void*, void*, void**, void*, void*) {
  return 0;
}

int32_t invoke(boot::EffectEntry, int32_t selector, void* input, void* output,
               void**, void*, void*, uint32_t*) {
  if (selector == 1) {
    timeline_seen_on_global = timeline_matches(input);
    std::memset(output, 0, contract::PF_OUT_DATA_SIZE);
  } else if (selector == 4) {
    timeline_seen_on_params = timeline_matches(input);
    const int32_t count = 1;
    std::memcpy(static_cast<std::byte*>(output) +
                    contract::OUT_NUM_PARAMS_OFFSET,
                &count, sizeof(count));
  }
  return 0;
}

int32_t discovered_parameter_count() { return 0; }
}  // namespace

int main() {
  boot::State state{};
  boot::Request request{};
  request.rendering_worker = true;
  request.skip_about = true;
  request.current_time = 6;
  request.time_step = 2;
  request.total_time = 240;
  request.time_scale = 60;
  const boot::RuntimeHooks hooks{
      &invoke, +[](bool) {}, +[](bool) {}, +[](bool, bool) {},
      +[](boot::EffectEntry, boot::State&) {}, &discovered_parameter_count};
  const auto result = boot::run(state, &effect_entry, {}, request, hooks);
  const bool passed = result.global_error == 0 && result.params_error == 0 &&
      result.parameter_count_contract_valid && timeline_seen_on_global &&
      timeline_seen_on_params;
  std::cout << "{\"worker_effect_bootstrap_timing_selftest\":\""
            << (passed ? "passed" : "failed") << "\"}\n";
  return passed ? 0 : 1;
}
