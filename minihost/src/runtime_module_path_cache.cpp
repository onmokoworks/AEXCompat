#include "runtime_module_path_cache.hpp"

#include <windows.h>

#include <atomic>

namespace aexcompat::worker_runtime::module_path_cache {
namespace {

using DllNotification = void(CALLBACK*)(ULONG, const void*, void*);
using RegisterDllNotification = LONG(NTAPI*)(ULONG, DllNotification, void*,
                                             void**);

class ProcessLoaderGeneration final {
 public:
  ProcessLoaderGeneration() noexcept {
    const HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    if (!ntdll) return;
    const auto register_notification =
        reinterpret_cast<RegisterDllNotification>(
            GetProcAddress(ntdll, "LdrRegisterDllNotification"));
    if (!register_notification) return;
    void* cookie = nullptr;
    if (register_notification(0, &on_notification, this, &cookie) != 0 ||
        !cookie)
      return;
    // The callback remains registered for the process lifetime. In particular,
    // it is not unregistered during CRT teardown while DLL unloads can still
    // occur and call it.
    cookie_ = cookie;
    available_ = true;
  }

  bool available() const noexcept { return available_; }
  uint64_t generation() const noexcept {
    return generation_.load(std::memory_order_acquire);
  }

 private:
  static void CALLBACK on_notification(ULONG, const void*,
                                       void* context) noexcept {
    static_cast<ProcessLoaderGeneration*>(context)->generation_.fetch_add(
        1, std::memory_order_acq_rel);
  }

  static_assert(std::atomic<uint64_t>::is_always_lock_free,
                "the loader callback may only perform a lock-free increment");
  std::atomic<uint64_t> generation_{1};
  void* cookie_{};
  bool available_{};
};

ProcessLoaderGeneration& process_loader_generation() {
  // Deliberately process-lifetime: unregistering during static teardown would
  // race the remaining loader teardown notifications. The operating system
  // reclaims this single object with the process.
  static ProcessLoaderGeneration* const generation =
      new ProcessLoaderGeneration();
  return *generation;
}

}  // namespace

std::optional<uint64_t> current_loader_generation() {
  ProcessLoaderGeneration& generation = process_loader_generation();
  if (!generation.available()) return std::nullopt;
  return generation.generation();
}

}  // namespace aexcompat::worker_runtime::module_path_cache
