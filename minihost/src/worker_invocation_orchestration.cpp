#include "worker_invocation_orchestration.hpp"

#include "worker_classic_runtime.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_mask_selftests.hpp"

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <cwchar>
#include <filesystem>
#include <iostream>
#include <string>
#include <thread>
#include <vector>

namespace aexcompat::worker_runtime::invocation {
namespace {
void apply_common(const request_parser::WorkerInvocation& source,
                  InvocationState& target) {
  const auto& mode = source.mode;
  target.external_pixel_bytes = mode.external_pixel_bytes;
  target.external_rgba = source.rgba;
  target.external_layers = source.layers;
  target.external_output = source.output;
  target.external_width = source.width;
  target.external_height = source.height;
  target.external_current_time = source.current_time;
  target.external_time_step = source.time_step;
  target.external_total_time = source.total_time;
  target.external_time_scale = source.time_scale;
}
void apply_click_draw(const request_parser::WorkerInvocation& source,
                      const ApplyHooks& hooks) {
  if (source.mode.image_click_context && hooks.set_click)
    hooks.set_click(source.click_x, source.click_y, source.picker_color);
  if (source.mode.image_draw_context && hooks.enable_draw) hooks.enable_draw();
}
}  // namespace

int parse_l2_modes(int argc, wchar_t** argv, InvocationState& target,
                   const L2ModeHooks& hooks) {
  target.user_changed_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--user-changed";
  target.aegp_update_menu_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-update-menu";
  target.aegp_idle_mode = argc == 4 && std::wstring(argv[1]) == L"--aegp-idle";
  target.aegp_command_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-command-roundtrip";
  target.aegp_active_idle_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-active-idle-roundtrip";
  target.aegp_keyframe_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-keyframe-roundtrip";
  target.aegp_seek_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-seek-roundtrip";
  target.aegp_trim_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-trim-roundtrip";
  target.aegp_switch_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-switch-roundtrip";
  target.aegp_comp_idle_roundtrip_mode = argc == 4 &&
      (std::wstring(argv[1]) == L"--aegp-comp-idle-roundtrip" ||
       target.aegp_keyframe_roundtrip_mode || target.aegp_seek_roundtrip_mode ||
       target.aegp_trim_roundtrip_mode || target.aegp_switch_roundtrip_mode);
  target.aegp_init_mode = (argc == 4 && std::wstring(argv[1]) == L"--aegp-init") ||
      target.aegp_update_menu_mode || target.aegp_idle_mode ||
      target.aegp_command_roundtrip_mode || target.aegp_active_idle_roundtrip_mode ||
      target.aegp_comp_idle_roundtrip_mode;
  target.params_only_mode = (argc == 4 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-params-only";
  target.runtime_module_authorization_mode = target.params_only_mode && argc == 6 &&
      std::wstring(argv[4]) == L"--runtime-module-authorization-v1";
  target.external_dependencies_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--l2-external-dependencies";
  target.do_dialog_mode = argc == 4 && std::wstring(argv[1]) == L"--l2-do-dialog";
  target.auto_dialog_mode = argc == 4 && std::wstring(argv[1]) == L"--l2-auto-dialog";
  target.adjust_cursor_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-adjust-cursor";
  target.draw_event_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-draw-event";
  target.click_event_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-click-event";
  target.drag_event_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-drag-event";
  target.ui_lifecycle_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-ui-lifecycle";
  target.ui_idle_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-ui-idle";
  target.ui_keydown_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-ui-keydown";
  target.ui_mouse_exited_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-ui-mouse-exited";
  target.ui_event_assignment_mode =
      ((target.adjust_cursor_mode || target.draw_event_mode || target.ui_lifecycle_mode ||
        target.ui_idle_mode || target.ui_mouse_exited_mode) && argc == 5) ||
      ((target.click_event_mode || target.drag_event_mode || target.ui_keydown_mode) && argc == 6);
  if (target.ui_event_assignment_mode &&
      (!hooks.parse_parameter_payload ||
       !hooks.parse_parameter_payload(argv[argc - 1], target.ui_event_assignments))) return 3;
  if (target.click_event_mode) {
    float red{}, green{}, blue{}, alpha{};
    if (swscanf_s(argv[4], L"%d,%d,%f,%f,%f,%f", &target.click_x, &target.click_y,
                  &red, &green, &blue, &alpha) != 6 ||
        target.click_x < 0 || target.click_x > 8192 || target.click_y < 0 || target.click_y > 8192 ||
        !std::isfinite(red) || !std::isfinite(green) || !std::isfinite(blue) || !std::isfinite(alpha) ||
        red < 0 || red > 1 || green < 0 || green > 1 || blue < 0 || blue > 1 || alpha < 0 || alpha > 1) return 3;
    target.picker_color = {red, green, blue, alpha};
  }
  if (target.drag_event_mode &&
      (swscanf_s(argv[4], L"%d,%d,%d,%d,%d", &target.click_x, &target.click_y,
                 &target.drag_end_x, &target.drag_end_y, &target.drag_steps) != 5 ||
       target.click_x < 0 || target.click_x > 8192 || target.click_y < 0 || target.click_y > 8192 ||
       target.drag_end_x < 0 || target.drag_end_x > 8192 || target.drag_end_y < 0 || target.drag_end_y > 8192 ||
       target.drag_steps < 1 || target.drag_steps > 32)) return 3;
  if (target.ui_keydown_mode &&
      (swscanf_s(argv[4], L"%d,%d,%u,%u", &target.click_x, &target.click_y,
                 &target.keydown_code, &target.keydown_modifiers) != 4 ||
       target.click_x < 0 || target.click_x > 8192 || target.click_y < 0 || target.click_y > 8192 ||
       (target.keydown_code & 0x3fff0000u) != 0 || target.keydown_modifiers > 0xffffu)) return 3;
  target.skip_about_mode = (argc == 4 && std::wstring(argv[1]) == L"--l2-no-about") ||
      target.params_only_mode || target.external_dependencies_mode || target.do_dialog_mode ||
      target.auto_dialog_mode || target.adjust_cursor_mode || target.draw_event_mode ||
      target.click_event_mode || target.drag_event_mode || target.ui_lifecycle_mode ||
      target.ui_idle_mode || target.ui_keydown_mode || target.ui_mouse_exited_mode;
  if (!target.user_changed_mode && !target.aegp_init_mode && !target.skip_about_mode &&
      (argc != 4 || std::wstring(argv[1]) != L"--l2")) return 2;
  if (target.params_only_mode && argc == 6 && !target.runtime_module_authorization_mode) return 2;
  if (target.user_changed_mode) {
    try { target.user_changed_param_slot = std::stoi(argv[4]); } catch (...) { return 3; }
    if (target.user_changed_param_slot <= 0 ||
        (hooks.max_params != 0 &&
         target.user_changed_param_slot > static_cast<int32_t>(hooks.max_params))) return 3;
    if (argc == 6 && (!hooks.parse_parameter_payload ||
                      !hooks.parse_parameter_payload(argv[5], target.user_changed_parameters))) return 3;
    target.user_changed_param_requested = true;
  }
  return 0;
}

void apply_render(const request_parser::WorkerInvocation& source,
                  InvocationState& target, const ApplyHooks& hooks) {
  apply_common(source, target);
  const auto& mode = source.mode;
  target.audio_mode = mode.audio_mode;
  target.image_audio_mode = mode.image_audio_mode;
  target.transport_argc = mode.transport_argc;
  target.image_click_context = mode.image_click_context;
  target.image_draw_context = mode.image_draw_context;
  target.image_click_argc = mode.image_click_argc;
  target.image_render_environment = mode.image_render_environment;
  target.image_environment_argc = mode.image_environment_argc;
  target.image_spatial_context = mode.image_spatial_context;
  target.image_trailer_argc = mode.image_trailer_argc;
  target.image_mask_context = mode.image_mask_context;
  target.image_argc = mode.image_argc;
  target.layered_image_mode = mode.layered_image_mode;
  target.image_mode = mode.image_mode;
  target.render_session_mode = mode.render_session_mode;
  target.audio_session_mode = mode.audio_session_mode;
  target.audio_session_max_samples = source.audio_session_max_samples;
  target.audio_session_channels = source.audio_session_channels;
  target.request_mode = mode.request_mode;
  target.external_audio = source.audio;
  target.external_audio_output = source.audio_output;
  target.external_audio_samples = source.audio_samples;
  target.external_audio_rate = source.audio_rate;
  apply_click_draw(source, hooks);
  if (mode.image_audio_mode && hooks.set_audio_source)
    hooks.set_audio_source(&target.external_audio, target.external_audio_samples);
}

void apply_smart(const request_parser::WorkerInvocation& source,
                 InvocationState& target, const ApplyHooks& hooks) {
  apply_common(source, target);
  const auto& mode = source.mode;
  target.smart_force_cpu = mode.force_cpu;
  target.smart_opencl = mode.opencl;
  target.smart_directx = mode.directx;
  target.smart_image_click_context = mode.image_click_context;
  target.smart_image_draw_context = mode.image_draw_context;
  target.smart_image_click_argc = mode.image_click_argc;
  target.smart_image_render_environment = mode.image_render_environment;
  target.smart_image_environment_argc = mode.image_environment_argc;
  target.smart_image_spatial_context = mode.image_spatial_context;
  target.smart_image_trailer_argc = mode.image_trailer_argc;
  target.smart_image_mask_context = mode.image_mask_context;
  target.smart_image_argc = mode.image_argc;
  target.smart_layered_image_mode = mode.layered_image_mode;
  target.smart_image_mode = mode.image_mode;
  target.render_session_mode = mode.render_session_mode;
  target.mask_request_mode = mode.mask_request_mode;
  target.mask_scene_request_mode = mode.mask_scene_request_mode;
  target.mask_context_request_mode = mode.mask_context_request_mode;
  target.mask_count_error_mode = mode.mask_count_error_mode;
  target.mask_count_crash_mode = mode.mask_count_crash_mode;
  target.mask_double_dispose_mode = mode.mask_double_dispose_mode;
  target.stream_live_value_dispose_mode = mode.stream_live_value_dispose_mode;
  target.stream_metadata_ownership_mode = mode.stream_metadata_ownership_mode;
  target.keyframe_ownership_mode = mode.keyframe_ownership_mode;
  target.dynamic_stream_tree_mode = mode.dynamic_stream_tree_mode;
  target.aegp_memory_strings_mode = mode.aegp_memory_strings_mode;
  target.suite_release_without_acquire_mode = mode.suite_release_without_acquire_mode;
  target.handle_resize_while_locked_mode = mode.handle_resize_while_locked_mode;
  target.world_double_dispose_mode = mode.world_double_dispose_mode;
  target.world_allocation_limit_mode = mode.world_allocation_limit_mode;
  target.pixel_format_registry_mode = mode.pixel_format_registry_mode;
  target.outline_mutation_mode = mode.outline_mutation_mode;
  target.mask_attribute_mode = mode.mask_attribute_mode;
  target.request_mode = mode.request_mode;
  if (hooks.set_mask_mode) hooks.set_mask_mode(mode.mask_model_enabled);
  if (hooks.set_mask_fault)
    hooks.set_mask_fault(mode.mask_count_error_mode, mode.mask_count_crash_mode);
  apply_click_draw(source, hooks);
}
}  // namespace aexcompat::worker_runtime::invocation

// Cross-TU declarations into worker_main's private renderers and state. The
// constants mirror l2_main's frozen protocol offsets and selector codes; every
// definition stays in l2_main.
namespace aexcompat::l2_detail {

using EffectEntry = aexcompat::worker_runtime::parameter_execution::EffectEntry;
using RequestedAssignments = aexcompat::worker_runtime::parameters::RequestedAssignments;
using ExternalLayerInput = aexcompat::worker_runtime::request_parser::LayerInput;
using SmartResult = aexcompat::worker_runtime::smart_execution::Result;

constexpr std::size_t kInSize = 408;
constexpr std::size_t kOutSize = 408;
constexpr std::size_t kInSequenceData = 320;
constexpr std::size_t kOutSequenceData = 56;
constexpr int32_t kSequenceSetup = 5;
constexpr int32_t kSequenceResetup = 6;
constexpr int32_t kSequenceFlatten = 7;
constexpr int32_t kSequenceSetdown = 8;
constexpr int32_t kGetFlattenedSequenceData = 28;

bool configure_mask_scene(const std::string& scene_id);
int32_t invoke_sequence_selector(EffectEntry entry, int32_t selector, void* input,
                                 void* output, uint32_t* exception_code = nullptr);
int32_t render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes, std::string& input_hash, std::string& output_hash,
                    bool& guards_intact, const RequestedAssignments* requested = nullptr,
                    const std::vector<unsigned char>* external_rgba = nullptr,
                    const std::filesystem::path* external_output = nullptr,
                    int32_t external_width = 0, int32_t external_height = 0,
                    const std::vector<ExternalLayerInput>* external_layers = nullptr,
                    int32_t external_current_time = 0, int32_t external_time_step = 1,
                    int32_t external_total_time = 1, uint32_t external_time_scale = 1,
                    int32_t external_pixel_bytes = 4, bool manage_sequence = true,
                    std::vector<unsigned char>* captured_argb = nullptr,
                    bool* output_validation_failed = nullptr);
SmartResult smart_render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                              std::array<std::byte, kOutSize>& output,
                              const std::string& case_id,
                              const RequestedAssignments* requested = nullptr,
                              const std::vector<unsigned char>* external_rgba = nullptr,
                              const std::filesystem::path* external_output = nullptr,
                              int32_t external_width = 0, int32_t external_height = 0,
                              const std::vector<ExternalLayerInput>* external_layers = nullptr,
                              int32_t external_current_time = 0, int32_t external_time_step = 1,
                              int32_t external_total_time = 1,
                              uint32_t external_time_scale = 1,
                              int32_t external_pixel_bytes = 4,
                              aexcompat::worker_runtime::smart_execution::SessionFrame*
                                  session = nullptr);
