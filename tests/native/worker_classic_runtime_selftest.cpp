#include "worker_classic_runtime.hpp"
#include "worker_classic_render_entry.hpp"
#include "worker_active_plugin_context.hpp"
#include "worker_invocation_orchestration.hpp"
#include "worker_selector_dispatch.hpp"

#include <atomic>
#include <cstddef>
#include <cstring>
#include <iostream>
#include <mutex>
#include <thread>
#include <vector>

using namespace aexcompat::worker_runtime::classic;

extern "C" int32_t __cdecl report_progress(void*, int32_t, int32_t);

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
int32_t g_expected_frame_time{};
int32_t g_previous_frame_time{};
int g_frame_setup_time_observations{};
bool g_expect_frame_wide_time{};
bool g_expect_frame_shutter_dependency{};
bool g_advertise_dynamic_wide_time{};
int g_render_wide_time_observations{};
int g_checkout_token{};
int g_selector_result{};
int g_cleanup_result{};
bool g_explicit_checkin{};
bool g_invalid_double_checkin{};

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
void capture_clean_audit() {}
bool audit_stays_clean() { return true; }

int render_with_parameter_checkout(void*) {
  auto* context = active_context();
  if (!context) return 4;
  context->record_checkout(&g_checkout_token, 1, 0, 1, 24);
  if (g_explicit_checkin && context->checkin(&g_checkout_token) != 0) return 4;
  if (g_invalid_double_checkin) return context->checkin(&g_checkout_token);
  return g_selector_result;
}

int cleanup_after_parameter_checkout(void*) { return g_cleanup_result; }
bool dependencies_are_ready(void*) { return true; }

int32_t __cdecl observe_frame_setup_checkout_time(
    int32_t command, void*, void* output, void**, void*, void*) {
  if (command == 18 && g_advertise_dynamic_wide_time) {
    constexpr uint32_t kWideTimeInput = 1u << 1;
    std::memcpy(static_cast<std::byte*>(output) + 96, &kWideTimeInput,
                sizeof(kWideTimeInput));
    return 0;
  }
  if (command == 11 && g_advertise_dynamic_wide_time) {
    auto* context = active_context();
    if (!context || !context->checkout_time_allowed(g_expected_frame_time + 1, 24))
      return 4;
    ++g_render_wide_time_observations;
    return 0;
  }
  if (command != 10) return 0;
  auto* context = active_context();
  if (!context || !context->checkout_time_allowed(g_expected_frame_time, 24))
    return 4;
  if (context->shutter_dependency_advertised() !=
      g_expect_frame_shutter_dependency)
    return 4;
  const bool foreign_allowed =
      context->checkout_time_allowed(g_expected_frame_time + 1, 24);
  if (!foreign_allowed ||
      (g_previous_frame_time != 0 &&
       !context->checkout_time_allowed(g_previous_frame_time, 24)))
    return 4;
  ++g_frame_setup_time_observations;
  return 0;
}
}  // namespace

