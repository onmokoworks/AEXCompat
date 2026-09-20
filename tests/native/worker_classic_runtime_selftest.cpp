#include "worker_classic_runtime.hpp"
#include "worker_classic_render_entry.hpp"
#include "worker_active_plugin_context.hpp"
#include "generated/aex_abi_contract.hpp"
#include "worker_invocation_orchestration.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_request_parser.hpp"
#include "worker_smart_execution.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstring>
#include <initializer_list>
#include <iostream>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

using namespace aexcompat::worker_runtime::classic;

// worker_l2_suite_abi.hpp declares report_progress in the global namespace,
// but that header is not self-contained (no <cstdint>) and carries a dozen
// unrelated callback declarations, so the one declaration this harness needs
// is repeated verbatim instead.
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
int32_t g_arbitrary_source_token{17};
int g_arbitrary_refcon_token{};
constexpr int16_t kArbitraryId = 73;
struct SyntheticArbitraryHandle {
  int32_t value{};
  bool live{true};
};
std::vector<std::unique_ptr<SyntheticArbitraryHandle>> g_arbitrary_handles;
std::array<int, 11> g_arbitrary_opcode_calls{};
std::array<bool, 11> g_arbitrary_time_checked{};
int32_t g_arbitrary_expected_current_time{};
int32_t g_arbitrary_expected_time_step{1};
int32_t g_arbitrary_expected_total_time{1};
uint32_t g_arbitrary_expected_time_scale{1};
int g_arbitrary_time_observations{};
int g_arbitrary_time_mismatches{};
int g_arbitrary_fail_opcode{-1};
int g_arbitrary_snapshot_observations{};
int32_t g_arbitrary_snapshot_value{};
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
int g_frame_setup_geometry_render_observations{};
int32_t g_frame_setup_offered_width{};
int32_t g_frame_setup_offered_height{};

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

template <typename T>
T read_synthetic_field(const void* bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, static_cast<const std::byte*>(bytes) + offset,
              sizeof(value));
  return value;
}

SyntheticArbitraryHandle* synthetic_handle(const void* value) {
  const auto found = std::find_if(
      g_arbitrary_handles.begin(), g_arbitrary_handles.end(),
      [value](const auto& candidate) { return candidate.get() == value; });
  return found == g_arbitrary_handles.end() ? nullptr : found->get();
}

void* create_synthetic_handle(int32_t value) {
  auto handle = std::make_unique<SyntheticArbitraryHandle>();
  handle->value = value;
  void* result = handle.get();
  g_arbitrary_handles.push_back(std::move(handle));
  return result;
}

bool synthetic_value(const void* value, int32_t* result) {
  if (!result) return false;
  if (value == &g_arbitrary_source_token) {
    *result = g_arbitrary_source_token;
    return true;
  }
  const auto* handle = synthetic_handle(value);
  if (!handle || !handle->live) return false;
  *result = handle->value;
  return true;
}

std::size_t live_synthetic_handle_count() {
  return static_cast<std::size_t>(std::count_if(
      g_arbitrary_handles.begin(), g_arbitrary_handles.end(),
      [](const auto& handle) { return handle->live; }));
}

void reset_synthetic_arbitrary(int32_t source_value = 17) {
  g_arbitrary_handles.clear();
  g_arbitrary_opcode_calls.fill(0);
  g_arbitrary_time_checked.fill(false);
  g_arbitrary_source_token = source_value;
  g_arbitrary_copy_calls = 0;
  g_arbitrary_dispose_calls = 0;
  g_arbitrary_expected_current_time = 0;
  g_arbitrary_expected_time_step = 1;
  g_arbitrary_expected_total_time = 1;
  g_arbitrary_expected_time_scale = 1;
  g_arbitrary_time_observations = 0;
  g_arbitrary_time_mismatches = 0;
  g_arbitrary_fail_opcode = -1;
  g_arbitrary_snapshot_observations = 0;
  g_arbitrary_snapshot_value = 0;
}

void expect_synthetic_time(int32_t current_time, int32_t time_step,
                           int32_t total_time, uint32_t time_scale,
                           std::initializer_list<int> opcodes) {
  g_arbitrary_expected_current_time = current_time;
  g_arbitrary_expected_time_step = time_step;
  g_arbitrary_expected_total_time = total_time;
  g_arbitrary_expected_time_scale = time_scale;
  for (const int opcode : opcodes)
    if (opcode >= 0 && opcode < static_cast<int>(g_arbitrary_time_checked.size()))
      g_arbitrary_time_checked[static_cast<std::size_t>(opcode)] = true;
}

void observe_synthetic_time(int opcode, const void* input) {
  if (opcode < 0 || opcode >= static_cast<int>(g_arbitrary_time_checked.size()) ||
      !g_arbitrary_time_checked[static_cast<std::size_t>(opcode)])
    return;
  ++g_arbitrary_time_observations;
  namespace contract = aexcompat::abi::x86_64_windows;
  if (!input ||
      read_synthetic_field<int32_t>(input, contract::IN_CURRENT_TIME_OFFSET) !=
          g_arbitrary_expected_current_time ||
      read_synthetic_field<int32_t>(input, contract::IN_TIME_STEP_OFFSET) !=
          g_arbitrary_expected_time_step ||
      read_synthetic_field<int32_t>(input, contract::IN_TOTAL_TIME_OFFSET) !=
          g_arbitrary_expected_total_time ||
      read_synthetic_field<uint32_t>(input, contract::IN_TIME_SCALE_OFFSET) !=
          g_arbitrary_expected_time_scale)
    ++g_arbitrary_time_mismatches;
}

