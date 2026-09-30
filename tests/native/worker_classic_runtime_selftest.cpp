#include "worker_classic_runtime.hpp"
#include "worker_classic_render_entry.hpp"
#include "worker_active_plugin_context.hpp"
#include "generated/aex_abi_contract.hpp"
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
int g_frame_setup_geometry_render_observations{};
int32_t g_frame_setup_offered_width{};
int32_t g_frame_setup_offered_height{};
int32_t g_nop_width{};
int32_t g_nop_height{};
int32_t g_nop_origin_x{};
int32_t g_nop_origin_y{};
int g_nop_render_calls{};

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

bool fill_synthetic_output(void* world) {
  using namespace aexcompat::abi::x86_64_windows;
  if (!world) return false;
  unsigned char* pixels{};
  int32_t width{}, height{}, rowbytes{};
  std::memcpy(&pixels, static_cast<std::byte*>(world) + LAYER_DATA_OFFSET,
              sizeof(pixels));
  std::memcpy(&width, static_cast<std::byte*>(world) + LAYER_WIDTH_OFFSET,
              sizeof(width));
  std::memcpy(&height, static_cast<std::byte*>(world) + LAYER_HEIGHT_OFFSET,
              sizeof(height));
  std::memcpy(&rowbytes, static_cast<std::byte*>(world) + LAYER_ROWBYTES_OFFSET,
              sizeof(rowbytes));
  if (!pixels || width <= 0 || height <= 0 || rowbytes < width * 4) return false;
  // This fixture observes lifecycle/context behavior, but a successful
  // selector also has to honor the shipping output-write contract (#1592).
  for (int32_t y = 0; y < height; ++y)
    std::memset(pixels + static_cast<std::size_t>(y) * rowbytes, 0x11,
                static_cast<std::size_t>(width) * 4);
  return true;
}