RenderSessionOutcome run_render_session(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, const RequestedAssignments* requested,
    int32_t max_width, int32_t max_height, int32_t time_step, int32_t total_time,
    uint32_t time_scale, int32_t pixel_bytes,
    const std::vector<ExternalLayerInput>* external_layers);
SmartRenderSessionOutcome run_smart_render_session(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, const RequestedAssignments* requested,
    const std::string& case_id, int32_t max_width, int32_t max_height,
    int32_t time_step, int32_t total_time, uint32_t time_scale,
    int32_t pixel_bytes, const std::vector<ExternalLayerInput>* external_layers);

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

}  // namespace aexcompat::l2_detail

namespace aexcompat::worker_runtime::invocation {

ClassicFinalDispatchResult run_classic_final_dispatch(const FinalDispatchRequest& request) {
  using namespace aexcompat::l2_detail;
  using namespace aexcompat::worker_runtime::handles;
  ClassicFinalDispatchResult result;
  const EffectEntry entry = request.entry;
  auto& input = *request.input;
  auto& output = *request.output;
  const InvocationState& invocation = *request.invocation;
  wchar_t** argv = request.argv;
  const int32_t params_error = request.params_error;
  const bool image_render_supported = request.image_render_supported;
  const bool depth_supported = request.depth_supported;
  std::string& case_id = result.case_id;
  std::string& input_hash = result.input_hash;
  std::string& output_hash = result.output_hash;
  bool& guards_intact = result.guards_intact;
  int32_t& render_width = result.render_width;
  int32_t& render_height = result.render_height;
  int32_t& render_rowbytes = result.render_rowbytes;
  auto& thread_errors = result.thread_errors;
  auto& thread_hashes = result.thread_hashes;
  auto& thread_guards = result.thread_guards;
  bool& concurrent_render = result.concurrent_render;
  bool& persistent_sequence = result.persistent_sequence;
  bool& session_protocol_violation = result.session_protocol_violation;
  bool& session_invariant_failure = result.session_invariant_failure;
  bool& flattened_sequence = result.flattened_sequence;
  bool& copied_flattened_sequence = result.copied_flattened_sequence;
  int32_t& persistent_sequence_setup_error = result.persistent_sequence_setup_error;
  int32_t& persistent_sequence_setdown_error = result.persistent_sequence_setdown_error;
  auto& persistent_frame_errors = result.persistent_frame_errors;
  auto& persistent_frame_hashes = result.persistent_frame_hashes;
  int32_t& sequence_flatten_error = result.sequence_flatten_error;
  int32_t& sequence_resetup_error = result.sequence_resetup_error;
  bool& flattened_handle_replaced = result.flattened_handle_replaced;
  bool& resetup_handle_replaced = result.resetup_handle_replaced;
  bool& flattened_handle_host_disposed = result.flattened_handle_host_disposed;
  int32_t& get_flattened_sequence_data_error = result.get_flattened_sequence_data_error;
  bool& original_sequence_preserved = result.original_sequence_preserved;
  int32_t& render_error = result.render_error;
  aexcompat::worker_runtime::classic::reset_selector_diagnostic();
  case_id = invocation.request_mode ? "request" : "";
  if (!invocation.request_mode) {
    for (const wchar_t* p = argv[4]; *p; ++p) {
      if (*p > 0x7f) { result.case_id_rejected = true; return result; }
      case_id.push_back(static_cast<char>(*p));
    }
  }
  concurrent_render = case_id == "threaded_default";
  persistent_sequence = case_id == "persistent_sequence";
  flattened_sequence = case_id == "flattened_sequence";
  copied_flattened_sequence = case_id == "copied_flattened_sequence";
  std::cerr << "stage:render_begin\n" << std::flush;
  render_error = !image_render_supported ? -7 : (depth_supported ? -1 : -6);
  if (params_error == 0 && image_render_supported && depth_supported && copied_flattened_sequence) {
    aexcompat::mask_runtime::set_model_enabled(true);
    configure_mask_scene("rectangle");
    std::cerr << "stage:sequence_setup_begin\n" << std::flush;
    persistent_sequence_setup_error = invoke_sequence_selector(
        entry, kSequenceSetup, input.data(), output.data());
    std::cerr << "stage:sequence_setup_end error=" << persistent_sequence_setup_error
              << "\n" << std::flush;
    void* original_handle = read<void*>(output, kOutSequenceData);
    write<void*>(input, kInSequenceData, original_handle);
    std::cerr << "stage:get_flattened_sequence_data_begin\n" << std::flush;
    get_flattened_sequence_data_error = persistent_sequence_setup_error == 0
        ? entry(kGetFlattenedSequenceData, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
    std::cerr << "stage:get_flattened_sequence_data_end error="
              << get_flattened_sequence_data_error << "\n" << std::flush;
    void* flattened_copy = read<void*>(output, kOutSequenceData);
    original_sequence_preserved = get_flattened_sequence_data_error == 0 &&
        original_handle && flattened_copy && original_handle != flattened_copy &&
        host_handle_is_live(original_handle) && host_handle_is_live(flattened_copy);
    if (original_sequence_preserved) {
      dispose_handle(reinterpret_cast<void**>(flattened_copy));
      flattened_handle_host_disposed = !host_handle_is_live(flattened_copy);
      write<void*>(input, kInSequenceData, original_handle);
      write<void*>(output, kOutSequenceData, original_handle);
    }
    bool frame_guards = false;
    persistent_frame_errors[0] = original_sequence_preserved && flattened_handle_host_disposed
        ? render_once(entry, input, output, "default", render_width, render_height,
                      render_rowbytes, input_hash, persistent_frame_hashes[0], frame_guards,
                      nullptr, nullptr, nullptr, 0, 0, nullptr, 0, 1, 1, 1, 4, false)
        : -1;
    guards_intact = frame_guards;
    std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
    persistent_sequence_setdown_error = persistent_frame_errors[0] == 0
        ? invoke_sequence_selector(entry, kSequenceSetdown, input.data(), output.data()) : -1;
    std::cerr << "stage:sequence_setdown_end error=" << persistent_sequence_setdown_error
              << "\n" << std::flush;
    write<void*>(input, kInSequenceData, nullptr);
    output_hash = persistent_frame_hashes[0];
    render_error = persistent_sequence_setup_error == 0 &&
        get_flattened_sequence_data_error == 0 && original_sequence_preserved &&
        flattened_handle_host_disposed && persistent_frame_errors[0] == 0 &&
        persistent_sequence_setdown_error == 0 ? 0 : -1;
  } else if (params_error == 0 && image_render_supported && depth_supported && flattened_sequence) {
    aexcompat::mask_runtime::set_model_enabled(true);
    configure_mask_scene("rectangle");
    std::cerr << "stage:sequence_setup_begin\n" << std::flush;
    persistent_sequence_setup_error = invoke_sequence_selector(
        entry, kSequenceSetup, input.data(), output.data());
    std::cerr << "stage:sequence_setup_end error=" << persistent_sequence_setup_error
              << "\n" << std::flush;
    void* unflattened_handle = read<void*>(output, kOutSequenceData);
    write<void*>(input, kInSequenceData, unflattened_handle);
    std::cerr << "stage:sequence_flatten_begin\n" << std::flush;
    sequence_flatten_error = persistent_sequence_setup_error == 0
        ? entry(kSequenceFlatten, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
    std::cerr << "stage:sequence_flatten_end error=" << sequence_flatten_error
              << "\n" << std::flush;
    void* flattened_handle = read<void*>(output, kOutSequenceData);
    flattened_handle_replaced = sequence_flatten_error == 0 && flattened_handle &&
        flattened_handle != unflattened_handle && !host_handle_is_live(unflattened_handle);
    write<void*>(input, kInSequenceData, flattened_handle);
    std::cerr << "stage:sequence_resetup_begin\n" << std::flush;
    sequence_resetup_error = flattened_handle_replaced
        ? invoke_sequence_selector(entry, kSequenceResetup, input.data(), output.data()) : -1;
    std::cerr << "stage:sequence_resetup_end error=" << sequence_resetup_error
              << "\n" << std::flush;
    void* resetup_handle = read<void*>(output, kOutSequenceData);
    resetup_handle_replaced = sequence_resetup_error == 0 && resetup_handle &&
        resetup_handle != flattened_handle && host_handle_is_live(flattened_handle) &&
        host_handle_is_live(resetup_handle);
    if (resetup_handle_replaced) {
      dispose_handle(reinterpret_cast<void**>(flattened_handle));
      flattened_handle_host_disposed = !host_handle_is_live(flattened_handle);
      write<void*>(input, kInSequenceData, resetup_handle);
    }
    bool frame_guards = false;
    persistent_frame_errors[0] = resetup_handle_replaced && flattened_handle_host_disposed
        ? render_once(entry, input, output, "default", render_width, render_height,
                      render_rowbytes, input_hash, persistent_frame_hashes[0], frame_guards,
                      nullptr, nullptr, nullptr, 0, 0, nullptr, 0, 1, 1, 1, 4, false)
        : -1;
    guards_intact = frame_guards;
    std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
    persistent_sequence_setdown_error = persistent_frame_errors[0] == 0
        ? invoke_sequence_selector(entry, kSequenceSetdown, input.data(), output.data()) : -1;
    std::cerr << "stage:sequence_setdown_end error=" << persistent_sequence_setdown_error
              << "\n" << std::flush;
    write<void*>(input, kInSequenceData, nullptr);
    output_hash = persistent_frame_hashes[0];
    render_error = persistent_sequence_setup_error == 0 && sequence_flatten_error == 0 &&
        sequence_resetup_error == 0 && flattened_handle_replaced &&
        resetup_handle_replaced && flattened_handle_host_disposed &&
        persistent_frame_errors[0] == 0 && persistent_sequence_setdown_error == 0 ? 0 : -1;
  } else if (params_error == 0 && image_render_supported && depth_supported && persistent_sequence) {
    std::cerr << "stage:sequence_setup_begin\n" << std::flush;
    persistent_sequence_setup_error = invoke_sequence_selector(
        entry, kSequenceSetup, input.data(), output.data());
    std::cerr << "stage:sequence_setup_end error=" << persistent_sequence_setup_error
              << "\n" << std::flush;
    write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData));
    for (int frame = 0; frame < 2 && persistent_sequence_setup_error == 0; ++frame) {
      int32_t frame_width = 0, frame_height = 0, frame_rowbytes = 0;
      std::string frame_input_hash;
      bool frame_guards = false;
      persistent_frame_errors[frame] = render_once(
          entry, input, output, "default", frame_width, frame_height, frame_rowbytes,
          frame_input_hash, persistent_frame_hashes[frame], frame_guards,
          nullptr, nullptr, nullptr, 0, 0, nullptr, frame, 1, 2, 1, 4, false);
      if (frame == 0) {
        render_width = frame_width; render_height = frame_height;
        render_rowbytes = frame_rowbytes; input_hash = frame_input_hash;
      }
      guards_intact = frame == 0 ? frame_guards : guards_intact && frame_guards;
    }
    std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
    persistent_sequence_setdown_error = persistent_sequence_setup_error == 0
        ? invoke_sequence_selector(entry, kSequenceSetdown, input.data(), output.data()) : -1;
    std::cerr << "stage:sequence_setdown_end error=" << persistent_sequence_setdown_error
              << "\n" << std::flush;
    write<void*>(input, kInSequenceData, nullptr);
    output_hash = persistent_frame_hashes[1];
    render_error = persistent_sequence_setup_error == 0 &&
        persistent_frame_errors[0] == 0 && persistent_frame_errors[1] == 0 &&
        persistent_sequence_setdown_error == 0 ? 0 : -1;
  } else if (params_error == 0 && image_render_supported && depth_supported && concurrent_render) {
    std::array<int32_t, 2> widths{}, heights{}, rowbytes{};
    std::array<std::string, 2> input_hashes{};
    auto run_thread = [&](std::size_t index) {
      auto thread_input = input;
      auto thread_output = output;
      thread_errors[index] = render_once(entry, thread_input, thread_output, "default",
          widths[index], heights[index], rowbytes[index], input_hashes[index],
          thread_hashes[index], thread_guards[index], nullptr);
    };
    std::thread first(run_thread, 0); std::thread second(run_thread, 1);
    first.join(); second.join();
    render_width = widths[0]; render_height = heights[0]; render_rowbytes = rowbytes[0];
    input_hash = input_hashes[0]; output_hash = thread_hashes[0];
    guards_intact = thread_guards[0] && thread_guards[1];
    render_error = thread_errors[0] == 0 && thread_errors[1] == 0 &&
        widths[0] == widths[1] && heights[0] == heights[1] && rowbytes[0] == rowbytes[1] &&
        input_hashes[0] == input_hashes[1] && thread_hashes[0] == thread_hashes[1] ? 0 : -1;
  } else if (params_error == 0 && image_render_supported && depth_supported &&
             invocation.render_session_mode) {
    const auto session_outcome = run_render_session(
        entry, input, output, &invocation.requested_parameters, invocation.external_width,
        invocation.external_height, invocation.external_time_step,
        invocation.external_total_time,
        invocation.external_time_scale, invocation.external_pixel_bytes,
        invocation.external_layers.empty() ? nullptr : &invocation.external_layers);
    persistent_sequence_setup_error = session_outcome.setup_error;
    persistent_sequence_setdown_error = session_outcome.setdown_error;
    render_width = session_outcome.width;
    render_height = session_outcome.height;
    render_rowbytes = session_outcome.rowbytes;
    input_hash = session_outcome.input_hash;
    output_hash = session_outcome.output_hash;
    guards_intact = session_outcome.guards_intact;
    session_protocol_violation = session_outcome.protocol_violation;
    session_invariant_failure = session_outcome.invariant_failure;
    render_error = session_outcome.render_error;
  } else if (params_error == 0 && image_render_supported && depth_supported) {
    render_error = render_once(entry, input, output, case_id, render_width, render_height,
                               render_rowbytes, input_hash, output_hash, guards_intact,
                               invocation.request_mode ? &invocation.requested_parameters : nullptr,
                               invocation.image_mode ? &invocation.external_rgba : nullptr,
                               invocation.image_mode ? &invocation.external_output : nullptr,
                               invocation.external_width, invocation.external_height,
                               invocation.layered_image_mode ? &invocation.external_layers : nullptr,
                               invocation.external_current_time, invocation.external_time_step,
                               invocation.external_total_time, invocation.external_time_scale,
                               invocation.external_pixel_bytes);
  }
  std::cerr << "stage:render_end error=" << render_error << "\n" << std::flush;
  return result;
}

SmartFinalDispatchResult run_smart_final_dispatch(const FinalDispatchRequest& request) {
  using namespace aexcompat::l2_detail;
  SmartFinalDispatchResult result;
  const EffectEntry entry = request.entry;
  auto& input = *request.input;
  auto& output = *request.output;
  const InvocationState& invocation = *request.invocation;
  wchar_t** argv = request.argv;
  const int32_t params_error = request.params_error;
  const bool image_render_supported = request.image_render_supported;
  const bool depth_supported = request.depth_supported;
  const bool smart_render_supported = request.smart_render_supported;
  std::string& case_id = result.case_id;
  SmartResult& smart = result.smart;
  bool& lifetime_fault_observed = result.lifetime_fault_observed;
  case_id = invocation.request_mode ? (invocation.smart_force_cpu ? "request_cpu" :
      (invocation.smart_opencl ? "gpu_opencl_float32" :
       (invocation.smart_directx ? "gpu_directx_float32" : "request"))) : "";
  if (!invocation.request_mode)
    for (const wchar_t* p = argv[4]; *p; ++p) {
      if (*p > 0x7f) { result.case_id_rejected = true; return result; }
      case_id.push_back(static_cast<char>(*p));
    }
  std::cerr << "stage:smart_render_begin\n" << std::flush;
  if (invocation.render_session_mode) {
    // Resident smart session (protocol v1.1). When the plug-in cannot render
    // (bad params, unsupported depth, no SmartFX support) the loop is never
    // entered; the broker observes the nonzero process exit instead of a
    // hanging session, exactly like the classic session branch.
    if (params_error == 0 && image_render_supported && depth_supported &&
        smart_render_supported) {
      const auto outcome = run_smart_render_session(
          entry, input, output, &invocation.requested_parameters, case_id,
          invocation.external_width, invocation.external_height,
          invocation.external_time_step, invocation.external_total_time,
          invocation.external_time_scale, invocation.external_pixel_bytes,
          invocation.external_layers.empty() ? nullptr : &invocation.external_layers);
      smart = outcome.last;
      // The report's guard verdict is the session-level one: a per-frame
      // guard violation invalidated the session (exit 24), and an empty
      // session never built a guarded buffer to corrupt.
      smart.guards_intact = outcome.session.guards_intact;
      result.session_protocol_violation = outcome.session.protocol_violation;
      result.session_invariant_failure = outcome.session.invariant_failure;
      result.session_frames_attempted = outcome.session.frames_attempted;
      result.session_sequence_setup_error = outcome.session.setup_error;
      result.session_sequence_setdown_error = outcome.session.setdown_error;
      result.session_render_error = outcome.session.render_error;
    }
  } else {
  smart = params_error == 0 && image_render_supported && depth_supported &&
      smart_render_supported
      ? smart_render_once(entry, input, output, case_id,
                          invocation.request_mode ? &invocation.requested_parameters : nullptr,
                          invocation.smart_image_mode ? &invocation.external_rgba : nullptr,
                          invocation.smart_image_mode ? &invocation.external_output : nullptr,
                          invocation.external_width, invocation.external_height,
                          invocation.smart_layered_image_mode ? &invocation.external_layers : nullptr,
                          invocation.external_current_time, invocation.external_time_step,
                          invocation.external_total_time, invocation.external_time_scale,
                          invocation.external_pixel_bytes)
      : SmartResult{};
  }
  lifetime_fault_observed = invocation.mask_double_dispose_mode
      ? verify_mask_double_dispose_rejected()
      : invocation.stream_live_value_dispose_mode
          ? verify_stream_dispose_with_live_value_rejected()
          : false;
  std::cerr << "stage:smart_render_end pre_error=" << smart.pre_error
            << " render_error=" << smart.render_error << "\n" << std::flush;
  return result;
}

}  // namespace aexcompat::worker_runtime::invocation
