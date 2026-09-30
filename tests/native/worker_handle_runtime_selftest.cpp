#include "worker_handle_runtime.hpp"

#include "trace_writer.hpp"

#include <cstddef>
#include <cstdint>
#include <vector>

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

  std::vector<void**> regression_handles;
  regression_handles.reserve(1025);
  for (std::size_t index = 0; index < 1025; ++index) {
    void** handle = new_handle(0);
    if (!handle) return 7;
    regression_handles.push_back(handle);
  }
  for (void** handle : regression_handles) dispose_handle(handle);

  // A PF lifecycle may logically dispose a handle while plug-in teardown still
  // retains an opaque reference to its backing. Host callbacks must reject the
  // stale handle immediately, while physical reclamation waits for the explicit
  // post-GLOBAL_SETDOWN boundary and remains included in the allocation budget.
  {
    HandleReclamationScope reclamation_scope;
    void** quarantined = new_handle(32);
    void** resize_probe = new_handle(1);
    if (!quarantined || !resize_probe) return 8;
    auto* retained = static_cast<std::uint8_t*>(lock_handle(quarantined));
    if (!retained) return 9;
    retained[0] = 0x9a;
    unlock_handle(quarantined);
    dispose_handle(quarantined);
    if (resize_handle(kMaxHandleBytes, &resize_probe) != 4 ||
        handle_size(resize_probe) != 1)
      return 10;
    dispose_handle(resize_probe);
    const Statistics quarantined_stats = statistics();
    if (host_handle_is_live(quarantined) || lock_handle(quarantined) ||
        retained[0] != 0x9a || quarantined_stats.live_count != 0 ||
        quarantined_stats.live_bytes != 0 ||
        quarantined_stats.quarantined_count != 2 ||
        quarantined_stats.quarantined_bytes != 33 ||
        handle_lifetimes_balanced())
      return 10;
    reclamation_scope.reclaim();
    const Statistics reclaimed_stats = statistics();
    if (reclaimed_stats.quarantined_count != 0 ||
        reclaimed_stats.quarantined_bytes != 0 ||
        !handle_lifetimes_balanced())
      return 11;
  }

  // PF host_dispose_handle is a void callback. A live locked handle therefore
  // has to be reclaimed by the host rather than rejected with an error the
  // plug-in cannot observe. The outstanding lock is accounted separately from
  // an explicit host_unlock_handle callback.
  void** locked_dispose = new_handle(16);
  if (!locked_dispose || !lock_handle(locked_dispose)) return 12;
  dispose_handle(locked_dispose);
  if (host_handle_is_live(locked_dispose)) return 13;

  const Statistics before_invalid = statistics();
  dispose_handle(locked_dispose);  // stale / double dispose
  dispose_handle(nullptr);
  void* foreign_data = nullptr;
  dispose_handle(&foreign_data);
  const Statistics after_invalid = statistics();
  if (after_invalid.invalid_operations != before_invalid.invalid_operations + 3 ||
      after_invalid.disposed != before_invalid.disposed)
    return 14;

  const Statistics after = statistics();
  return handle_lifetimes_balanced() &&
          after.created == before.created + 1030 &&
          after.disposed == before.disposed + 1030 &&
          after.locks == before.locks + 5 &&
          after.unlocks == before.unlocks + 4 &&
          after.locks_released_on_dispose ==
              before.locks_released_on_dispose + 1 &&
          after.invalid_operations == before.invalid_operations + 5 &&
          after.live_count == 0 && after.live_bytes == 0 &&
          after.quarantined_count == 0 && after.quarantined_bytes == 0
      ? 0
      : 15;
}
