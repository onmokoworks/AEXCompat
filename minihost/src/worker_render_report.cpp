#include "worker_render_report.hpp"

#include <ostream>
#include <iomanip>

namespace aexcompat::worker_render_report {

ReportSnapshot::ReportSnapshot(const std::ios& formatting_source) {
  stream_.copyfmt(formatting_source);
}

void append_custom_ui(ReportSnapshot& report, const CustomUiSnapshot& value) {
  report.stream()
      << ",\"custom_ui_click_dispatched\":" << (value.click_dispatched ? "true" : "false")
      << ",\"custom_ui_click_error\":" << value.click_error
      << ",\"custom_ui_click_out_flags\":" << value.click_out_flags
      << ",\"custom_ui_click_changed_value\":" << (value.click_changed_value ? "true" : "false")
      << ",\"custom_ui_draw_dispatched\":" << (value.draw_dispatched ? "true" : "false")
      << ",\"custom_ui_draw_error\":" << value.draw_error
      << ",\"custom_ui_draw_out_flags\":" << value.draw_out_flags
      << ",\"custom_ui_lifecycle_errors\":[" << value.lifecycle_errors[0] << ','
      << value.lifecycle_errors[1] << ',' << value.lifecycle_errors[2] << ','
      << value.lifecycle_errors[3] << ']'
      << ",\"custom_ui_context_closed\":" << (value.context_closed ? "true" : "false")
      << ",\"app_color_picker_calls\":" << value.color_picker_calls
      << ",\"app_invalidate_rect_calls\":" << value.invalidate_rect_calls
      << ",\"picker_color_rgba\":[" << value.picker_color[0] << ','
      << value.picker_color[1] << ',' << value.picker_color[2] << ','
      << value.picker_color[3] << ']';
}

void finish_requested_parameters(
    ReportSnapshot& report, const RequestedParametersSnapshot& value) {
  report.stream()
      << ",\"requested_parameters\":" << value.parameters_json
      << ",\"requested_amount\":" << value.amount
      << ",\"requested_direction\":" << value.direction
      << ",\"requested_seed\":" << value.seed
      << ",\"requested_mix\":" << std::setprecision(17) << value.mix
      << ",\"requested_invert_map\":" << value.invert_map
      << ",\"render_performed\":" << (value.render_performed ? "true" : "false")
      << ",\"module_audit\":" << value.module_audit_json << "}\n";
}

void begin_classic(ReportSnapshot& report, const ClassicReport::Head& value) {
  report.stream()
      << "{\"schema_version\":1,\"stage\":\"classic_render\",\"status\":\""
      << (value.completed ? "render_completed" : "render_failed")
      << "\",\"global_setup_error\":" << value.global_setup_error
      << ",\"params_setup_error\":" << value.params_setup_error
      << ",\"advertised_out_flags\":" << value.advertised_out_flags
      << ",\"advertised_out_flags2\":" << value.advertised_out_flags2
      << ",\"image_render_supported\":" << (value.image_render_supported ? "true" : "false")
      << ",\"nop_render_advertised\":" << (value.nop_render_advertised ? "true" : "false")
      << ",\"input_write_advertised\":" << (value.input_write_advertised ? "true" : "false")
      << ",\"expand_buffer_advertised\":" << (value.expand_buffer_advertised ? "true" : "false")
      << ",\"shrink_buffer_advertised\":" << (value.shrink_buffer_advertised ? "true" : "false")
      << ",\"input_buffer_writable\":" << (value.input_write_advertised ? "true" : "false")
      << ",\"wide_time_checkout_allowed\":" << (value.wide_time_checkout_allowed ? "true" : "false")
      << ",\"rejected_temporal_param_checkouts\":" << value.rejected_temporal_param_checkouts
      << ",\"shutter_dependency_advertised\":" << (value.shutter_dependency_advertised ? "true" : "false");
}

void append_classic_sequence(ReportSnapshot& report, const ClassicReport::Sequence& value) {
  report.stream()
      << ",\"persistent_sequence\":" << (value.persistent ? "true" : "false")
      << ",\"persistent_sequence_setup_error\":" << value.setup_error
      << ",\"persistent_sequence_setdown_error\":" << value.setdown_error
      << ",\"persistent_frame_errors\":[" << value.frame_errors[0] << ',' << value.frame_errors[1] << ']'
      << ",\"persistent_frame_hashes\":[\"" << value.frame_hashes[0] << "\",\"" << value.frame_hashes[1] << "\"]"
      << ",\"flattened_sequence\":" << (value.flattened ? "true" : "false")
      << ",\"sequence_flatten_error\":" << value.flatten_error
      << ",\"sequence_resetup_error\":" << value.resetup_error
      << ",\"flattened_handle_replaced\":" << (value.flattened_handle_replaced ? "true" : "false")
      << ",\"resetup_handle_replaced\":" << (value.resetup_handle_replaced ? "true" : "false")
      << ",\"flattened_handle_host_disposed\":" << (value.flattened_handle_host_disposed ? "true" : "false")
      << ",\"copied_flattened_sequence\":" << (value.copied_flattened ? "true" : "false")
      << ",\"get_flattened_sequence_data_error\":" << value.get_flattened_error
      << ",\"original_sequence_preserved\":" << (value.original_preserved ? "true" : "false");
}

void append_classic_audio(ReportSnapshot& report, const ClassicReport::Audio& value) {
  report.stream()
      << ",\"audio_usage_advertised\":" << (value.usage_advertised ? "true" : "false")
      << ",\"audio_checkout_allowed\":" << (value.checkout_allowed ? "true" : "false")
      << ",\"audio_source_available\":" << (value.source_available ? "true" : "false")
      << ",\"rejected_unadvertised_audio_checkouts\":" << value.rejected_unadvertised_checkouts
      << ",\"rejected_audio_format_requests\":" << value.rejected_format_requests
      << ",\"audio_handle_exhaustions\":" << value.handle_exhaustions
      << ",\"peak_live_audio_handles\":" << value.peak_live_handles
      << ",\"audio_checkout_calls\":" << value.checkout_calls
      << ",\"audio_checkin_calls\":" << value.checkin_calls
      << ",\"audio_get_data_calls\":" << value.get_data_calls
      << ",\"invalid_audio_operations\":" << value.invalid_operations
      << ",\"last_audio_checkout_start_time\":" << value.last_checkout_start_time
      << ",\"last_audio_checkout_duration\":" << value.last_checkout_duration
      << ",\"last_audio_checkout_time_scale\":" << value.last_checkout_time_scale
      << ",\"last_audio_window_start_sample\":" << value.last_window_start_sample
      << ",\"last_audio_window_sample_count\":" << value.last_window_sample_count
      << ",\"last_audio_window_silence_samples\":" << value.last_window_silence_samples
      << ",\"last_audio_output_rate_fixed\":" << value.last_output_rate
      << ",\"last_audio_output_bytes_per_sample\":" << value.last_output_bytes_per_sample
      << ",\"last_audio_output_channels\":" << value.last_output_channels
      << ",\"last_audio_output_format\":" << value.last_output_format
      << ",\"last_audio_returned_sample_frames\":" << value.last_returned_sample_frames
      << ",\"audio_lifetimes_balanced\":" << (value.lifetimes_balanced ? "true" : "false");
}

void append_classic_frame(ReportSnapshot& report, const ClassicReport::Frame& value) {
  report.stream()
      << ",\"global_setdown_error\":" << value.global_setdown_error
      << ",\"return_message\":\"" << value.escaped_return_message << '"'
      << ",\"case_id\":\"" << value.case_id << "\",\"pixel_format\":\"" << value.pixel_format
      << "\",\"width\":" << value.width << ",\"height\":" << value.height
      << ",\"rowbytes\":" << value.rowbytes
      << ",\"bytes_written_per_row\":" << value.bytes_written_per_row
      << ",\"undefined_tail_bytes_per_row\":" << value.undefined_tail_bytes_per_row
      << ",\"input_sha256\":\"" << value.input_sha256 << "\",\"output_sha256\":\""
      << value.output_sha256 << "\",\"guard_bytes_intact\":"
      << (value.guard_bytes_intact ? "true" : "false") << value.world_debug_json;
}

void append_classic_threads(ReportSnapshot& report, const ClassicReport::Threads& value) {
  report.stream()
      << ",\"concurrent_render\":" << (value.concurrent ? "true" : "false")
      << ",\"thread_1_error\":" << value.errors[0]
      << ",\"thread_2_error\":" << value.errors[1]
      << ",\"thread_1_sha256\":\"" << value.hashes[0] << '"'
      << ",\"thread_2_sha256\":\"" << value.hashes[1] << '"'
      << ",\"thread_1_guards_intact\":" << (value.guards_intact[0] ? "true" : "false")
      << ",\"thread_2_guards_intact\":" << (value.guards_intact[1] ? "true" : "false");
}

void append_gpu_diagnostics(ReportSnapshot& report, const GpuDiagnosticsSnapshot& value) {
  report.stream()
      << ",\"gpu_memory_lifetimes_balanced\":" << (value.memory_lifetimes_balanced ? "true" : "false")
      << ",\"cuda_context_used\":" << (value.cuda_context_used ? "true" : "false")
      << ",\"cuda_upload_bytes\":" << value.cuda[0]
      << ",\"cuda_download_bytes\":" << value.cuda[1]
      << ",\"cuda_sync_failures\":" << value.cuda[2]
      << ",\"cuda_device_count\":" << value.cuda[3]
      << ",\"cuda_device_index\":" << value.cuda[4]
      << ",\"opencl_context_used\":" << (value.opencl_context_used ? "true" : "false")
      << ",\"opencl_upload_bytes\":" << value.opencl[0]
      << ",\"opencl_download_bytes\":" << value.opencl[1]
      << ",\"opencl_sync_failures\":" << value.opencl[2]
      << ",\"opencl_device_count\":" << value.opencl[3]
      << ",\"opencl_device_index\":" << value.opencl[4]
      << ",\"directx_context_used\":" << (value.directx_context_used ? "true" : "false")
      << ",\"directx_device_count\":" << value.directx[0]
      << ",\"directx_device_index\":" << value.directx[1]
      << ",\"directx_upload_bytes\":" << value.directx[2]
      << ",\"directx_download_bytes\":" << value.directx[3]
      << ",\"directx_sync_failures\":" << value.directx[4]
      << ",\"gpu_allocations_created\":" << value.allocations[0]
      << ",\"gpu_allocations_freed\":" << value.allocations[1]
      << ",\"live_gpu_allocation_count\":" << value.allocations[2]
      << ",\"live_gpu_memory_bytes\":" << value.allocations[3]
      << ",\"gpu_exclusive_access_depth\":" << value.allocations[4]
      << ",\"invalid_gpu_memory_operations\":" << value.allocations[5];
}

void append_seh_diagnostics(ReportSnapshot& report, const SehDiagnosticsSnapshot& value) {
  report.stream()
      << ",\"last_seh_exception_code\":" << value.code
      << ",\"last_seh_exception_address\":" << value.address
      << ",\"last_seh_exception_module\":\"" << value.escaped_module << '"'
      << ",\"last_seh_selector\":\"" << value.escaped_selector << '"'
      << ",\"last_seh_error\":" << value.error;
}

void append_classic_subsystems(
    ReportSnapshot& report, const ClassicSubsystemDiagnostics& value) {
  report.stream()
      << ",\"suite_leases_balanced\":" << (value.suite_balanced ? "true" : "false")
      << ",\"suite_lease_warning\":" << (!value.suite_balanced ? "true" : "false")
      << ",\"suite_acquires\":" << value.suite_counts[0]
      << ",\"suite_releases\":" << value.suite_counts[1] << value.missing_suites_json
      << ",\"live_suite_lease_count\":" << value.suite_counts[2]
      << ",\"live_suite_reference_count\":" << value.suite_counts[3]
      << ",\"live_suite_leases\":\"" << value.live_suite_leases << '"'
      << ",\"handle_lifetimes_balanced\":" << (value.handle_balanced ? "true" : "false")
      << ",\"pf_path_lifetimes_balanced\":" << (value.path_balanced ? "true" : "false")
      << ",\"pf_path_checkout_calls\":" << value.path_counts[0]
      << ",\"pf_path_checkin_calls\":" << value.path_counts[1]
      << ",\"pf_path_mask_calls\":" << value.path_counts[2]
      << ",\"pf_path_preps_created\":" << value.path_counts[3]
      << ",\"pf_path_preps_disposed\":" << value.path_counts[4]
      << ",\"invalid_pf_path_operations\":" << value.path_counts[5]
      << ",\"pf_path_reject_reason\":" << value.path_counts[6]
      << ",\"pf_path_last_feather\":[" << value.path_feather[0] << ',' << value.path_feather[1] << ']'
      << ",\"pf_path_last_opacity\":" << value.path_opacity
      << ",\"pf_path_last_quality\":" << value.path_quality
      << ",\"pf_path_last_bounds\":[" << value.path_bounds[0] << ',' << value.path_bounds[1]
      << ',' << value.path_bounds[2] << ',' << value.path_bounds[3] << ']'
      << ",\"handles_created\":" << value.handles[0]
      << ",\"handles_disposed\":" << value.handles[1]
      << ",\"arbitrary_copy_calls\":" << value.arbitrary[0]
      << ",\"arbitrary_dispose_calls\":" << value.arbitrary[1]
      << ",\"arbitrary_print_calls\":" << value.arbitrary[2]
      << ",\"arbitrary_print_failures\":" << value.arbitrary[3]
      << ",\"arbitrary_roundtrip_calls\":" << value.arbitrary[4]
      << ",\"arbitrary_roundtrip_failures\":" << value.arbitrary[5]
      << ",\"arbitrary_scan_calls\":" << value.arbitrary[6]
      << ",\"arbitrary_scan_failures\":" << value.arbitrary[7]
      << ",\"arbitrary_compare_disagreements\":" << value.arbitrary[8]
      << ",\"arbitrary_new_calls\":" << value.arbitrary[9]
      << ",\"arbitrary_interpolation_calls\":" << value.arbitrary[10]
      << ",\"arbitrary_interpolation_failures\":" << value.arbitrary[11]
      << ",\"arbitrary_interpolation_amount\":" << value.arbitrary_interpolation_amount
      << ",\"invalid_arbitrary_operations\":" << value.arbitrary[12]
      << ",\"world_lifetimes_balanced\":" << (value.world_balanced ? "true" : "false")
      << ",\"worlds_created\":" << value.worlds[0]
      << ",\"worlds_disposed\":" << value.worlds[1]
      << ",\"receipt_lifetimes_balanced\":" << (value.receipt_balanced ? "true" : "false")
      << ",\"receipts_created\":" << value.receipts[0]
      << ",\"receipts_checked_in\":" << value.receipts[1]
      << ",\"live_receipts\":" << value.receipts[2]
      << ",\"live_receipt_bytes\":" << value.receipts[3]
      << ",\"invalid_receipt_operations\":" << value.receipts[4]
      << ",\"async_layer_requests_balanced\":" << (value.async_balanced ? "true" : "false")
      << ",\"async_layer_requests_created\":" << value.async[0]
      << ",\"async_layer_requests_completed\":" << value.async[1]
      << ",\"async_layer_requests_canceled\":" << value.async[2]
      << ",\"async_layer_callback_failures\":" << value.async[3]
      << ",\"async_layer_callback_exceptions\":" << value.async[4]
      << ",\"live_async_layer_requests\":" << value.async[5]
      << ",\"async_layer_reserved_bytes\":" << value.async[6];
}

void emit(const ReportSnapshot& snapshot, std::ostream& output) {
  output << snapshot.json();
}

}  // namespace aexcompat::worker_render_report
