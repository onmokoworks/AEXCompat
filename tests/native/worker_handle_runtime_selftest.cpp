#include "worker_handle_runtime.hpp"

#include "trace_writer.hpp"

#include <cstddef>
#include <cstdint>

namespace aexcompat::l2_detail {
aexcompat::TraceWriter* g_trace_writer{};
}

using namespace aexcompat::worker_runtime::handles;

int main() {
  const Statistics before = statistics();

  void** large = new_handle(kObservedLargeHandleBytes);
  if (!large || handle_size(large) != kObservedLargeHandleBytes) return 1;
  auto* large_data = static_cast<std::uint8_t*>(lock_handle(large));
  if (!large_data) return 2;
  large_data[0] = 0x12;
  large_data[kObservedLargeHandleBytes - 1] = 0x34;
  unlock_handle(large);
  dispose_handle(large);

  void** resized = new_handle(32);
  if (!resized) return 3;
  auto* original = static_cast<std::uint8_t*>(lock_handle(resized));
  if (!original) return 4;
  original[0] = 0x56;
  original[31] = 0x78;
  unlock_handle(resized);
  void** stable_handle = resized;
  if (resize_handle(64, &resized) != 0 || resized != stable_handle ||
      handle_size(resized) != 64)
    return 5;
  auto* replacement = static_cast<std::uint8_t*>(lock_handle(resized));
  if (!replacement || replacement[0] != 0x56 || replacement[31] != 0x78 ||
      replacement[63] != 0)
    return 6;
  unlock_handle(resized);
  dispose_handle(resized);

  const Statistics after = statistics();
  return handle_lifetimes_balanced() &&
          after.created == before.created + 2 &&
          after.disposed == before.disposed + 2 &&
          after.locks == before.locks + 3 &&
          after.unlocks == before.unlocks + 3 &&
          after.invalid_operations == before.invalid_operations &&
          after.live_count == 0 && after.live_bytes == 0
      ? 0
      : 7;
}