bool parse_synthetic_integer(const char* text, uint32_t length, int32_t* value) {
  if (!text || !value || length == 0) return false;
  bool negative = false;
  std::size_t offset = 0;
  if (text[0] == '-') {
    negative = true;
    offset = 1;
  }
  if (offset == length) return false;
  int64_t parsed = 0;
  for (; offset < length; ++offset) {
    if (text[offset] < '0' || text[offset] > '9') return false;
    parsed = parsed * 10 + (text[offset] - '0');
    if (parsed > 0x7fffffffLL + static_cast<int64_t>(negative)) return false;
  }
  *value = static_cast<int32_t>(negative ? -parsed : parsed);
  return true;
}

int32_t __cdecl copy_synthetic_arbitrary(
    int32_t command, void* input, void*, void** params, void*, void* extra) {
  namespace contract = aexcompat::abi::x86_64_windows;
  if (command != 22) {
    if (command == contract::PF_CMD_FRAME_SETUP && params && params[1]) {
      void* value = read_synthetic_field<void*>(params[1], 72);
      int32_t decoded{};
      if (synthetic_value(value, &decoded)) {
        ++g_arbitrary_snapshot_observations;
        g_arbitrary_snapshot_value = decoded;
      }
    }
    return 0;
  }
  if (!extra) return 4;
  auto* bytes = static_cast<std::byte*>(extra);
  const int32_t which = read_synthetic_field<int32_t>(bytes, 0);
  const int16_t id = read_synthetic_field<int16_t>(bytes, 4);
  void* refcon = read_synthetic_field<void*>(bytes, 8);
  if (id != kArbitraryId || refcon != &g_arbitrary_refcon_token) return 4;
  if (which < 0 || which >= static_cast<int32_t>(g_arbitrary_opcode_calls.size()))
    return 4;
  ++g_arbitrary_opcode_calls[static_cast<std::size_t>(which)];
  observe_synthetic_time(which, input);
  if (which == g_arbitrary_fail_opcode) return 4;

  switch (which) {
    case 0: {  // NEW
      auto** destination = read_synthetic_field<void**>(bytes, 16);
      if (!destination) return 4;
      *destination = create_synthetic_handle(0);
      return 0;
    }
    case 1: {  // DISPOSE
      void* source = read_synthetic_field<void*>(bytes, 16);
      auto* handle = synthetic_handle(source);
      if (!handle || !handle->live) return 4;
      handle->live = false;
      ++g_arbitrary_dispose_calls;
      return 0;
    }
    case 2: {  // COPY
      void* source = read_synthetic_field<void*>(bytes, 16);
      auto** destination = read_synthetic_field<void**>(bytes, 24);
      int32_t value{};
      if (!destination || !synthetic_value(source, &value)) return 4;
      *destination = create_synthetic_handle(value);
      ++g_arbitrary_copy_calls;
      return 0;
    }
    case 3: {  // FLAT_SIZE
      void* source = read_synthetic_field<void*>(bytes, 16);
      auto* flat_size = read_synthetic_field<uint32_t*>(bytes, 24);
      int32_t value{};
      if (!flat_size || !synthetic_value(source, &value)) return 4;
      *flat_size = sizeof(value);
      return 0;
    }
    case 4: {  // FLATTEN
      void* source = read_synthetic_field<void*>(bytes, 16);
      const uint32_t flat_size = read_synthetic_field<uint32_t>(bytes, 24);
      auto* destination = read_synthetic_field<void*>(bytes, 32);
      int32_t value{};
      if (!destination || flat_size != sizeof(value) ||
          !synthetic_value(source, &value))
        return 4;
      std::memcpy(destination, &value, sizeof(value));
      return 0;
    }
    case 5: {  // UNFLATTEN
      const uint32_t flat_size = read_synthetic_field<uint32_t>(bytes, 16);
      const auto* source = read_synthetic_field<const void*>(bytes, 24);
      auto** destination = read_synthetic_field<void**>(bytes, 32);
      int32_t value{};
      if (!source || !destination || flat_size != sizeof(value)) return 4;
      std::memcpy(&value, source, sizeof(value));
      *destination = create_synthetic_handle(value);
      return 0;
    }
    case 6: {  // INTERP
      void* left_handle = read_synthetic_field<void*>(bytes, 16);
      void* right_handle = read_synthetic_field<void*>(bytes, 24);
      const double amount = read_synthetic_field<double>(bytes, 32);
      auto** destination = read_synthetic_field<void**>(bytes, 40);
      int32_t left{}, right{};
      if (!destination || !synthetic_value(left_handle, &left) ||
          !synthetic_value(right_handle, &right))
        return 4;
      auto* output_handle = synthetic_handle(*destination);
      if (!output_handle || !output_handle->live) return 4;
      output_handle->value = static_cast<int32_t>(
          std::lround(left + (right - left) * amount));
      return 0;
    }
    case 7: {  // COMPARE
      void* left_handle = read_synthetic_field<void*>(bytes, 16);
      void* right_handle = read_synthetic_field<void*>(bytes, 24);
      auto* comparison = read_synthetic_field<int32_t*>(bytes, 32);
      int32_t left{}, right{};
      if (!comparison || !synthetic_value(left_handle, &left) ||
          !synthetic_value(right_handle, &right))
        return 4;
      *comparison = left == right ? 0 : (left < right ? -1 : 1);
      return 0;
    }
    case 8: {  // PRINT_SIZE
      void* source = read_synthetic_field<void*>(bytes, 16);
      auto* print_size = read_synthetic_field<uint32_t*>(bytes, 24);
      int32_t value{};
      if (!print_size || !synthetic_value(source, &value)) return 4;
      *print_size = static_cast<uint32_t>(std::to_string(value).size() + 1);
      return 0;
    }
    case 9: {  // PRINT
      void* source = read_synthetic_field<void*>(bytes, 24);
      const uint32_t print_size = read_synthetic_field<uint32_t>(bytes, 32);
      auto* destination = read_synthetic_field<char*>(bytes, 40);
      int32_t value{};
      if (!destination || !synthetic_value(source, &value)) return 4;
      const std::string text = std::to_string(value);
      if (print_size <= text.size()) return 4;
      std::memcpy(destination, text.c_str(), text.size() + 1);
      return 0;
    }
    case 10: {  // SCAN
      const auto* source = read_synthetic_field<const char*>(bytes, 16);
      const uint32_t length = read_synthetic_field<uint32_t>(bytes, 24);
      auto** destination = read_synthetic_field<void**>(bytes, 32);
      int32_t value{};
      if (!destination || !parse_synthetic_integer(source, length, &value)) return 4;
      *destination = create_synthetic_handle(value);
      return 0;
    }
    default:
      return 4;
  }
}
int32_t invoke_synthetic_arbitrary(
    aexcompat::worker_runtime::parameter_execution::EffectEntry entry,
    int32_t command, void* input,
    void* output, void** params, void* world, void* extra,
    uint32_t* exception_code) {
  if (exception_code) *exception_code = 0;
  return entry(command, input, output, params, world, extra);
}
bool synthetic_handle_is_live(const void* value) {
  if (value == &g_arbitrary_source_token) return true;
  const auto* handle = synthetic_handle(value);
  return handle && handle->live;
}
std::size_t no_active_masks() { return 0; }
bool no_active_mask_id(std::size_t, int32_t*) { return false; }
void capture_clean_audit() {}
bool audit_stays_clean() { return true; }

