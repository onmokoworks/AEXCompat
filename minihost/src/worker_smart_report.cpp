#include "worker_smart_report.hpp"

#include "gpu_directx_backend.hpp"
#include "gpu_memory_world_transport.hpp"
#include "gpu_opencl_backend.hpp"
#include "host_audio_runtime.hpp"
#include "native_stdout_guard.hpp"
#include "runtime_module_audit.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_parameter_execution.hpp"
#include "worker_pf_pixel_format_registry.hpp"
#include "worker_render_report.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_suite_call_slot_probe.hpp"
#include "worker_suite_registry.hpp"
#include "worker_ui_event_execution.hpp"
#include "worker_world_registry.hpp"

#include <atomic>
#include <cstdint>
#include <iostream>
#include <sstream>
#include <string>

namespace aexcompat::l2_detail {

// Worker-entry owned custom-UI/telemetry state and helpers; the definitions
// stay in l2_main with the dispatch that mutates them.
namespace {
auto& g_smart_ui_telemetry =
    aexcompat::worker_runtime::ui_event_execution::custom_ui_telemetry();
auto& g_render_click_enabled = g_smart_ui_telemetry.render_click_enabled;
auto& g_render_draw_enabled = g_smart_ui_telemetry.render_draw_enabled;
auto& g_render_click_error = g_smart_ui_telemetry.render_click_error;
auto& g_render_click_out_flags = g_smart_ui_telemetry.render_click_out_flags;
auto& g_render_click_changed_value = g_smart_ui_telemetry.render_click_changed_value;
auto& g_render_draw_error = g_smart_ui_telemetry.render_draw_error;
auto& g_render_draw_out_flags = g_smart_ui_telemetry.render_draw_out_flags;
auto& g_render_ui_lifecycle_errors = g_smart_ui_telemetry.render_ui_lifecycle_errors;
auto& g_render_ui_context_closed = g_smart_ui_telemetry.render_ui_context_closed;
auto& g_app_color_picker_calls = g_smart_ui_telemetry.app_color_picker_calls;
auto& g_app_invalidate_rect_calls = g_smart_ui_telemetry.app_invalidate_rect_calls;
auto& g_app_picker_color = g_smart_ui_telemetry.app_picker_color;
}  // namespace
// Mirrors l2_main's frozen guid mix-in transport bound; the report publishes
// it beside the observed sizes.
constexpr uint32_t kMaxGuidMixInBytes = 1024 * 1024;
std::string escape(const std::string&);
std::string world_debug_report_json();
std::string missing_suites_report_json();
std::string unsupported_suite_calls_report_json();
std::string suite_timeline_report_json();
std::string live_suite_lease_summary();
uint32_t suite_acquire_count();
uint32_t suite_release_count();
std::size_t live_suite_lease_count();
uint32_t live_suite_reference_count();
bool suite_leases_balanced();
const aexcompat::host_audio::Telemetry& audio_telemetry();
bool audio_handle_lifetimes_balanced();
bool param_checkouts_balanced();
std::size_t mask_open_count();
std::size_t mask_tangent_vertex_count();
bool mask_lifetimes_balanced();

bool emit_smart_completion_report(const SmartCompletionInputs& in) {
  namespace report = aexcompat::worker_render_report;
  namespace gpu_transport = aexcompat::gpu_runtime::memory_world_transport;
  namespace opencl = aexcompat::gpu_runtime::opencl;
  namespace directx_backend = aexcompat::gpu_runtime::directx_backend;
  using aexcompat::worker_runtime::parameter_execution::requested_parameters_json;
  using aexcompat::worker_runtime::parameter_execution::requested_value;
  const auto& smart = *in.smart;
  const auto& arbitrary = worker_runtime::parameters::state().arbitrary;
  const auto& checkout = worker_runtime::parameters::state().checkout;
  const auto mask_report = aexcompat::mask_runtime::snapshot();
  const auto utility_undo_groups = report::capture_utility_undo_groups();
  std::ostringstream protocol;
  report::ReportSnapshot report_snapshot(protocol);
  const auto& host_telemetry = aexcompat::worker_runtime::smart::host_telemetry();
  const bool host_state_clean =
                in.parameter_count_contract_valid &&
                in.arbitrary_defaults_disposed && arbitrary.invalid_operations == 0 &&
                smart.guards_intact &&
                worker_runtime::handles::handle_lifetimes_balanced() &&
                aexcompat::world_registry::lifetimes_balanced() &&
                gpu_transport::gpu_memory_lifetimes_balanced() &&
                audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
                utility_undo_groups.balanced && utility_undo_groups.operations_valid &&
                param_checkouts_balanced() &&
                ((!g_render_click_enabled && !g_render_draw_enabled) ||
                 g_render_ui_context_closed);
  // Session completion is the session mechanics verdict (hoisted setup and
  // setdown clean, no protocol/invariant break): frame-local errors were
  // already reported through frame_done and the broker owned the continue
  // decision, so the last frame's selector errors do not fail a clean close.
  const bool smart_completed = in.session_mode
      ? in.session_render_error == 0 && host_state_clean
      : smart.pre_error == 0 && smart.render_error == 0 && smart.rects_valid &&
                smart.gpu_setup_error == 0 && smart.gpu_setdown_error == 0 &&
                host_state_clean;
  const std::string report_pixel_format = smart.session_narrowed8
      ? "argb8" : smart.runtime->pixel_format;
  report::begin_smart(report_snapshot, {
      smart_completed,
      {in.global_error, in.params_error, in.advertised_out_flags, in.advertised_out_flags2},
      {in.image_render_supported, in.smart_render_supported, in.nop_render_advertised,
       in.input_write_advertised},
      {smart.runtime->wide_time_checkout_allowed, smart.runtime->shutter_dependency_advertised,
       !in.nop_render_advertised, smart.selector_dispatched, false},
      report::make_temporal_checkout_counters(
          report::LayerTemporalRefusals{smart.runtime->rejected_temporal_checkouts},
          report::ParameterTemporalRefusals{checkout.rejected_temporal}),
      {host_telemetry.comp_bg_color_successes.load(std::memory_order_relaxed),
       host_telemetry.comp_bg_color_rejections.load(std::memory_order_relaxed),
       host_telemetry.guid_mix_in_calls.load(std::memory_order_relaxed),
       host_telemetry.guid_mix_in_successes.load(std::memory_order_relaxed),
       host_telemetry.guid_mix_in_rejections.load(std::memory_order_relaxed),
       host_telemetry.guid_mix_in_last_size.load(std::memory_order_relaxed),
       host_telemetry.guid_mix_in_max_size.load(std::memory_order_relaxed), kMaxGuidMixInBytes,
       host_telemetry.guid_mix_in_last_result.load(std::memory_order_relaxed)},
      in.depth_supported,
      {smart.pre_error, smart.render_error, smart.selector_error, smart.gpu_setup_error,
       smart.gpu_setdown_error, smart.gpu_setdown_exception_code},
      {smart.gpu_render_possible, smart.gpu_render_dispatched},
      {smart.checkout_time, smart.checkout_time_step, smart.checkout_time_scale},
      smart.roi_contract_valid, smart.runtime->input_checkout_request,
      smart.runtime->map_checkout_request, smart.input_checkout_result_rect,
      smart.map_checkout_result_rect, smart.malformed_checkout_requests,
      smart.empty_checkout_pixel_denials, smart.returns_extra_pixels,
      smart.result_within_request, smart.extra_pixels_contract_violation,
      smart.empty_result_rect, smart.output_extent_hint,
      in.setdown_error, in.case_id, report_pixel_format,
      {smart.output_width, smart.output_height, smart.output_rowbytes},
      {in.external_size[0], in.external_size[1]},
      report_pixel_format == "argb32f" ? 16 :
          (report_pixel_format == "argb16" ? 8 : 4),
      smart.input_hash,
      smart.output_hash, smart.rects_valid, world_debug_report_json(),
      smart.empty_layer_param_checkouts,
      smart.empty_layer_param_pixel_checkouts,
      smart.empty_result_passthrough});
  const auto& attempt = smart.auto_gpu_attempt;
  report_snapshot.stream() << ",\"gpu_auto8_attempt\":";
  if (!attempt.attempted) {
    report_snapshot.stream() << "null";
  } else {
    report_snapshot.stream()
        << "{\"setup_dispatched\":" << (attempt.setup_dispatched ? "true" : "false")
        << ",\"render_dispatched\":" << (attempt.render_dispatched ? "true" : "false")
        << ",\"fallback_used\":" << (attempt.fallback_used ? "true" : "false")
        << ",\"setup_error\":" << attempt.setup_error
        << ",\"pre_error\":" << attempt.pre_error
        << ",\"render_error\":" << attempt.render_error
        << ",\"setdown_error\":" << attempt.setdown_error
        << ",\"cleanup_error\":" << attempt.cleanup_error
        << ",\"lifecycle_error\":" << attempt.lifecycle_error
        << ",\"fallback_reason\":\"" << attempt.fallback_reason << "\""
        << ",\"internal_pixel_format\":\"argb32f\""
        << ",\"internal_float_input_sha256\":\"" << attempt.internal_float_input_sha256 << "\""
        << ",\"internal_float_output_sha256\":\"" << attempt.internal_float_output_sha256 << "\"}";
  }
  const auto& audio = audio_telemetry();
  report::append_audio(report_snapshot, {
      audio.usage_advertised, audio.checkout_allowed, audio.source_available,
      audio.unadvertised_checkout_calls,
      audio.rejected_unadvertised_checkouts, audio.rejected_format_requests,
      audio.handle_exhaustions, audio.peak_live_handles,
      audio.last_checkout_index, audio.checkout_calls, audio.checkin_calls,
      audio.automatic_checkins, audio.get_data_calls, audio.invalid_operations,
      audio.last_checkout_start_time, audio.last_checkout_duration,
      audio.last_checkout_time_scale, audio.last_window_start_sample,
      audio.last_window_sample_count, audio.last_window_silence_samples,
      audio.last_output_rate, audio.last_output_bytes_per_sample,
      audio.last_output_channels, audio.last_output_format,
      audio.last_returned_sample_frames, audio_handle_lifetimes_balanced()});
  if (in.session_mode)
    report::append_smart_session(report_snapshot, {
        in.session_frames_attempted, in.session_sequence_setup_error,
        in.session_sequence_setdown_error, in.session_render_error,
        in.session_protocol_violation, in.session_invariant_failure});
  report::append_custom_ui(report_snapshot, {
      g_render_click_enabled, g_render_click_error, g_render_click_out_flags,
      g_render_click_changed_value, g_render_draw_enabled, g_render_draw_error,
      g_render_draw_out_flags, g_render_ui_lifecycle_errors, g_render_ui_context_closed,
      g_app_color_picker_calls, g_app_invalidate_rect_calls, g_app_picker_color});
  report::append_smart_context(report_snapshot, {
      smart.result_rect, smart.max_result_rect,
      {smart.guards_intact, smart.output_pixels_valid, param_checkouts_balanced()},
      {checkout.checkout_calls, checkout.checkin_calls, checkout.automatic_checkins,
       checkout.invalid_checkins}, in.request_mode,
      {in.downsample_x[0], in.downsample_x[1]},
      {in.downsample_y[0], in.downsample_y[1]},
      {in.pixel_aspect_ratio[0], in.pixel_aspect_ratio[1]},
      {in.resolution[0], in.resolution[1]},
      {in.context_head[0], in.context_head[1], in.context_head[2], in.context_head[3],
       in.context_head[4], in.context_head[5]},
      {in.context_zoom[0], in.context_zoom[1]},
      {in.context_origin[0], in.context_origin[1]},
      {in.context_extent[0], in.context_extent[1]}});
  const auto handle_stats = worker_runtime::handles::statistics();
  const auto world_stats = aexcompat::world_registry::statistics();
  report::append_smart_lifetimes(report_snapshot, {
      aexcompat::mask_runtime::mask_scene_id(), {static_cast<int64_t>(mask_report.active_masks), static_cast<int64_t>(mask_open_count()), static_cast<int64_t>(mask_tangent_vertex_count())},
      mask_lifetimes_balanced(), {mask_report.masks_acquired, mask_report.masks_disposed,
      mask_report.streams_acquired, mask_report.streams_disposed, mask_report.values_acquired,
      mask_report.values_disposed}, in.lifetime_fault_observed, suite_leases_balanced(),
      {static_cast<int64_t>(suite_acquire_count()), static_cast<int64_t>(suite_release_count()), static_cast<int64_t>(live_suite_lease_count()),
       static_cast<int64_t>(live_suite_reference_count())},
      missing_suites_report_json() + unsupported_suite_calls_report_json() +
          suite_timeline_report_json() +
          aexcompat::worker_runtime::suite_call_slot_probe::report_json(),
      live_suite_lease_summary(),
      // #1182 (owner-directed): a rejected suite release is contained as a no-op
      // (see worker_render_report.cpp) and is a benign warning recorded in the
      // suite_timeline, not a session-failing fault. Keep only genuine passed-in
      // suite faults; handle/world double-dispose faults stay fail-closed.
      in.suite_fault_observed,
      worker_runtime::handles::handle_lifetimes_balanced(),
      {handle_stats.created, handle_stats.disposed},
      {arbitrary.copy_calls, arbitrary.dispose_calls, arbitrary.print_calls,
       arbitrary.print_failures, arbitrary.roundtrip_calls, arbitrary.roundtrip_failures,
       arbitrary.scan_calls, arbitrary.scan_failures, arbitrary.compare_disagreements,
       arbitrary.new_calls, arbitrary.interpolation_calls,
       arbitrary.interpolation_failures, arbitrary.invalid_operations},
      arbitrary.last_interpolation_amount,
      {static_cast<int64_t>(handle_stats.automatic_pre_render_disposals), static_cast<int64_t>(handle_stats.locks), static_cast<int64_t>(handle_stats.unlocks),
       static_cast<int64_t>(handle_stats.live_count), static_cast<int64_t>(handle_stats.live_bytes), static_cast<int64_t>(handle_stats.invalid_operations), 0},
      in.handle_fault_observed, in.world_fault_observed,
      aexcompat::world_registry::lifetimes_balanced(),
      {static_cast<int64_t>(world_stats.created), static_cast<int64_t>(world_stats.disposed), static_cast<int64_t>(world_stats.live_count), static_cast<int64_t>(world_stats.live_bytes),
       static_cast<int64_t>(world_stats.invalid_operations)},
      gpu_transport::gpu_memory_lifetimes_balanced(),
      {static_cast<int64_t>(gpu_transport::allocations_created), static_cast<int64_t>(gpu_transport::allocations_freed), static_cast<int64_t>(gpu_transport::live_allocation_count()),
       static_cast<int64_t>(gpu_transport::live_memory_bytes()), static_cast<int64_t>(gpu_transport::exclusive_access_depth()),
       static_cast<int64_t>(gpu_transport::invalid_memory_operations)}});
  report::append_seh_diagnostics(report_snapshot, report::capture_seh_diagnostics());
  const auto directx_stats = directx_backend::diagnostics();
  const auto aegp_memory_stats = worker_runtime::handles::aegp_memory_statistics();
  const auto pixel_format_stats = pixel_format_telemetry();
  report::append_smart_faults(report_snapshot, {
      {static_cast<int64_t>(gpu_transport::cuda_upload_bytes), static_cast<int64_t>(gpu_transport::cuda_download_bytes), static_cast<int64_t>(gpu_transport::cuda_sync_failures),
       gpu_transport::last_cuda_device_count, gpu_transport::last_cuda_device_index},
      {static_cast<int64_t>(gpu_transport::opencl_upload_bytes), static_cast<int64_t>(gpu_transport::opencl_download_bytes), static_cast<int64_t>(gpu_transport::opencl_sync_failures),
       opencl::last_device_count(), opencl::last_device_index()},
      {static_cast<int64_t>(directx_stats.device_count), static_cast<int64_t>(directx_stats.device_index), static_cast<int64_t>(directx_stats.upload_bytes),
       static_cast<int64_t>(directx_stats.download_bytes), static_cast<int64_t>(directx_stats.sync_failures)}, directx_stats.context_used,
      in.pixel_format_fault_observed,
      {static_cast<int64_t>(pixel_format_stats.add_calls),
       static_cast<int64_t>(pixel_format_stats.clear_calls),
       static_cast<int64_t>(pixel_format_stats.supported_count),
       pixel_format_stats.invalid_operations},
      {in.outline_fault_observed, in.mask_attribute_fault_observed, in.stream_metadata_fault_observed,
       in.keyframe_fault_observed, in.dynamic_stream_fault_observed, in.aegp_memory_fault_observed, false},
      {mask_report.outline_mutations, mask_report.invalid_outline_operations,
       mask_report.mask_mutations, mask_report.invalid_mask_operations,
       g_stream_metadata_queries, g_stream_duplicates, g_invalid_stream_operations,
       mask_report.keyframe_mutations, mask_report.invalid_keyframe_operations,
       g_dynamic_stream_queries, g_dynamic_stream_mutations, g_invalid_dynamic_stream_operations,
       0, 0, 0},
      {static_cast<int64_t>(aegp_memory_stats.created), static_cast<int64_t>(aegp_memory_stats.freed), static_cast<int64_t>(aegp_memory_stats.live_count),
       static_cast<int64_t>(aegp_memory_stats.live_bytes), static_cast<int64_t>(aegp_memory_stats.invalid_operations)}});
  report::append_utility_undo_groups(report_snapshot, utility_undo_groups);
  report::finish_requested_parameters(report_snapshot, {
      requested_parameters_json(*in.requested_parameters),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"amount")),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"direction")),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"seed")),
      requested_value(*in.requested_parameters, L"mix"),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"invert_map")),
      !in.nop_render_advertised, worker_runtime::module_audit_json()});
  report::emit(report_snapshot, protocol);
  if (!protocol.good()) return false;
  return worker_runtime::emit_protocol_stdout(protocol.str());
}

}  // namespace aexcompat::l2_detail
