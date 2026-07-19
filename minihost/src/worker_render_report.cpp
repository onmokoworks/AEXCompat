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

void emit(const ReportSnapshot& snapshot, std::ostream& output) {
  output << snapshot.json();
}

}  // namespace aexcompat::worker_render_report