aexcompat::worker_runtime::parameters::ParamRecord make_synthetic_arbitrary_record(
    const std::string& summary = {}) {
  using aexcompat::worker_runtime::parameters::ParamRecord;
  ParamRecord record{};
  record.index = 1;
  record.disk_id = 7001;
  record.type = 11;
  record.name = "Synthetic Arbitrary";
  record.arbitrary_summary = summary;
  const int32_t raw_type = 11;
  void* default_value = &g_arbitrary_source_token;
  void* refcon = &g_arbitrary_refcon_token;
  std::memcpy(record.raw.data() + 12, &raw_type, sizeof(raw_type));
  std::memcpy(record.raw.data() + 56, &kArbitraryId, sizeof(kArbitraryId));
  std::memcpy(record.raw.data() + 64, &default_value, sizeof(default_value));
  std::memcpy(record.raw.data() + 80, &refcon, sizeof(refcon));
  return record;
}

std::vector<unsigned char> synthetic_arbitrary_bytes(int32_t value) {
  std::vector<unsigned char> bytes(sizeof(value));
  std::memcpy(bytes.data(), &value, sizeof(value));
  return bytes;
}

struct ArbitraryObservation {
  bool rendered{};
  int32_t pre_error{};
  int32_t render_error{};
  std::array<int, 11> opcodes{};
  std::size_t live_handles{};
  int time_observations{};
  int time_mismatches{};
  int snapshot_observations{};
  int32_t snapshot_value{};
};

