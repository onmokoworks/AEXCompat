// SDK-free fixture for discovery's session-wide deferred reclamation contract.
#include "generated/aex_abi_contract.hpp"
#include "worker_handle_runtime.hpp"

#include <cstdint>
#include <cstring>

namespace {
using Handle = void**;
struct BasicSuite {
  int32_t (__cdecl* acquire)(const char*, int32_t, const void**);
  int32_t (__cdecl* release)(const char*, int32_t);
};
struct HandleSuite {
  Handle (__cdecl* create)(uint64_t);
  void* (__cdecl* lock)(Handle);
  void (__cdecl* unlock)(Handle);
  void (__cdecl* dispose)(Handle);
};
Handle last_disposed{};
}

extern "C" __declspec(dllexport) int32_t __cdecl EffectMain(
    int32_t command, void* input, void* output, void**, void*, void*) {
  namespace abi = aexcompat::abi::x86_64_windows;
  if (command == 4) {
    const int32_t count = 1;  // input only
    std::memcpy(static_cast<std::byte*>(output) + abi::OUT_NUM_PARAMS_OFFSET,
                &count, sizeof(count));
    return 0;
  }
  if (command != 1 && command != 3) return 0;
  const BasicSuite* basic{};
  std::memcpy(&basic, static_cast<std::byte*>(input) + abi::IN_PICA_BASICP_OFFSET,
              sizeof(basic));
  const void* acquired{};
  if (!basic || basic->acquire("PF Handle Suite", 2, &acquired) != 0 || !acquired)
    return 4;
  const auto* handles = static_cast<const HandleSuite*>(acquired);
  int32_t error = 0;
  if (command == 1) {
    // Four successful columns consume the bounded quarantine. A fifth must
    // fail allocation, even though all handles are already logically disposed.
    // This detects missing or per-column reclamation scopes without reading
    // freed memory, relying on allocator address reuse, or proprietary AEX.
    constexpr auto count = aexcompat::worker_runtime::handles::kMaxHandleCount / 4;
    static_assert(count > 0 && count * 4 == aexcompat::worker_runtime::handles::kMaxHandleCount);
    for (std::size_t i = 0; i < count; ++i) {
      Handle handle = handles->create(1);
      if (!handle) { error = 4; break; }
      handles->dispose(handle);
      last_disposed = handle;
    }
  } else if (!last_disposed || handles->lock(last_disposed) != nullptr) {
    // Quarantining storage never revives a disposed host handle.
    error = 4;
  }
  const int32_t released = basic->release("PF Handle Suite", 2);
  return error != 0 ? error : released;
}
