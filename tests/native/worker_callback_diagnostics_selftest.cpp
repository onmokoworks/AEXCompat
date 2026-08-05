#include "worker_callback_diagnostics.hpp"
#include "worker_smart_runtime.hpp"

#include <cstdint>
#include <iostream>
#include <limits>

int main() {
  using namespace aexcompat::worker_runtime::smart;
  std::atomic<uint32_t> saturated{std::numeric_limits<uint32_t>::max()};
  aexcompat::callback_diagnostics::increment_saturating(saturated);
  if (saturated.load() != std::numeric_limits<uint32_t>::max()) return 5;
  aexcompat::callback_diagnostics::reset();
  for (std::size_t callback = 0;
       callback < static_cast<std::size_t>(aexcompat::callback_diagnostics::Callback::Count);
       ++callback)
    for (std::size_t reason = 1;
         reason < static_cast<std::size_t>(aexcompat::callback_diagnostics::Reason::Count);
         ++reason)
      aexcompat::callback_diagnostics::record(
          static_cast<aexcompat::callback_diagnostics::Callback>(callback), 4,
          static_cast<aexcompat::callback_diagnostics::Reason>(reason));
  if (aexcompat::callback_diagnostics::snapshot_json().size() > 32 * 1024) return 6;
  aexcompat::callback_diagnostics::reset();
  {
    Session session;
    auto& runtime = state();
    runtime.current_time = 0;
    runtime.current_time_scale = 1;
    runtime.output_world = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1000));

    void* world = reinterpret_cast<void*>(static_cast<uintptr_t>(1));
    if (checkout_output(nullptr, &world) != 0 || world != runtime.output_world) return 1;
    if (checkout_output(nullptr, nullptr) != 4) return 2;
    if (checkout_pixels(nullptr, 404, &world) != 4 || world != nullptr) return 3;
    if (pre_checkout_layer(nullptr, 0, 0, nullptr, 1, 1, 1, &world) != 4) return 4;
  }
  std::cout << "{\"schema_version\":1"
            << aexcompat::callback_diagnostics::report_field_json() << "}\n";
  return 0;
}
