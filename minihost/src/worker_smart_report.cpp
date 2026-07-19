#include "worker_smart_report.hpp"

#include "gpu_directx_backend.hpp"
#include "gpu_memory_world_transport.hpp"
#include "gpu_opencl_backend.hpp"
#include "host_audio_runtime.hpp"
#include "runtime_module_audit.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_custom_ui_state.hpp"
#include "worker_parameter_execution.hpp"
#include "worker_pf_pixel_format_registry.hpp"
#include "worker_render_report.hpp"
#include "worker_world_registry.hpp"

#include <atomic>
#include <cstdint>
#include <iostream>
#include <string>

namespace aexcompat::l2_detail {

// Worker-entry owned custom-UI/telemetry state and helpers; the definitions
// stay in l2_main with the dispatch that mutates them.
namespace {
auto& g_render_click_enabled = worker_runtime::custom_ui::state().render_click_enabled;
auto& g_render_draw_enabled = worker_runtime::custom_ui::state().render_draw_enabled;
auto& g_render_click_error = worker_runtime::custom_ui::state().render_click_error;
auto& g_render_click_out_flags = worker_runtime::custom_ui::state().render_click_out_flags;
auto& g_render_click_changed_value = worker_runtime::custom_ui::state().render_click_changed_value;
auto& g_render_draw_error = worker_runtime::custom_ui::state().render_draw_error;
auto& g_render_draw_out_flags = worker_runtime::custom_ui::state().render_draw_out_flags;
auto& g_render_ui_lifecycle_errors = worker_runtime::custom_ui::state().render_ui_lifecycle_errors;
auto& g_render_ui_context_closed = worker_runtime::custom_ui::state().render_ui_context_closed;
auto& g_app_color_picker_calls = worker_runtime::custom_ui::state().app_color_picker_calls;
auto& g_app_invalidate_rect_calls = worker_runtime::custom_ui::state().app_invalidate_rect_calls;
auto& g_app_picker_color = worker_runtime::custom_ui::state().app_picker_color;
}  // namespace
// Mirrors l2_main's frozen guid mix-in transport bound; the report publishes
// it beside the observed sizes.
constexpr uint32_t kMaxGuidMixInBytes = 1024 * 1024;
std::string escape(const std::string&);
std::string world_debug_report_json();
std::string missing_suites_report_json();
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

void emit_smart_completion_report(const SmartCompletionInputs& in) {
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
  report::ReportSnapshot report_snapshot(std::cout);
  const auto& host_telemetry = aexcompat::worker_runtime::smart::host_telemetry();
  const bool smart_completed = smart.pre_error == 0 && smart.render_error == 0 &&
                in.parameter_count_contract_valid && smart.rects_valid &&
                in.arbitrary_defaults_disposed && arbitrary.invalid_operations == 0 &&
                smart.gpu_setup_error == 0 && smart.gpu_setdown_error == 0 &&
                smart.guards_intact &&
                worker_runtime::handles::handle_lifetimes_balanced() &&
                aexcompat::world_registry::lifetimes_balanced() &&
                gpu_transport::gpu_memory_lifetimes_balanced() &&
                audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
                param_checkouts_balanced() &&
                ((!g_render_click_enabled && !g_render_draw_enabled) ||
                 g_render_ui_context_closed);
  report::begin_smart(report_snapshot, {
      smart_completed,
      {in.global_error, in.params_error, in.advertised_out_flags, in.advertised_out_flags2},
      {in.image_render_supported, in.smart_render_supported, in.nop_render_advertised,
       in.input_write_advertised},
      {smart.runtime->wide_time_checkout_allowed, smart.runtime->shutter_dependency_advertised,
       !in.nop_render_advertised, smart.selector_dispatched, false},
      smart.runtime->rejected_temporal_checkouts,
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
      in.setdown_error, in.case_id, smart.runtime->pixel_format,
      {smart.output_width, smart.output_height, smart.output_rowbytes},
      {in.external_size[0], in.external_size[1]},
      smart.runtime->pixel_format == "argb32f" ? 16 :
          (smart.runtime->pixel_format == "argb16" ? 8 : 4),
      smart.input_hash,
      smart.output_hash, smart.rects_valid, world_debug_report_json()});
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
       static_cast<int64_t>(live_suite_reference_count())}, missing_suites_report_json() + suite_timeline_report_json(), live_suite_lease_summary(),
      in.suite_fault_observed, worker_runtime::handles::handle_lifetimes_balanced(),
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
  report::append_smart_faults(report_snapshot, {
      {static_cast<int64_t>(gpu_transport::cuda_upload_bytes), static_cast<int64_t>(gpu_transport::cuda_download_bytes), static_cast<int64_t>(gpu_transport::cuda_sync_failures),
       gpu_transport::last_cuda_device_count, gpu_transport::last_cuda_device_index},
      {static_cast<int64_t>(gpu_transport::opencl_upload_bytes), static_cast<int64_t>(gpu_transport::opencl_download_bytes), static_cast<int64_t>(gpu_transport::opencl_sync_failures),
       opencl::last_device_count(), opencl::last_device_index()},
      {static_cast<int64_t>(directx_stats.device_count), static_cast<int64_t>(directx_stats.device_index), static_cast<int64_t>(directx_stats.upload_bytes),
       static_cast<int64_t>(directx_stats.download_bytes), static_cast<int64_t>(directx_stats.sync_failures)}, directx_stats.context_used,
      in.pixel_format_fault_observed,
      {static_cast<int64_t>(g_pixel_format_add_calls), static_cast<int64_t>(g_pixel_format_clear_calls), static_cast<int64_t>(g_supported_pixel_formats.size()),
       g_invalid_pixel_format_operations},
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
  report::finish_requested_parameters(report_snapshot, {
      requested_parameters_json(*in.requested_parameters),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"amount")),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"direction")),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"seed")),
      requested_value(*in.requested_parameters, L"mix"),
      static_cast<int32_t>(requested_value(*in.requested_parameters, L"invert_map")),
      !in.nop_render_advertised, worker_runtime::module_audit_json()});
  report::emit(report_snapshot, std::cout);
}

}  // namespace aexcompat::l2_detail