int32_t __cdecl observe_frame_setup_checkout_time(
    int32_t command, void*, void* output, void**, void* world, void*) {
  if (command == 18 && g_advertise_dynamic_wide_time) {
    constexpr uint32_t kWideTimeInput = 1u << 1;
    std::memcpy(static_cast<std::byte*>(output) + 96, &kWideTimeInput,
                sizeof(kWideTimeInput));
    return 0;
  }
  if (command == 11) {
    if (g_advertise_dynamic_wide_time) {
      auto* context = active_context();
      if (!context || !context->checkout_time_allowed(g_expected_frame_time + 1, 24))
        return 4;
      ++g_render_wide_time_observations;
    }
    return fill_synthetic_output(world) ? 0 : 4;
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
  ++g_frame_setup_geometry_render_observations;
  return fill_synthetic_output(world) ? 0 : 4;
}

int32_t __cdecl nop_render_passthrough(
    int32_t command, void*, void*, void**, void*, void*) {
  // The host owns the pixels on this path. The effect must never see RENDER.
  if (command == 11) { ++g_nop_render_calls; return 4; }
  return 0;
}

int32_t __cdecl request_nop_geometry(
    int32_t command, void*, void* output, void**, void*, void*) {
  using namespace aexcompat::abi::x86_64_windows;
  if (command == 11) { ++g_nop_render_calls; return 4; }
  if (command == PF_CMD_FRAME_SETUP) {
    std::memcpy(static_cast<std::byte*>(output) + OUT_WIDTH_OFFSET,
                &g_nop_width, sizeof(g_nop_width));
    std::memcpy(static_cast<std::byte*>(output) + OUT_HEIGHT_OFFSET,
                &g_nop_height, sizeof(g_nop_height));
    std::memcpy(static_cast<std::byte*>(output) + OUT_ORIGIN_OFFSET,
                &g_nop_origin_x, sizeof(g_nop_origin_x));
    std::memcpy(static_cast<std::byte*>(output) + OUT_ORIGIN_OFFSET + sizeof(int32_t),
                &g_nop_origin_y, sizeof(g_nop_origin_y));
  }
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
  if (aexcompat::l2_detail::render_once(
          &observe_frame_setup_checkout_time, frame_input, frame_output, "default",
          frame_width, frame_height, frame_rowbytes, frame_input_hash,
          frame_output_hash, frame_guards, nullptr, nullptr, 0, 0, nullptr,
          g_expected_frame_time, 1, 100, 24) != 0 ||
      g_frame_setup_time_observations != 4 ||
      g_render_wide_time_observations != 1)
    return 33;
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

  // NOP_RENDER still honors a FRAME_SETUP resize and places the source at the
  // requested origin, leaving transparent pixels around it. Compare to an
  // unresized host-owned passthrough so the generated input pattern is checked
  // without depending on its implementation.
  frame_input = {};
  frame_output = {};
  constexpr uint32_t kNopRender = 1u << 18;
  constexpr uint32_t kNopExpand = kExpandBuffer | kNopRender;
  std::memcpy(frame_output.data() +
                  aexcompat::abi::x86_64_windows::OUT_OUT_FLAGS_OFFSET,
              &kNopRender, sizeof(kNopRender));
  std::vector<unsigned char> source_pixels;
  if (aexcompat::l2_detail::render_once(
          &nop_render_passthrough, frame_input, frame_output, "default",
          frame_width, frame_height, frame_rowbytes, frame_input_hash,
          frame_output_hash, frame_guards, nullptr, nullptr, 0, 0, nullptr, 0,
          1, 1, 1, 4, false, &source_pixels) != 0 || !frame_guards ||
      source_pixels.size() != static_cast<std::size_t>(frame_width) * frame_height * 4)
    return 43;
  if (g_nop_render_calls != 0) return 49;
  const int32_t source_width = frame_width;
  const int32_t source_height = frame_height;
  std::memcpy(frame_output.data() +
                  aexcompat::abi::x86_64_windows::OUT_OUT_FLAGS_OFFSET,
              &kNopExpand, sizeof(kNopExpand));
  std::vector<unsigned char> resized_pixels;
  aexcompat::render::ClassicFrameOutput nop_geometry{};
  const int32_t nop_resize_error = aexcompat::l2_detail::render_once(
          &mutate_out_data_after_frame_setup, frame_input, frame_output, "default",
          frame_width, frame_height, frame_rowbytes, frame_input_hash,
          frame_output_hash, frame_guards, nullptr, nullptr, 0, 0, nullptr, 0,
          1, 1, 1, 4, false, &resized_pixels, &nop_geometry);
  if (nop_resize_error != 0) {
    std::cerr << "nop_resize_error=" << nop_resize_error << "\n";
    return 44;
  }
  if (frame_width != 260 || frame_height != 150 ||
      frame_rowbytes != frame_width * 4) {
    std::cerr << "nop_resize_dims=" << frame_width << "x" << frame_height
              << " source=" << source_width << "x" << source_height
              << " rowbytes=" << frame_rowbytes << "\n";
    return 45;
  }
  if (!frame_guards || nop_geometry.input_origin_x != 2 ||
      nop_geometry.input_origin_y != 3) return 46;
  if (resized_pixels.size() != static_cast<std::size_t>(frame_width) * frame_height * 4 ||
      g_frame_setup_geometry_render_observations != 2) return 47;
  for (int32_t y = 0; y < frame_height; ++y)
    for (int32_t x = 0; x < frame_width; ++x)
      for (int32_t channel = 0; channel < 4; ++channel) {
        const std::size_t offset = (static_cast<std::size_t>(y) * frame_width + x) * 4 + channel;
        const unsigned char expected = x >= 2 && x < source_width + 2 &&
                y >= 3 && y < source_height + 3
            ? source_pixels[(static_cast<std::size_t>(y - 3) * source_width + (x - 2)) * 4 + channel]
            : 0;
        if (resized_pixels[offset] != expected) return 48;
      }
  if (g_nop_render_calls != 0) return 49;

  // A refused extent and a disjoint origin remain frame-local. A subsequent
  // crop must still use the original source, not either refused geometry.
  auto run_nop_geometry = [&](uint32_t flags, int32_t requested_width,
                              int32_t requested_height, int32_t origin_x,
                              int32_t origin_y, std::vector<unsigned char>& pixels,
                              aexcompat::render::ClassicFrameOutput& geometry) {
    frame_input = {};
    frame_output = {};
    std::memcpy(frame_output.data() +
                    aexcompat::abi::x86_64_windows::OUT_OUT_FLAGS_OFFSET,
                &flags, sizeof(flags));
    g_nop_width = requested_width;
    g_nop_height = requested_height;
    g_nop_origin_x = origin_x;
    g_nop_origin_y = origin_y;
    return aexcompat::l2_detail::render_once(
        &request_nop_geometry, frame_input, frame_output, "default", frame_width,
        frame_height, frame_rowbytes, frame_input_hash, frame_output_hash,
        frame_guards, nullptr, nullptr, 0, 0, nullptr, 0, 1, 1, 1, 4,
        false, &pixels, &geometry);
  };
  constexpr uint32_t kNopShrink = kNopRender | (1u << 12);
  std::vector<unsigned char> crop_pixels;
  aexcompat::render::ClassicFrameOutput crop_geometry{};
  if (run_nop_geometry(kNopRender, 260, 150, 2, 3, crop_pixels,
                       crop_geometry) != 4 || crop_geometry.validation_failed ||
      g_nop_render_calls != 0) return 50;
  crop_geometry = {};
  if (run_nop_geometry(kNopShrink, 8, 6, 1000000, -2, crop_pixels,
                       crop_geometry) != 4 || crop_geometry.validation_failed ||
      g_nop_render_calls != 0) return 51;
  crop_geometry = {};
  if (run_nop_geometry(kNopShrink, 8, 6, -3, -2, crop_pixels,
                       crop_geometry) != 0 || !frame_guards ||
      frame_width != 8 || frame_height != 6 ||
      crop_geometry.input_origin_x != -3 || crop_geometry.input_origin_y != -2 ||
      crop_pixels.size() != 8u * 6u * 4u || g_nop_render_calls != 0) return 52;
  for (int32_t y = 0; y < 6; ++y)
    for (int32_t x = 0; x < 8; ++x)
      for (int32_t channel = 0; channel < 4; ++channel)
        if (crop_pixels[(static_cast<std::size_t>(y) * 8 + x) * 4 + channel] !=
            source_pixels[(static_cast<std::size_t>(y + 2) * source_width + (x + 3)) * 4 + channel])
          return 53;
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
