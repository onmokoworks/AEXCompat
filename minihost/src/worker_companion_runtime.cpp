#include "worker_companion_runtime.hpp"

#include "worker_aegp_entry_guard.hpp"
#include "worker_aegp_init_runtime.hpp"
#include "worker_dynamic_suite_registry.hpp"

#include <algorithm>
#include <atomic>
#include <set>
#include <tuple>

namespace aexcompat::worker_runtime::companions {
namespace {
std::atomic_uint32_t g_active_runtimes{};
using DynamicIdentity = dynamic_suites::RegisteredSuiteIdentity;

auto identity_set(const std::vector<DynamicIdentity>& identities) {
  std::set<std::tuple<std::string, int32_t, int32_t>> result;
  for (const auto& identity : identities)
    result.insert(
        {identity.name, identity.api_version, identity.internal_version});
  return result;
}

auto declared_set(const Entry& entry) {
  std::set<std::tuple<std::string, int32_t, int32_t>> result;
  for (const auto& identity : entry.suites)
    result.insert(
        {identity.name, identity.api_version, identity.internal_version});
  return result;
}

}  // namespace

bool host_services_active() noexcept {
  return g_active_runtimes.load(std::memory_order_acquire) != 0;
}

Runtime::~Runtime() { (void)shutdown(); }

bool Runtime::initialize(const Manifest& manifest, void* basic_suite,
                         FileSha256 file_sha256) noexcept {
  if (!modules_.empty() || shutdown_attempted_ || !basic_suite || !file_sha256 ||
      manifest.entries.empty())
    return false;
  int32_t next_plugin_id = 1;
  for (const auto& entry : manifest.entries) {
    std::string actual;
    // Identity is evidence, never a launch gate.  The broker records the
    // selected bytes in the manifest and the worker observes the bytes it is
    // about to load, but a rebuild between those observations is a state
    // transition (#309/#678), not authorization failure.
    if (!file_sha256(entry.path, actual)) {
      (void)shutdown();
      return false;
    }
    const auto before = identity_set(dynamic_suites::registered_suites());
    HMODULE module = LoadLibraryExW(
        entry.path.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_USER_DIRS |
            LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
      (void)shutdown();
      return false;
    }
    const int32_t plugin_id = next_plugin_id++;
    const auto entrypoint =
        reinterpret_cast<aegp_entry_guard::EntryPoint>(
            GetProcAddress(module, "EntryPointFunc"));
    void* global_refcon{};
    const auto invoked = aegp_entry_guard::invoke(
        entrypoint, basic_suite, 24, 0, plugin_id, &global_refcon);
    modules_.push_back({module, plugin_id, global_refcon});
    const auto after = identity_set(dynamic_suites::registered_suites());
    auto added = after;
    for (const auto& identity : before) added.erase(identity);
    if (!entrypoint || invoked.error != 0 ||
        invoked.fault != aegp_entry_guard::FaultKind::none ||
        added != declared_set(entry)) {
      (void)shutdown();
      return false;
    }
  }
  g_active_runtimes.fetch_add(1, std::memory_order_release);
  host_services_enabled_ = true;
  return true;
}

bool Runtime::shutdown() noexcept {
  if (shutdown_attempted_) return modules_.empty();
  // PF lifecycle must have released every consumer lease before any provider
  // death hook runs or any provider-owned function table is discarded.
  if (!dynamic_suites::references_drained()) return false;
  shutdown_attempted_ = true;
  bool passed = true;
  for (auto it = modules_.rbegin(); it != modules_.rend(); ++it) {
    const auto death =
        aegp_init::dispatch_death_for_plugin(it->plugin_id, it->global_refcon);
    if (death.error != 0) passed = false;
    aegp_init::forget_plugin_registrations(it->plugin_id);
  }
  if (!dynamic_suites::drain()) return false;
  for (auto it = modules_.rbegin(); it != modules_.rend(); ++it)
    if (it->module && !FreeLibrary(it->module)) passed = false;
  modules_.clear();
  if (host_services_enabled_) {
    g_active_runtimes.fetch_sub(1, std::memory_order_release);
    host_services_enabled_ = false;
  }
  return passed;
}

}  // namespace aexcompat::worker_runtime::companions