ArbitraryObservation run_smart_arbitrary_case(
    const aexcompat::worker_runtime::parameters::RequestedAssignments* requested,
    const std::vector<aexcompat::parameter_animation::ParameterTimeline>& timelines,
    int32_t current_time, int32_t time_step, int32_t total_time,
    uint32_t time_scale, std::initializer_list<int> time_checked_opcodes,
    const std::string& arbitrary_summary = {}, int fail_opcode = -1) {
  using namespace aexcompat::worker_runtime;
  reset_synthetic_arbitrary();
  parameter_execution::configure_hooks({&invoke_synthetic_arbitrary,
      &synthetic_handle_is_live, &no_active_masks, &no_active_mask_id});
  auto& runtime = parameters::state();
  runtime.records = {make_synthetic_arbitrary_record(arbitrary_summary)};
  runtime.timelines = timelines;
  expect_synthetic_time(current_time, time_step, total_time, time_scale,
                        time_checked_opcodes);
  g_arbitrary_fail_opcode = fail_opcode;

  parameter_execution::BufferIn input{};
  parameter_execution::BufferOut output{};
  constexpr uint32_t kNopRender = 1u << 18;
  std::memcpy(output.data() +
                  aexcompat::abi::x86_64_windows::OUT_OUT_FLAGS_OFFSET,
              &kNopRender, sizeof(kNopRender));
  constexpr int32_t kWidth = 16;
  constexpr int32_t kHeight = 12;
  std::vector<unsigned char> rgba(
      static_cast<std::size_t>(kWidth) * kHeight * 4, 0x40);
  const auto result = smart_execution::render_once(
      &copy_synthetic_arbitrary, input, output, "request", requested, &rgba,
      kWidth, kHeight, nullptr, current_time, time_step, total_time, time_scale,
      4, nullptr);

  ArbitraryObservation observation;
  observation.rendered = result.pre_error == 0 && result.render_error == 0 &&
      result.guards_intact;
  observation.pre_error = result.pre_error;
  observation.render_error = result.render_error;
  observation.opcodes = g_arbitrary_opcode_calls;
  observation.live_handles = live_synthetic_handle_count();
  observation.time_observations = g_arbitrary_time_observations;
  observation.time_mismatches = g_arbitrary_time_mismatches;
  observation.snapshot_observations = g_arbitrary_snapshot_observations;
  observation.snapshot_value = g_arbitrary_snapshot_value;
  runtime.records.clear();
  runtime.timelines.clear();
  g_arbitrary_fail_opcode = -1;
  g_arbitrary_time_checked.fill(false);
  return observation;
}

ArbitraryObservation run_classic_arbitrary_default_case(
    int32_t current_time, int32_t time_step, int32_t total_time,
    uint32_t time_scale) {
  using namespace aexcompat::worker_runtime;
  reset_synthetic_arbitrary();
  parameter_execution::configure_hooks({&invoke_synthetic_arbitrary,
      &synthetic_handle_is_live, &no_active_masks, &no_active_mask_id});
  auto& runtime = parameters::state();
  runtime.records = {make_synthetic_arbitrary_record("17")};
  runtime.timelines.clear();

  aexcompat::l2_detail::BufferIn input{};
  aexcompat::l2_detail::BufferOut output{};
  constexpr uint32_t kNopRender = 1u << 18;
  std::memcpy(output.data() +
                  aexcompat::abi::x86_64_windows::OUT_OUT_FLAGS_OFFSET,
              &kNopRender, sizeof(kNopRender));
  int32_t width{}, height{}, rowbytes{};
  std::string input_hash, output_hash;
  bool guards_intact{};
  const int32_t error = aexcompat::l2_detail::render_once(
      &copy_synthetic_arbitrary, input, output, "default", width, height,
      rowbytes, input_hash, output_hash, guards_intact, nullptr, nullptr, 0, 0,
      nullptr, current_time, time_step, total_time, time_scale);

  ArbitraryObservation observation;
  observation.rendered = error == 0 && guards_intact;
  observation.pre_error = error;
  observation.render_error = error;
  observation.opcodes = g_arbitrary_opcode_calls;
  observation.live_handles = live_synthetic_handle_count();
  observation.time_observations = g_arbitrary_time_observations;
  observation.time_mismatches = g_arbitrary_time_mismatches;
  observation.snapshot_observations = g_arbitrary_snapshot_observations;
  observation.snapshot_value = g_arbitrary_snapshot_value;
  runtime.records.clear();
  return observation;
}