int main() {
  // PF_PROGRESS is an abort poll, not a validated ratio: AE-shipped effects
  // report current=-1 (PW, issue #1079), total=0 (Write-on, issue #1055), and
  // current>total (Wave Warp, issue #1037), and all render in AE. The host
  // accepts and clamps; only a null effect_ref stays refused. A non-positive
  // total leaves the last-progress telemetry untouched (no ratio to record).
  {
    auto& telemetry = host_callback_telemetry();
    int marker{};
    if (report_progress(nullptr, 1, 2) != 4) return 40;
    if (report_progress(&marker, -1, 10) != 0 ||
        telemetry.last_progress_current != 0 ||
        telemetry.last_progress_total != 10) return 41;
    if (report_progress(&marker, 11, 10) != 0 ||
        telemetry.last_progress_current != 10 ||
        telemetry.last_progress_total != 10) return 42;
    if (report_progress(&marker, 5, 0) != 0 ||
        telemetry.last_progress_current != 10 ||
        telemetry.last_progress_total != 10) return 43;
    if (report_progress(&marker, 5, -3) != 0 ||
        telemetry.last_progress_total != 10) return 44;
    if (telemetry.progress_calls != 4) return 45;
  }
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
        !context.checkout_time_allowed(foreign_slot, 24))
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
      result.rejected_temporal_checkouts == 0 && result.balanced &&
      result.shutter_dependency_advertised &&
      last_selector_dispatched())) return 3;

  // The shipping Classic dispatch boundary must reclaim a successful
  // checkout left live by the selector, while preserving the primary selector
  // error. An explicit plug-in checkin remains explicit rather than being
  // counted as host reclamation, and a cleanup error remains observable when
  // the selector itself succeeded.
  reset_selector_diagnostic();
  g_selector_result = 37;
  g_cleanup_result = 41;
  g_explicit_checkin = false;
  g_invalid_double_checkin = false;
  Request leaked_checkout_request{
      nullptr,
      {&render_with_parameter_checkout, &cleanup_after_parameter_checkout,
       &dependencies_are_ready},
      false};
  if (dispatch(leaked_checkout_request) != 37) return 34;
  const auto reclaimed = diagnostics();
  if (reclaimed.checkout_calls != 1 || reclaimed.checkin_calls != 1 ||
      reclaimed.automatic_checkins != 1 || reclaimed.invalid_checkins != 0 ||
      !reclaimed.balanced)
    return 35;

  reset_selector_diagnostic();
  g_selector_result = 0;
  g_cleanup_result = 41;
  g_explicit_checkin = true;
  if (dispatch(leaked_checkout_request) != 41) return 36;
  const auto explicit_checkin = diagnostics();
  if (explicit_checkin.checkout_calls != 1 ||
      explicit_checkin.checkin_calls != 1 ||
      explicit_checkin.automatic_checkins != 0 ||
      explicit_checkin.invalid_checkins != 0 || !explicit_checkin.balanced)
    return 37;

  reset_selector_diagnostic();
  g_cleanup_result = 0;
  g_explicit_checkin = true;
  g_invalid_double_checkin = true;
  if (dispatch(leaked_checkout_request) != 4) return 38;
  const auto invalid_checkin = diagnostics();
  if (invalid_checkin.checkout_calls != 1 || invalid_checkin.checkin_calls != 1 ||
      invalid_checkin.automatic_checkins != 0 ||
      invalid_checkin.invalid_checkins != 1 || invalid_checkin.balanced)
    return 39;

  // Exercise the shipping render_once boundary: FRAME_SETUP must observe the
  // current nonzero frame time, not Context's default or the previous frame.
  aexcompat::l2_detail::BufferIn frame_input{};
  aexcompat::l2_detail::BufferOut frame_output{};
  int32_t frame_width{}, frame_height{}, frame_rowbytes{};
  std::string frame_input_hash, frame_output_hash;
  bool frame_guards{};
  aexcompat::worker_runtime::configure_selector_dispatch_audit(
      &capture_clean_audit, &audit_stays_clean);
  g_expected_frame_time = 37;
  if (aexcompat::l2_detail::render_once(
          &observe_frame_setup_checkout_time, frame_input, frame_output, "default",
          frame_width, frame_height, frame_rowbytes, frame_input_hash,
          frame_output_hash, frame_guards, nullptr, nullptr, 0, 0, nullptr,
          g_expected_frame_time, 1, 100, 24) != 0 ||
      g_frame_setup_time_observations != 1)
    return 30;
  g_previous_frame_time = g_expected_frame_time;
  g_expected_frame_time = 41;
  if (aexcompat::l2_detail::render_once(
          &observe_frame_setup_checkout_time, frame_input, frame_output, "default",
          frame_width, frame_height, frame_rowbytes, frame_input_hash,
          frame_output_hash, frame_guards, nullptr, nullptr, 0, 0, nullptr,
          g_expected_frame_time, 1, 100, 24) != 0 ||
      g_frame_setup_time_observations != 2)
    return 31;
  constexpr uint32_t kWideTimeInput = 1u << 1;
  g_previous_frame_time = g_expected_frame_time;
  g_expected_frame_time = 47;
  g_expect_frame_wide_time = true;
  g_expect_frame_shutter_dependency = true;
  constexpr uint32_t kUsesShutterAngle = 1u << 19;
  constexpr uint32_t kWideTimeAndShutter = kWideTimeInput | kUsesShutterAngle;
  std::memcpy(frame_output.data() + 96, &kWideTimeAndShutter,
              sizeof(kWideTimeAndShutter));
  if (aexcompat::l2_detail::render_once(
          &observe_frame_setup_checkout_time, frame_input, frame_output, "default",
          frame_width, frame_height, frame_rowbytes, frame_input_hash,
          frame_output_hash, frame_guards, nullptr, nullptr, 0, 0, nullptr,
          g_expected_frame_time, 1, 100, 24) != 0 ||
      g_frame_setup_time_observations != 3)
    return 32;
  frame_output = {};
  g_expect_frame_shutter_dependency = false;
  g_previous_frame_time = g_expected_frame_time;
  g_expected_frame_time = 53;
  g_expect_frame_wide_time = false;
  g_advertise_dynamic_wide_time = true;
  aexcompat::worker_runtime::parameters::state().ui.dynamic_flags_advertised = true;
  if (aexcompat::l2_detail::render_once(
          &observe_frame_setup_checkout_time, frame_input, frame_output, "default",
          frame_width, frame_height, frame_rowbytes, frame_input_hash,
          frame_output_hash, frame_guards, nullptr, nullptr, 0, 0, nullptr,
          g_expected_frame_time, 1, 100, 24) != 0 ||
      g_frame_setup_time_observations != 4 ||
      g_render_wide_time_observations != 1)
    return 33;
  aexcompat::worker_runtime::parameters::state().ui.dynamic_flags_advertised = false;

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
