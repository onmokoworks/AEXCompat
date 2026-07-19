#include "worker_pf_effect_sequence_selftests.hpp"

#include <atomic>
#include <thread>
#include <vector>

namespace aexcompat::pf_effect_sequence_selftests {

bool verify_suite1(void* effect_ref, const Hooks& hooks) {
  if (!effect_ref || !hooks.acquire_suite || !hooks.release_suite || !hooks.suite)
    return false;
  using namespace pf_state_runtime;
  invalidate_effect_sequence(effect_ref);
  const void* acquired{};
  PfConstHandle observed = reinterpret_cast<PfConstHandle>(1);
  int payload = 0x53455131;
  void* handle_value = &payload;
  void** handle = &handle_value;
  uint32_t foreign_owner = 0x4652474e;
  bool ok = hooks.acquire_suite("PF Effect Sequence Data Suite", 1, &acquired) == 0 &&
      acquired == hooks.suite &&
      hooks.suite->get_effect_sequence_data(effect_ref, &observed) ==
          hooks.bad_callback_param && observed == nullptr &&
      publish_effect_sequence(effect_ref, handle) &&
      hooks.suite->get_effect_sequence_data(effect_ref, &observed) == 0 &&
      observed == reinterpret_cast<PfConstHandle>(handle) && *observed == &payload;
  std::atomic<bool> concurrent_ok{true};
  std::vector<std::thread> readers;
  for (int thread_index = 0; thread_index < 8; ++thread_index) {
    readers.emplace_back([&] {
      for (int iteration = 0; iteration < 256; ++iteration) {
        PfConstHandle concurrent_observed{};
        if (hooks.suite->get_effect_sequence_data(effect_ref, &concurrent_observed) != 0 ||
            concurrent_observed != reinterpret_cast<PfConstHandle>(handle) ||
            *concurrent_observed != &payload) {
          concurrent_ok.store(false, std::memory_order_relaxed);
          break;
        }
      }
    });
  }
  for (auto& reader : readers) reader.join();
  ok = ok && concurrent_ok.load(std::memory_order_relaxed);
  PfConstHandle foreign_observed = reinterpret_cast<PfConstHandle>(1);
  ok = ok && hooks.suite->get_effect_sequence_data(
                    &foreign_owner, &foreign_observed) == hooks.bad_callback_param &&
      foreign_observed == nullptr &&
      hooks.suite->get_effect_sequence_data(nullptr, &observed) ==
          hooks.bad_callback_param &&
      hooks.suite->get_effect_sequence_data(effect_ref, nullptr) ==
          hooks.bad_callback_param;
  invalidate_effect_sequence(effect_ref);
  observed = reinterpret_cast<PfConstHandle>(1);
  ok = ok && hooks.suite->get_effect_sequence_data(effect_ref, &observed) ==
                    hooks.bad_callback_param && observed == nullptr &&
      hooks.release_suite("PF Effect Sequence Data Suite", 1) == 0;
  return ok;
}

}  // namespace aexcompat::pf_effect_sequence_selftests