bool verify_arbitrary_hot_path_contract() {
  using namespace aexcompat::worker_runtime;
  auto& runtime = parameters::state();
  const auto saved_records = runtime.records;
  const auto saved_timelines = runtime.timelines;
  parameter_execution::configure_hooks({&invoke_synthetic_arbitrary,
      &synthetic_handle_is_live, &no_active_masks, &no_active_mask_id});

  // The diagnostic helpers remain callable explicitly and still exercise the
  // complete non-null value contract. This is deliberately separate from the
  // shipping frame cases below: a source-text grep could not prove either
  // ownership or selector behavior.
  reset_synthetic_arbitrary(19);
  runtime.records = {make_synthetic_arbitrary_record()};
  runtime.timelines.clear();
  parameter_execution::Definitions conformance_definitions(2);
  parameter_execution::initialize_parameter_definitions(
      conformance_definitions, 16, 12);
  parameter_execution::BufferIn conformance_input{};
  parameter_execution::BufferOut conformance_output{};
  constexpr int32_t kConformanceTime = 10;
  constexpr int32_t kConformanceStep = 2;
  constexpr int32_t kConformanceTotal = 20;
  constexpr uint32_t kConformanceScale = 24;
  std::memcpy(conformance_input.data() +
                  aexcompat::abi::x86_64_windows::IN_CURRENT_TIME_OFFSET,
              &kConformanceTime, sizeof(kConformanceTime));
  std::memcpy(conformance_input.data() +
                  aexcompat::abi::x86_64_windows::IN_TIME_STEP_OFFSET,
              &kConformanceStep, sizeof(kConformanceStep));
  std::memcpy(conformance_input.data() +
                  aexcompat::abi::x86_64_windows::IN_TOTAL_TIME_OFFSET,
              &kConformanceTotal, sizeof(kConformanceTotal));
  std::memcpy(conformance_input.data() +
                  aexcompat::abi::x86_64_windows::IN_TIME_SCALE_OFFSET,
              &kConformanceScale, sizeof(kConformanceScale));
  expect_synthetic_time(kConformanceTime, kConformanceStep,
                        kConformanceTotal, kConformanceScale,
                        {0, 3, 4, 5, 6, 7, 8, 9, 10});
  parameter_execution::observe_arbitrary_defaults(
      &copy_synthetic_arbitrary, conformance_input, conformance_output);
  bool conformance_ok = parameter_execution::initialize_arbitrary_values(
      &copy_synthetic_arbitrary, conformance_input, conformance_output,
      conformance_definitions);
  conformance_ok = conformance_ok &&
      parameter_execution::interpolate_arbitrary_values(
          &copy_synthetic_arbitrary, conformance_input, conformance_output,
          conformance_definitions) &&
      parameter_execution::roundtrip_arbitrary_values(
          &copy_synthetic_arbitrary, conformance_input, conformance_output,
          conformance_definitions);
  parameter_execution::probe_arbitrary_scan(
      &copy_synthetic_arbitrary, conformance_input, conformance_output,
      conformance_definitions);
  void* conformance_value_handle{};
  std::memcpy(&conformance_value_handle,
              conformance_definitions[1].data() + 72,
              sizeof(conformance_value_handle));
  int32_t conformance_value{};
  conformance_ok = conformance_ok &&
      synthetic_value(conformance_value_handle, &conformance_value) &&
      conformance_value == 19 &&
      runtime.records[0].arbitrary_summary == "19" &&
      parameter_execution::dispose_arbitrary_values(
          &copy_synthetic_arbitrary, conformance_input, conformance_output,
          conformance_definitions) &&
      live_synthetic_handle_count() == 0 &&
      g_arbitrary_time_mismatches == 0 &&
      g_arbitrary_opcode_calls[0] == 1 &&
      g_arbitrary_opcode_calls[1] == 4 &&
      g_arbitrary_opcode_calls[2] == 1 &&
      g_arbitrary_opcode_calls[3] == 1 &&
      g_arbitrary_opcode_calls[4] == 2 &&
      g_arbitrary_opcode_calls[5] == 1 &&
      g_arbitrary_opcode_calls[6] == 1 &&
      g_arbitrary_opcode_calls[7] == 1 &&
      g_arbitrary_opcode_calls[8] == 1 &&
      g_arbitrary_opcode_calls[9] == 1 &&
      g_arbitrary_opcode_calls[10] == 1;

  const auto default_frame = run_smart_arbitrary_case(
      nullptr, {}, 7, 2, 24, 24, {}, "17");
  bool default_ok = default_frame.rendered &&
      default_frame.opcodes[1] == 1 && default_frame.opcodes[2] == 1 &&
      default_frame.live_handles == 0 &&
      default_frame.snapshot_observations == 1 &&
      default_frame.snapshot_value == 17;
  for (const int diagnostic : {0, 3, 4, 5, 6, 7, 10})
    default_ok = default_ok && default_frame.opcodes[diagnostic] == 0;

  const auto classic_default_frame = run_classic_arbitrary_default_case(
      7, 2, 24, 24);
  bool classic_default_ok = classic_default_frame.rendered &&
      classic_default_frame.opcodes[1] == 1 &&
      classic_default_frame.opcodes[2] == 1 &&
      classic_default_frame.live_handles == 0 &&
      classic_default_frame.snapshot_observations == 1 &&
      classic_default_frame.snapshot_value == 17;
  for (const int diagnostic : {0, 3, 4, 5, 6, 7, 10})
    classic_default_ok =
        classic_default_ok && classic_default_frame.opcodes[diagnostic] == 0;

  parameters::RequestedAssignment text_assignment{};
  text_assignment.id = L"1";
  text_assignment.index = 1;
  text_assignment.kind = parameters::RequestedKind::ArbitraryText;
  text_assignment.text = "27";
  const parameters::RequestedAssignments text_assignments{text_assignment};
  const auto text_frame = run_smart_arbitrary_case(
      &text_assignments, {}, 9, 3, 30, 30, {10});
  const bool text_ok = text_frame.rendered &&
      text_frame.opcodes[1] == 2 && text_frame.opcodes[2] == 1 &&
      text_frame.opcodes[10] == 1 && text_frame.time_observations == 1 &&
      text_frame.time_mismatches == 0 && text_frame.live_handles == 0 &&
      text_frame.snapshot_observations == 1 && text_frame.snapshot_value == 27;

  aexcompat::parameter_animation::ParameterTimeline timeline{};
  timeline.slot = 1;
  aexcompat::parameter_animation::AnimationKey left{};
  left.time = 0;
  left.scale = 24;
  left.kind = aexcompat::parameter_animation::AnimationValueKind::Arbitrary;
  left.arbitrary = synthetic_arbitrary_bytes(10);
  auto right = left;
  right.time = 24;
  right.arbitrary = synthetic_arbitrary_bytes(30);
  timeline.keys = {left, right};

  const auto midpoint_frame = run_smart_arbitrary_case(
      nullptr, {timeline}, 12, 1, 24, 24, {0, 5, 6});
  const bool midpoint_ok = midpoint_frame.rendered &&
      midpoint_frame.opcodes[0] == 1 && midpoint_frame.opcodes[1] == 4 &&
      midpoint_frame.opcodes[2] == 1 && midpoint_frame.opcodes[5] == 2 &&
      midpoint_frame.opcodes[6] == 1 && midpoint_frame.time_observations == 4 &&
      midpoint_frame.time_mismatches == 0 &&
      midpoint_frame.live_handles == 0 &&
      midpoint_frame.snapshot_observations == 1 &&
      midpoint_frame.snapshot_value == 20;

  const auto endpoint_frame = run_smart_arbitrary_case(
      nullptr, {timeline}, 0, 1, 24, 24, {5});
  const bool endpoint_ok = endpoint_frame.rendered &&
      endpoint_frame.opcodes[0] == 0 && endpoint_frame.opcodes[1] == 3 &&
      endpoint_frame.opcodes[2] == 1 && endpoint_frame.opcodes[5] == 2 &&
      endpoint_frame.opcodes[6] == 0 && endpoint_frame.time_observations == 2 &&
      endpoint_frame.time_mismatches == 0 && endpoint_frame.live_handles == 0 &&
      endpoint_frame.snapshot_observations == 1 &&
      endpoint_frame.snapshot_value == 10;

  const auto failed_frame = run_smart_arbitrary_case(
      nullptr, {timeline}, 12, 1, 24, 24, {0, 5, 6}, {}, 6);
  const bool failure_cleanup_ok = !failed_frame.rendered &&
      failed_frame.opcodes[0] == 1 && failed_frame.opcodes[1] == 4 &&
      failed_frame.opcodes[2] == 1 && failed_frame.opcodes[5] == 2 &&
      failed_frame.opcodes[6] == 1 && failed_frame.time_observations == 4 &&
      failed_frame.time_mismatches == 0 && failed_frame.live_handles == 0 &&
      failed_frame.snapshot_observations == 0;

  runtime.records = saved_records;
  runtime.timelines = saved_timelines;
  const bool passed = conformance_ok && default_ok && classic_default_ok &&
      text_ok && midpoint_ok && endpoint_ok && failure_cleanup_ok;
  if (!passed) {
    std::cerr << "arbitrary hot-path contract failed: conformance="
              << conformance_ok << " default=" << default_ok
              << " classic_default=" << classic_default_ok
              << " text=" << text_ok << " midpoint=" << midpoint_ok
              << " endpoint=" << endpoint_ok
              << " failure_cleanup=" << failure_cleanup_ok << "\n";
  }
  return passed;
}

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
    int32_t command, void*, void* output, void**, void* world, void*) {
  if (command == 18 && g_advertise_dynamic_wide_time) {
    constexpr uint32_t kWideTimeInput = 1u << 1;
    uint32_t out_flags{};
    std::memcpy(&out_flags, static_cast<std::byte*>(output) + 96,
                sizeof(out_flags));
    out_flags |= kWideTimeInput;
    std::memcpy(static_cast<std::byte*>(output) + 96, &out_flags,
                sizeof(out_flags));
    return 0;
  }
  if (command == aexcompat::abi::x86_64_windows::PF_CMD_RENDER) {
    void* pixels{};
    if (!world) return 4;
    std::memcpy(&pixels,
                static_cast<std::byte*>(world) +
                    aexcompat::abi::x86_64_windows::LAYER_DATA_OFFSET,
                sizeof(pixels));
    if (!pixels) return 4;
    // This time-propagation fixture must still satisfy the shipping output
    // contract. Flip one real output byte so success never depends on an
    // unrelated diagnostic hook touching the frame allocation.
    *static_cast<unsigned char*>(pixels) ^= 1u;
    if (g_advertise_dynamic_wide_time) {
      auto* context = active_context();
      if (!context ||
          !context->checkout_time_allowed(g_expected_frame_time + 1, 24))
        return 4;
      ++g_render_wide_time_observations;
    }
    return 0;
  }
  if (command != aexcompat::abi::x86_64_windows::PF_CMD_FRAME_SETUP) return 0;
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

int32_t __cdecl mutate_out_data_after_frame_setup(
    int32_t command, void* input, void* output, void**, void* world, void*) {
  using namespace aexcompat::abi::x86_64_windows;
  constexpr int32_t kOutputWidth = 260;
  constexpr int32_t kOutputHeight = 150;
  constexpr int32_t kOriginX = 2;
  constexpr int32_t kOriginY = 3;
  const auto write_i32 = [](void* bytes, std::size_t offset, int32_t value) {
    std::memcpy(static_cast<std::byte*>(bytes) + offset, &value, sizeof(value));
  };
  const auto read_i32 = [](const void* bytes, std::size_t offset) {
    int32_t value{};
    std::memcpy(&value, static_cast<const std::byte*>(bytes) + offset,
                sizeof(value));
    return value;
  };

  if (command == PF_CMD_FRAME_SETUP) {
    g_frame_setup_offered_width = read_i32(output, OUT_WIDTH_OFFSET);
    g_frame_setup_offered_height = read_i32(output, OUT_HEIGHT_OFFSET);
    write_i32(output, OUT_WIDTH_OFFSET, kOutputWidth);
    write_i32(output, OUT_HEIGHT_OFFSET, kOutputHeight);
    write_i32(output, OUT_ORIGIN_OFFSET, kOriginX);
    write_i32(output, OUT_ORIGIN_OFFSET + sizeof(int32_t), kOriginY);
    return 0;
  }
  if (command == PF_CMD_QUERY_DYNAMIC_FLAGS) {
    // QUERY_DYNAMIC_FLAGS legitimately reuses PF_OutData but owns only the
    // flags. Mutating geometry here models a plug-in that treats every other
    // field as scratch; the FRAME_SETUP answer must already be host-owned.
    write_i32(output, OUT_WIDTH_OFFSET, g_frame_setup_offered_width);
    write_i32(output, OUT_HEIGHT_OFFSET, g_frame_setup_offered_height);
    write_i32(output, OUT_ORIGIN_OFFSET, 0);
    write_i32(output, OUT_ORIGIN_OFFSET + sizeof(int32_t), 0);
    return 0;
  }
  if (command != PF_CMD_RENDER) return 0;
  if (read_i32(input, IN_OUTPUT_ORIGIN_X_OFFSET) != kOriginX ||
      read_i32(input, IN_OUTPUT_ORIGIN_Y_OFFSET) != kOriginY || !world ||
      read_i32(world, LAYER_WIDTH_OFFSET) != kOutputWidth ||
      read_i32(world, LAYER_HEIGHT_OFFSET) != kOutputHeight)
    return 4;
  void* pixels{};
  std::memcpy(&pixels, static_cast<std::byte*>(world) + LAYER_DATA_OFFSET,
              sizeof(pixels));
  if (!pixels) return 4;
  *static_cast<unsigned char*>(pixels) ^= 1u;
  ++g_frame_setup_geometry_render_observations;
  return 0;
}
}  // namespace

