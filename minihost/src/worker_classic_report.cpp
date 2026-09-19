#include "worker_classic_report.hpp"

#include "gpu_memory_world_transport.hpp"
#include "host_audio_runtime.hpp"
#include "native_stdout_guard.hpp"
#include "worker_aegp_async_layer_runtime.hpp"
#include "worker_classic_runtime.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_parameter_execution.hpp"
#include "worker_pf_ae_channel_runtime.hpp"
#include "runtime_module_audit.hpp"
#include "worker_pf_path_runtime.hpp"
#include "worker_render_receipts.hpp"
#include "worker_render_report.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_ui_event_execution.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <cstdint>
#include <iostream>
#include <sstream>
#include <string>

namespace aexcompat::l2_detail {

// Worker-entry owned custom-UI/callback/context state and helpers; the
// definitions stay in l2_main with the dispatch that mutates them.
namespace {
auto& g_classic_ui_telemetry =
    aexcompat::worker_runtime::ui_event_execution::custom_ui_telemetry();
auto& g_render_click_enabled = g_classic_ui_telemetry.render_click_enabled;
auto& g_render_draw_enabled = g_classic_ui_telemetry.render_draw_enabled;
auto& g_render_click_error = g_classic_ui_telemetry.render_click_error;
auto& g_render_click_out_flags = g_classic_ui_telemetry.render_click_out_flags;
auto& g_render_click_changed_value = g_classic_ui_telemetry.render_click_changed_value;
auto& g_render_draw_error = g_classic_ui_telemetry.render_draw_error;
auto& g_render_draw_out_flags = g_classic_ui_telemetry.render_draw_out_flags;
auto& g_render_ui_lifecycle_errors = g_classic_ui_telemetry.render_ui_lifecycle_errors;
auto& g_render_ui_context_closed = g_classic_ui_telemetry.render_ui_context_closed;
auto& g_app_color_picker_calls = g_classic_ui_telemetry.app_color_picker_calls;
auto& g_app_invalidate_rect_calls = g_classic_ui_telemetry.app_invalidate_rect_calls;
auto& g_app_picker_color = g_classic_ui_telemetry.app_picker_color;
auto& g_register_ui_calls = g_classic_ui_telemetry.register_ui_calls;
}  // namespace
namespace {
auto& g_report_callback_telemetry =
    aexcompat::worker_runtime::classic::host_callback_telemetry();
auto& g_transform_world_calls = g_report_callback_telemetry.transform_world_calls;
auto& g_last_transform_x = g_report_callback_telemetry.last_transform_x;
auto& g_last_transform_y = g_report_callback_telemetry.last_transform_y;
auto& g_last_transform_opacity = g_report_callback_telemetry.last_transform_opacity;
auto& g_abort_calls = g_report_callback_telemetry.abort_calls;
auto& g_progress_calls = g_report_callback_telemetry.progress_calls;
auto& g_last_progress_current = g_report_callback_telemetry.last_progress_current;
auto& g_last_progress_total = g_report_callback_telemetry.last_progress_total;
}  // namespace
std::string escape(const std::string&);
std::string world_debug_report_json();
const aexcompat::host_audio::Telemetry& audio_telemetry();
bool audio_handle_lifetimes_balanced();

bool emit_classic_completion_report(const ClassicCompletionInputs& in) {
  namespace report = aexcompat::worker_render_report;
  namespace gpu_transport = aexcompat::gpu_runtime::memory_world_transport;
  using aexcompat::worker_runtime::parameter_execution::requested_parameters_json;
  using aexcompat::worker_runtime::parameter_execution::requested_value;
  const auto& arbitrary = worker_runtime::parameters::state().arbitrary;
  const auto& parameter_ui = worker_runtime::parameters::state().ui;
  const auto classic_diagnostics = aexcompat::worker_runtime::classic::diagnostics();
  std::ostringstream protocol;
  report::ReportSnapshot report_snapshot(protocol);
  report::ClassicReport classic_report;
  classic_report.head = {
      in.render_error == 0 && in.parameter_count_contract_valid && in.guards_intact &&
                in.arbitrary_defaults_disposed && arbitrary.invalid_operations == 0 &&
                worker_runtime::handles::handle_lifetimes_balanced() &&
                aexcompat::world_registry::lifetimes_balanced() &&
                gpu_transport::gpu_memory_lifetimes_balanced() &&
                aexcompat::pf_path_runtime::lifetimes_balanced() &&
                aexcompat::render_receipts::lifetimes_balanced() &&
                aexcompat::aegp_async_layer::balanced() &&
                audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
                classic_diagnostics.balanced &&
                ((!g_render_click_enabled && !g_render_draw_enabled) ||
                 g_render_ui_context_closed),
      in.global_error, in.params_error, in.advertised_out_flags, in.advertised_out_flags2,
      in.image_render_supported, in.nop_render_advertised, in.input_write_advertised,
      in.expand_buffer_advertised, in.shrink_buffer_advertised,
      classic_diagnostics.wide_time_allowed, classic_diagnostics.rejected_temporal_checkouts,
      classic_diagnostics.shutter_dependency_advertised};
  const auto& audio_report = audio_telemetry();
  classic_report.audio = {
      audio_report.usage_advertised, audio_report.checkout_allowed, audio_report.source_available,
      audio_report.unadvertised_checkout_calls,
      audio_report.rejected_unadvertised_checkouts, audio_report.rejected_format_requests,
      audio_report.handle_exhaustions, audio_report.peak_live_handles,
      audio_report.last_checkout_index, audio_report.checkout_calls,
      audio_report.checkin_calls, audio_report.automatic_checkins,
      audio_report.get_data_calls, audio_report.invalid_operations,
      audio_report.last_checkout_start_time, audio_report.last_checkout_duration,
      audio_report.last_checkout_time_scale, audio_report.last_window_start_sample,
      audio_report.last_window_sample_count, audio_report.last_window_silence_samples,
      audio_report.last_output_rate, audio_report.last_output_bytes_per_sample,
      audio_report.last_output_channels, audio_report.last_output_format,
      audio_report.last_returned_sample_frames, audio_handle_lifetimes_balanced()};
  classic_report.sequence = {
      in.persistent_sequence, in.persistent_sequence_setup_error, in.persistent_sequence_setdown_error,
      in.persistent_frame_errors, in.persistent_frame_hashes, in.flattened_sequence,
      in.sequence_flatten_error, in.sequence_resetup_error, in.flattened_handle_replaced,
      in.resetup_handle_replaced, in.flattened_handle_host_disposed, in.copied_flattened_sequence,
      in.get_flattened_sequence_data_error, in.original_sequence_preserved};
  const auto& smart_runtime_state = aexcompat::worker_runtime::smart::state();
  const int32_t bytes_per_pixel = smart_runtime_state.pixel_format == "argb32f" ? 16 :
      (smart_runtime_state.pixel_format == "argb16" ? 8 : 4);
  classic_report.frame = {
      in.setdown_error,
      escape(in.frame_return_message),
      in.case_id, smart_runtime_state.pixel_format, in.render_width, in.render_height,
      in.render_rowbytes,
      in.render_width * bytes_per_pixel,
      std::max(0, in.render_rowbytes - in.render_width * bytes_per_pixel),
      in.input_hash, in.output_hash, in.guards_intact, world_debug_report_json()};
  const report::CustomUiSnapshot classic_custom_ui{
      g_render_click_enabled, g_render_click_error, g_render_click_out_flags,
      g_render_click_changed_value, g_render_draw_enabled, g_render_draw_error,
      g_render_draw_out_flags, g_render_ui_lifecycle_errors, g_render_ui_context_closed,
      g_app_color_picker_calls, g_app_invalidate_rect_calls, g_app_picker_color};
  const auto i64 = [](auto value) { return static_cast<int64_t>(value); };
  classic_report.callbacks = {
      classic_diagnostics.balanced,
      {i64(classic_diagnostics.checkout_calls), i64(classic_diagnostics.checkin_calls),
       i64(classic_diagnostics.automatic_checkins), i64(classic_diagnostics.invalid_checkins),
       i64(classic_diagnostics.last_index), i64(classic_diagnostics.last_time),
       i64(classic_diagnostics.last_time_step), i64(classic_diagnostics.last_time_scale)},
      escape(parameter_ui.options_button_name),
      {i64(parameter_ui.options_button_name_calls),
       i64(aexcompat::pf_ae_channel::channel_count_queries()),
       i64(g_transform_world_calls), i64(g_last_transform_x), i64(g_last_transform_y),
       i64(g_last_transform_opacity), i64(g_abort_calls), i64(g_progress_calls),
       i64(g_register_ui_calls), i64(g_last_progress_current), i64(g_last_progress_total)}};
  classic_report.threads = {in.concurrent_render, in.thread_errors, in.thread_hashes,
                            in.thread_guards};
  classic_report.context = {
      in.request_mode,
      {in.downsample_x[0], in.downsample_x[1]},
      {in.downsample_y[0], in.downsample_y[1]},
      {in.pixel_aspect_ratio[0], in.pixel_aspect_ratio[1]},
      {in.resolution[0], in.resolution[1]},
      {in.context_head[0], in.context_head[1], in.context_head[2], in.context_head[3],
       in.context_head[4], in.context_head[5]},
      {in.context_zoom[0], in.context_zoom[1]},
      {in.context_origin[0], in.context_origin[1]},
      {in.context_extent[0], in.context_extent[1]}};
  const report::RequestedParametersSnapshot classic_requested{
      requested_parameters_json(*in.requested_parameters),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"amount")),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"direction")),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"seed")),
      requested_value(*in.requested_parameters, L"mix"),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"invert_map")),
      !in.nop_render_advertised, worker_runtime::module_audit_json()};
  report::emit_classic_complete(report_snapshot, {
      classic_report, classic_custom_ui, report::capture_classic_subsystems(),
      report::capture_gpu_diagnostics(), report::capture_seh_diagnostics(), classic_requested,
      aexcompat::worker_runtime::classic::last_selector_dispatched(),
      in.depth_supported, in.render_error});
  report::emit(report_snapshot, protocol);
  if (!protocol.good()) return false;
  return worker_runtime::emit_protocol_stdout(protocol.str());
}

}  // namespace aexcompat::l2_detail
