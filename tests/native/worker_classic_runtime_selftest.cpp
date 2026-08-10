#include "worker_classic_runtime.hpp"
#include "worker_active_plugin_context.hpp"
#include "worker_invocation_orchestration.hpp"

#include <atomic>
#include <cstddef>
#include <cstring>
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
int g_arbitrary_copy_calls{};
int g_arbitrary_dispose_calls{};
int g_arbitrary_source_token{};
int g_arbitrary_destination_token{};
int g_arbitrary_refcon_token{};
constexpr int16_t kArbitraryId = 73;

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

int32_t __cdecl copy_synthetic_arbitrary(
    int32_t command, void*, void*, void**, void*, void* extra) {
  if (command != 22 || !extra) return 4;
  auto* bytes = static_cast<std::byte*>(extra);
  int32_t which{};
  void* source{};
  void** destination{};
  int16_t id{};
  void* refcon{};
  std::memcpy(&which, bytes, sizeof(which));
  std::memcpy(&id, bytes + 4, sizeof(id));
  std::memcpy(&refcon, bytes + 8, sizeof(refcon));
  std::memcpy(&source, bytes + 16, sizeof(source));
  if (id != kArbitraryId || refcon != &g_arbitrary_refcon_token) return 4;
  if (which == 1) {
    if (source != &g_arbitrary_destination_token) return 4;
    ++g_arbitrary_dispose_calls;
    return 0;
  }
  std::memcpy(&destination, bytes + 24, sizeof(destination));
  if (which != 2 || source != &g_arbitrary_source_token || !destination) return 4;
  *destination = &g_arbitrary_destination_token;
  ++g_arbitrary_copy_calls;
  return 0;
}
int32_t invoke_synthetic_arbitrary(
    aexcompat::worker_runtime::parameter_execution::EffectEntry entry,
    int32_t command, void* input,
    void* output, void** params, void* world, void* extra,
    uint32_t* exception_code) {
  if (exception_code) *exception_code = 0;
  return entry(command, input, output, params, world, extra);
}
bool synthetic_handle_is_live(const void* value) { return value != nullptr; }
std::size_t no_active_masks() { return 0; }
bool no_active_mask_id(std::size_t, int32_t*) { return false; }
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

  // An arbitrary parameter may have no default handle.  It must remain null
  // without dispatching COPY, while a non-null default is still copied into a
  // distinct caller-owned handle.
  auto& parameter_state = parameters::state();
  parameter_state.records.assign(2, {});
  parameter_state.records[0].type = 11;
  parameter_state.records[1].type = 11;
  parameter_execution::Definitions arbitrary_definitions(3);
  void* source_value = &g_arbitrary_source_token;
  void* refcon_value = &g_arbitrary_refcon_token;
  std::memcpy(arbitrary_definitions[2].data() + 56,
              &kArbitraryId, sizeof(kArbitraryId));
  std::memcpy(arbitrary_definitions[2].data() + 64,
              &source_value, sizeof(source_value));
  std::memcpy(arbitrary_definitions[2].data() + 80,
              &refcon_value, sizeof(refcon_value));
  parameter_execution::BufferIn arbitrary_input{};
  parameter_execution::BufferOut arbitrary_output{};
  parameter_execution::configure_hooks({&invoke_synthetic_arbitrary,
      &synthetic_handle_is_live, &no_active_masks, &no_active_mask_id});
  if (!parameter_execution::initialize_arbitrary_values(
          &copy_synthetic_arbitrary, arbitrary_input, arbitrary_output,
          arbitrary_definitions) ||
      g_arbitrary_copy_calls != 1) return 4;
  void* null_value = reinterpret_cast<void*>(1);
  void* copied_value{};
  std::memcpy(&null_value, arbitrary_definitions[1].data() + 72,
              sizeof(null_value));
  std::memcpy(&copied_value, arbitrary_definitions[2].data() + 72,
              sizeof(copied_value));
  if (null_value != nullptr || copied_value != &g_arbitrary_destination_token)
    return 5;
  if (!parameter_execution::dispose_arbitrary_values(
          &copy_synthetic_arbitrary, arbitrary_input, arbitrary_output,
          arbitrary_definitions) ||
      g_arbitrary_dispose_calls != 1) return 6;
  std::memcpy(&copied_value, arbitrary_definitions[2].data() + 72,
              sizeof(copied_value));
  if (copied_value != nullptr) return 7;
  parameter_state.records.clear();

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
  parameter_state.records.assign(1, {});
  parameter_state.records[0].type = 11;
  parameter_execution::Definitions null_arbitrary_definitions(2);
  const auto interpolation_failures_before =
      parameter_state.arbitrary.interpolation_failures;
  const auto roundtrip_failures_before =
      parameter_state.arbitrary.roundtrip_failures;
  const bool null_arbitrary_passed =
      parameter_execution::interpolate_arbitrary_values(
          &copy_synthetic_arbitrary, arbitrary_input, arbitrary_output,
          null_arbitrary_definitions) &&
      parameter_execution::roundtrip_arbitrary_values(
          &copy_synthetic_arbitrary, arbitrary_input, arbitrary_output,
          null_arbitrary_definitions) &&
      parameter_state.arbitrary.interpolation_failures ==
          interpolation_failures_before &&
      parameter_state.arbitrary.roundtrip_failures == roundtrip_failures_before;
  parameter_state.records.clear();
  if (concurrent_context_passed && null_arbitrary_passed)
    std::cout << "{\"classic_runtime_selftest\":\"passed\"}\n";
  return concurrent_context_passed && null_arbitrary_passed ? 0 : 8;
}