int main() {
  {
    namespace parser = aexcompat::worker_runtime::request_parser;
    const auto parse_layers = [](parser::Kind kind, const wchar_t* trailer) {
      std::vector<std::wstring> args{L"worker", kind == parser::Kind::Smart ?
          L"--smart-session-v1" : L"--render-session-v1", L"unused.aex",
          std::wstring(64, L'0'), L"none", L"48", L"32", L"1", L"300", L"30", trailer};
      std::vector<wchar_t*> argv;
      for (auto& arg : args) argv.push_back(arg.data());
      parser::Hooks hooks{};
      // The dispatcher requires auxiliary hooks even when no auxiliary flags
      // are present. Reject any unexpected option in this parser-only fixture.
      const auto reject_option = [](void*, const wchar_t*) { return false; };
      hooks.auxiliary.set_dump_worlds_dir = reject_option;
      hooks.auxiliary.enable_checksum_detail = [](void*) { return false; };
      hooks.auxiliary.load_aux_manifest = reject_option;
      hooks.auxiliary.parse_alpha_coverage = reject_option;
      hooks.auxiliary.load_parameter_animation = reject_option;
      hooks.auxiliary.parse_conformance_render_settings = reject_option;
      hooks.parse_parameters = [](const wchar_t*, void*) { return true; };
      return parser::parse(kind, static_cast<int>(argv.size()), argv.data(), hooks);
    };
    for (const auto kind : {parser::Kind::Classic, parser::Kind::Smart}) {
      const auto accepted = parse_layers(kind, L"session-layers:v2|0,48,32,1,30,123");
      if (accepted.error || accepted.invocation.layers.size() != 1 ||
          accepted.invocation.layers[0].slot != 0 ||
          !accepted.invocation.layers[0].timed) return 90;
      for (const auto* invalid : {
          L"session-layers:v2|0,48,32,123",
          L"session-layers:v2|0,48,32,123,1",
          L"session-layers:v2|0,48,32,1,0,123",
          L"session-layers:v2|0,48,32,1,30,0",
          L"session-layers:v2|0,48,32,1,30,123;0,48,32,2,60,124",
          L"session-layers:v2|-1,48,32,1,30,123",
          L"session-layers:v2|0,0,32,1,30,123"}) {
        if (parse_layers(kind, invalid).error != 3) return 91;
      }
    }
  }
  {
    Context context;
    context.configure_checkout_time(2, 30, true, false);
    ParameterDefinition current{}, past{}, actual{};
    current[64] = std::byte{23};
    past[64] = std::byte{71};
    context.set_definition(0, current);
    if (!context.add_timed_layer({0, 1, 30, past})) return 80;
    if (!context.copy_timed_layer(0, 2, 60, actual.data(), actual.size()) ||
        actual != past) return 81;
    if (!context.copy_timed_layer(0, 4, 60, actual.data(), actual.size()) ||
        actual != current) return 82;
    if (context.copy_timed_layer(0, 3, 30, actual.data(), actual.size())) return 83;
    if (context.copy_timed_layer(0, 1, 0, actual.data(), actual.size())) return 84;
    if (context.add_timed_layer({-1, 1, 30, past}) ||
        context.add_timed_layer({0, 1, 0, past})) return 85;
  }
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
        telemetry.last_progress_current != 10 ||
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
  const int32_t dynamic_time_result = aexcompat::l2_detail::render_once(
      &observe_frame_setup_checkout_time, frame_input, frame_output, "default",
      frame_width, frame_height, frame_rowbytes, frame_input_hash,
      frame_output_hash, frame_guards, nullptr, nullptr, 0, 0, nullptr,
      g_expected_frame_time, 1, 100, 24);
  if (dynamic_time_result != 0 || g_frame_setup_time_observations != 4 ||
      g_render_wide_time_observations != 1) {
    std::cerr << "dynamic time fixture failed: result=" << dynamic_time_result
              << " frame_setup=" << g_frame_setup_time_observations
              << " frame_setdown=" << g_render_wide_time_observations << "\n";
    return 33;
  }
  aexcompat::worker_runtime::parameters::state().ui.dynamic_flags_advertised = false;

  // `begin_lifecycle` dispatches QUERY_DYNAMIC_FLAGS after FRAME_SETUP and
  // before production output preparation. Both selectors receive the same
  // PF_OutData buffer, so output geometry must be snapshotted at the first
  // boundary rather than read from that shared buffer later (#999).
  constexpr uint32_t kExpandBuffer = 1u << 9;
  aexcompat::worker_runtime::parameters::state().ui.dynamic_flags_advertised = true;
  g_frame_setup_geometry_render_observations = 0;
  for (const bool manage_sequence : {true, false}) {
    frame_input = {};
    frame_output = {};
    std::memcpy(frame_output.data() +
                    aexcompat::abi::x86_64_windows::OUT_OUT_FLAGS_OFFSET,
                &kExpandBuffer, sizeof(kExpandBuffer));
    aexcompat::render::ClassicFrameOutput geometry{};
    if (aexcompat::l2_detail::render_once(
            &mutate_out_data_after_frame_setup, frame_input, frame_output,
            "default", frame_width, frame_height, frame_rowbytes,
            frame_input_hash, frame_output_hash, frame_guards, nullptr, nullptr,
            0, 0, nullptr, 0, 1, 1, 1, 4, manage_sequence, nullptr,
            &geometry) != 0 ||
        frame_width != 260 || frame_height != 150 ||
        geometry.input_origin_x != 2 || geometry.input_origin_y != 3)
      return manage_sequence ? 40 : 41;
  }
  if (g_frame_setup_geometry_render_observations != 2) return 42;

  // NOP_RENDER has no prepare_output call, but it must consume the same
  // snapshot when deciding whether FRAME_SETUP requested the resize that #1002
  // currently requires it to refuse explicitly. A later selector must not turn
  // that refusal into a silent old-extent passthrough.
  frame_input = {};
  frame_output = {};
  constexpr uint32_t kNopRender = 1u << 18;
  constexpr uint32_t kNopExpand = kExpandBuffer | kNopRender;
  std::memcpy(frame_output.data() +
                  aexcompat::abi::x86_64_windows::OUT_OUT_FLAGS_OFFSET,
              &kNopExpand, sizeof(kNopExpand));
  if (aexcompat::l2_detail::render_once(
          &mutate_out_data_after_frame_setup, frame_input, frame_output, "default",
          frame_width, frame_height, frame_rowbytes, frame_input_hash,
          frame_output_hash, frame_guards, nullptr, nullptr, 0, 0, nullptr, 0,
          1, 1, 1, 4, false) != 4 ||
      frame_width != g_frame_setup_offered_width ||
      frame_height != g_frame_setup_offered_height ||
      g_frame_setup_geometry_render_observations != 2)
    return 43;
  aexcompat::worker_runtime::parameters::state().ui.dynamic_flags_advertised = false;

  using namespace aexcompat::worker_runtime;

  // An arbitrary parameter may have no default handle.  It must remain null
  // without dispatching COPY, while a non-null default is still copied into a
  // distinct caller-owned handle.
  auto& parameter_state = parameters::state();
  reset_synthetic_arbitrary();
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
  int32_t copied_arbitrary_value{};
  if (null_value != nullptr || copied_value == source_value ||
      !synthetic_value(copied_value, &copied_arbitrary_value) ||
      copied_arbitrary_value != g_arbitrary_source_token ||
      live_synthetic_handle_count() != 1) return 5;
  if (!parameter_execution::dispose_arbitrary_values(
      &copy_synthetic_arbitrary, arbitrary_input, arbitrary_output,
          arbitrary_definitions) ||
      g_arbitrary_dispose_calls != 1 || live_synthetic_handle_count() != 0)
    return 6;
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
  const bool arbitrary_hot_path_passed = verify_arbitrary_hot_path_contract();
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
  if (concurrent_context_passed && null_arbitrary_passed &&
      arbitrary_hot_path_passed)
    std::cout << "{\"classic_runtime_selftest\":\"passed\"}\n";
  return concurrent_context_passed && null_arbitrary_passed &&
      arbitrary_hot_path_passed ? 0 : 8;
}
