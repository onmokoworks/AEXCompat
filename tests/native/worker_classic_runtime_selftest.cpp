#include "worker_classic_runtime.hpp"

#include <atomic>
#include <cstddef>
#include <thread>

using namespace aexcompat::worker_runtime::classic;

int main() {
  ParameterDefinition outer_definition{};
  outer_definition[0] = std::byte{0x11};
  Context outer;
  outer.set_definition(1, outer_definition);
  {
    ParameterDefinition nested_definition{};
    nested_definition[0] = std::byte{0x22};
    Context nested;
    nested.set_definition(2, nested_definition);
    ParameterDefinition copied{};
    if (active_context() != &nested ||
        !nested.copy_definition(2, copied.data(), copied.size()) ||
        copied[0] != std::byte{0x22} ||
        nested.copy_definition(1, copied.data(), copied.size())) return 1;
  }
  ParameterDefinition copied{};
  if (active_context() != &outer ||
      !outer.copy_definition(1, copied.data(), copied.size()) ||
      copied[0] != std::byte{0x11}) return 2;

  reset_selector_diagnostic();
  std::atomic<int> ready{};
  std::atomic_bool go{};
  std::atomic_bool isolated{true};
  auto run = [&](int32_t own_slot, int32_t foreign_slot, std::byte marker,
                 bool dispatch_selector) {
    Context context;
    context.configure_checkout_time(own_slot, 24, false);
    ParameterDefinition definition{};
    definition[0] = marker;
    context.set_definition(own_slot, definition);
    ready.fetch_add(1, std::memory_order_release);
    while (!go.load(std::memory_order_acquire)) std::this_thread::yield();
    ParameterDefinition local{};
    if (!context.copy_definition(own_slot, local.data(), local.size()) ||
        local[0] != marker ||
        context.copy_definition(foreign_slot, local.data(), local.size()) ||
        !context.checkout_time_allowed(own_slot, 24) ||
        context.checkout_time_allowed(foreign_slot, 24))
      isolated.store(false, std::memory_order_relaxed);
    context.record_checkout(local.data(), own_slot, own_slot, 1, 24);
    if (context.checkin(local.data()) != 0 || !context.checkouts_balanced())
      isolated.store(false, std::memory_order_relaxed);
    if (dispatch_selector) context.mark_selector_dispatched();
  };
  std::thread first(run, 7, 9, std::byte{0x77}, false);
  std::thread second(run, 9, 7, std::byte{0x99}, true);
  while (ready.load(std::memory_order_acquire) != 2) std::this_thread::yield();
  go.store(true, std::memory_order_release);
  first.join();
  second.join();
  bool off_thread_failed_closed{};
  std::thread off_thread([&] {
    off_thread_failed_closed = active_context() == nullptr && dispatch_active();
  });
  off_thread.join();
  const auto result = diagnostics();
  return isolated.load(std::memory_order_relaxed) && off_thread_failed_closed &&
      result.checkout_calls == 2 && result.checkin_calls == 2 &&
      result.rejected_temporal_checkouts == 2 && result.balanced &&
      last_selector_dispatched() ? 0 : 3;
}
