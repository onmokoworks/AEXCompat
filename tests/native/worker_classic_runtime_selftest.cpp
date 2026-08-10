#include "worker_classic_runtime.hpp"
#include "worker_active_plugin_context.hpp"
#include "worker_invocation_orchestration.hpp"

#include <atomic>
#include <cstddef>
#include <iostream>
#include <mutex>
#include <thread>
#include <vector>

using namespace aexcompat::worker_runtime::classic;

namespace {
std::atomic<int> g_context_observations{};
std::atomic<int> g_table_mismatches{};
std::atomic<int> g_module_mismatches{};
const aexcompat::aex_strings::StringTable* g_expected_table{};
HMODULE g_expected_module{};
std::mutex g_thread_ids_mutex;
std::vector<std::thread::id> g_thread_ids;

void observe_concurrent_render_context() {
  using namespace aexcompat::worker_runtime;
  if (active_plugin::string_table != g_expected_table) {
    g_table_mismatches.fetch_add(1);
    return;
  }
  if (active_plugin::effect_module != g_expected_module) {
    g_module_mismatches.fetch_add(1);
    return;
  }
  g_context_observations.fetch_add(1, std::memory_order_relaxed);
  std::lock_guard lock(g_thread_ids_mutex);
  g_thread_ids.push_back(std::this_thread::get_id());
}

int32_t __cdecl fail_synthetic_render(
    int32_t, void*, void*, void**, void*, void*) { return 4; }
}  // namespace

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
    context.configure_checkout_time(own_slot, 24, false, dispatch_selector);
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
  if (!(isolated.load(std::memory_order_relaxed) && off_thread_failed_closed &&
      result.checkout_calls == 2 && result.checkin_calls == 2 &&
      result.rejected_temporal_checkouts == 2 && result.balanced &&
      result.shutter_dependency_advertised &&
      last_selector_dispatched())) return 3;

  using namespace aexcompat::worker_runtime;
  aexcompat::aex_strings::StringTable caller_table;
  g_expected_table = &caller_table;
  g_expected_module = reinterpret_cast<HMODULE>(static_cast<uintptr_t>(0x1082));
  active_plugin::Scope caller_context(g_expected_table, g_expected_module);

  parameter_execution::BufferIn input{};
  parameter_execution::BufferOut output{};
  invocation::InvocationState invocation_state{};
  wchar_t arg0[] = L"worker";
  wchar_t arg1[] = L"plugin";
  wchar_t arg2[] = L"hash";
  wchar_t arg3[] = L"unused";
  wchar_t arg4[] = L"threaded_default";
  wchar_t* argv[] = {arg0, arg1, arg2, arg3, arg4};
  invocation::FinalDispatchRequest request{};
  request.entry = &fail_synthetic_render;
  request.input = &input;
  request.output = &output;
  request.invocation = &invocation_state;
  request.argv = argv;
  request.params_error = 0;
  request.image_render_supported = true;
  request.depth_supported = true;
  request.concurrent_thread_context_probe = &observe_concurrent_render_context;

  const auto dispatch = invocation::run_classic_final_dispatch(request);
  const bool two_distinct_render_threads = g_thread_ids.size() == 2 &&
      g_thread_ids[0] != g_thread_ids[1] &&
      g_thread_ids[0] != std::this_thread::get_id() &&
      g_thread_ids[1] != std::this_thread::get_id();
  if (!(dispatch.concurrent_render &&
        g_context_observations.load(std::memory_order_relaxed) == 2 &&
        two_distinct_render_threads &&
        active_plugin::string_table == g_expected_table &&
        active_plugin::effect_module == g_expected_module)) {
    std::cerr << "concurrent=" << dispatch.concurrent_render
              << " observations=" << g_context_observations.load()
              << " table_mismatch=" << g_table_mismatches.load()
              << " module_mismatch=" << g_module_mismatches.load()
              << " ids=" << g_thread_ids.size()
              << " distinct=" << two_distinct_render_threads
              << " table=" << (active_plugin::string_table == g_expected_table)
              << " module=" << (active_plugin::effect_module == g_expected_module)
              << "\n";
  }
  const bool concurrent_context_passed = dispatch.concurrent_render &&
      g_context_observations.load(std::memory_order_relaxed) == 2 &&
      two_distinct_render_threads &&
      active_plugin::string_table == g_expected_table &&
      active_plugin::effect_module == g_expected_module;
  if (concurrent_context_passed)
    std::cout << "{\"classic_runtime_selftest\":\"passed\"}\n";
  return concurrent_context_passed ? 0 : 4;
}
