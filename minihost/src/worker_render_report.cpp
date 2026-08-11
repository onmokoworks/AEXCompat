#include "worker_render_report.hpp"
#include "worker_callback_diagnostics.hpp"

#include "gpu_directx_backend.hpp"
#include "gpu_memory_world_transport.hpp"
#include "gpu_opencl_backend.hpp"
#include "worker_aegp_async_layer_runtime.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_pf_path_runtime.hpp"
#include "worker_render_receipts.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_suite_call_slot_probe.hpp"
#include "worker_suite_registry.hpp"
#include "worker_world_registry.hpp"

#include <cstddef>
#include <ostream>
#include <iomanip>
#include <string>
#include <vector>

namespace aexcompat::worker_render_report {

ReportSnapshot::ReportSnapshot(const std::ios& formatting_source) {
  stream_.copyfmt(formatting_source);
}

// Legacy callers do not install conformance render settings and already
// preflight their input worlds as premultiplied. Keep that contract unless an
// explicit --conformance-render-settings-v1 trailer overrides it.
std::string g_conformance_premultiplication = "premultiplied";
std::string g_conformance_renderer = "software";
bool g_conformance_render_settings_seen = false;

bool narrow_ascii_checked(const std::wstring& text, std::string& output) {
  output.clear();
  output.reserve(text.size());
  for (wchar_t character : text) {
    if (character < L' ' || character > L'~') return false;
    output.push_back(static_cast<char>(character));
  }
  return true;
}

bool parse_conformance_render_settings(const wchar_t* encoded) {
  if (!encoded) return false;
  std::vector<std::wstring> fields;
  std::wstring value(encoded);
  std::size_t start = 0;
  while (start <= value.size()) {
    const std::size_t end = value.find(L'|', start);
    fields.push_back(value.substr(start, end == std::wstring::npos ? end : end - start));
    if (end == std::wstring::npos) break;
    start = end + 1;
  }
  if (fields.size() != 6 || fields[0] != L"v1" || fields[2] != L"0" ||
      fields[3] != L"-" || fields[4] != L"0" ||
      (fields[1] != L"straight" && fields[1] != L"premultiplied" &&
       fields[1] != L"opaque") ||
      (fields[5] != L"AEXCompat CPU" && fields[5] != L"software")) return false;
  std::string premultiplication;
  std::string renderer;
  if (!narrow_ascii_checked(fields[1], premultiplication) ||
      !narrow_ascii_checked(fields[5], renderer)) return false;
  g_conformance_premultiplication = std::move(premultiplication);
  g_conformance_renderer = std::move(renderer);
  g_conformance_render_settings_seen = true;
  return true;
}

std::string conformance_render_settings_report_json() {
  if (!g_conformance_render_settings_seen) return ",\"render_settings\":null";
  return ",\"render_settings\":{\"premultiplication\":\"" +
      g_conformance_premultiplication +
      "\",\"color_management\":{\"enabled\":false,\"working_space\":null},\"linear_light\":false,\"renderer\":\"" +
      g_conformance_renderer + "\"}";
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
      << ",\"module_audit\":" << value.module_audit_json
      << conformance_render_settings_report_json() << "}\n";
}

void emit_classic_complete(ReportSnapshot& output, const ClassicEmission& value) {
  begin_classic(output, value.report.head);
  append_audio(output, value.report.audio);
  output.stream() << ",\"render_selector_dispatched\":"
      << (value.selector_dispatched ? "true" : "false")
      << ",\"depth_supported\":" << (value.depth_supported ? "true" : "false")
      << ",\"render_error\":" << value.render_error;
  append_classic_sequence(output, value.report.sequence);
  append_classic_frame(output, value.report.frame);
  append_custom_ui(output, value.custom_ui);
  append_classic_subsystems(output, value.subsystems);
  append_gpu_diagnostics(output, value.gpu);
  append_seh_diagnostics(output, value.seh);
  append_classic_callbacks(output, value.report.callbacks);
  append_classic_threads(output, value.report.threads);
  append_classic_context(output, value.report.context);
  finish_requested_parameters(output, value.requested);
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

void append_audio(ReportSnapshot& report, const ClassicReport::Audio& value) {
  report.stream()
      << ",\"audio_usage_advertised\":" << (value.usage_advertised ? "true" : "false")
      << ",\"audio_checkout_allowed\":" << (value.checkout_allowed ? "true" : "false")
      << ",\"audio_source_available\":" << (value.source_available ? "true" : "false")
      << ",\"unadvertised_audio_checkout_calls\":" << value.unadvertised_checkout_calls
      << ",\"rejected_unadvertised_audio_checkouts\":" << value.rejected_unadvertised_checkouts
      << ",\"rejected_audio_format_requests\":" << value.rejected_format_requests
      << ",\"audio_handle_exhaustions\":" << value.handle_exhaustions
      << ",\"peak_live_audio_handles\":" << value.peak_live_handles
      << ",\"last_audio_checkout_index\":" << value.last_checkout_index
      << ",\"audio_checkout_calls\":" << value.checkout_calls
      << ",\"audio_checkin_calls\":" << value.checkin_calls
      << ",\"automatic_audio_checkins\":" << value.automatic_checkins
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
      << (value.guard_bytes_intact ? "true" : "false")
      << aexcompat::callback_diagnostics::report_field_json() << value.world_debug_json;
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

void append_classic_callbacks(ReportSnapshot& report, const ClassicReport::Callbacks& value) {
  report.stream()
      << ",\"param_checkouts_balanced\":" << (value.param_checkouts_balanced ? "true" : "false")
      << ",\"param_checkout_calls\":" << value.param[0]
      << ",\"param_checkin_calls\":" << value.param[1]
      << ",\"automatic_param_checkins\":" << value.param[2]
      << ",\"invalid_param_checkins\":" << value.param[3]
      << ",\"last_param_checkout_index\":" << value.param[4]
      << ",\"last_param_checkout_time\":" << value.param[5]
      << ",\"last_param_checkout_time_step\":" << value.param[6]
      << ",\"last_param_checkout_time_scale\":" << value.param[7]
      << ",\"options_button_name\":\"" << value.escaped_options_button_name << '"'
      << ",\"options_button_name_calls\":" << value.host[0]
      << ",\"channel_count_queries\":" << value.host[1]
      << ",\"transform_world_calls\":" << value.host[2]
      << ",\"last_transform_x\":" << value.host[3]
      << ",\"last_transform_y\":" << value.host[4]
      << ",\"last_transform_opacity\":" << value.host[5]
      << ",\"abort_calls\":" << value.host[6]
      << ",\"progress_calls\":" << value.host[7]
      << ",\"register_ui_calls\":" << value.host[8]
      << ",\"last_progress_current\":" << value.host[9]
      << ",\"last_progress_total\":" << value.host[10];
}

void append_classic_context(ReportSnapshot& report, const ClassicReport::Context& value) {
  report.stream()
      << ",\"request_mode\":" << (value.request_mode ? "true" : "false")
      << ",\"downsample_x\":[" << value.downsample_x[0] << ',' << value.downsample_x[1] << ']'
      << ",\"downsample_y\":[" << value.downsample_y[0] << ',' << value.downsample_y[1] << ']'
      << ",\"pixel_aspect_ratio\":[" << value.pixel_aspect_ratio[0] << ',' << value.pixel_aspect_ratio[1] << ']'
      << ",\"full_resolution_dimensions\":[" << value.full_resolution_dimensions[0] << ','
      << value.full_resolution_dimensions[1] << ']'
      << ",\"quality\":" << value.scalar_metadata[0]
      << ",\"in_data_num_params\":" << value.scalar_metadata[1]
      << ",\"local_time_step\":" << value.scalar_metadata[2]
      << ",\"field\":" << value.scalar_metadata[3]
      << ",\"shutter_angle_fixed\":" << value.scalar_metadata[4]
      << ",\"shutter_phase_fixed\":" << value.scalar_metadata[5]
      << ",\"in_data_dimensions\":[" << value.input_dimensions[0] << ',' << value.input_dimensions[1] << ']'
      << ",\"pre_effect_source_origin\":[" << value.pre_effect_source_origin[0] << ','
      << value.pre_effect_source_origin[1] << ']'
      << ",\"output_origin\":[" << value.output_origin[0] << ',' << value.output_origin[1] << ']';
}

void begin_smart(ReportSnapshot& report, const SmartReport::Head& v) {
  report.stream()
      << "{\"schema_version\":1,\"stage\":\"smartfx_render\",\"status\":\""
      << (v.completed ? "render_completed" : "render_failed")
      << "\",\"global_setup_error\":" << v.setup_flags[0]
      << ",\"params_setup_error\":" << v.setup_flags[1]
      << ",\"advertised_out_flags\":" << v.setup_flags[2]
      << ",\"advertised_out_flags2\":" << v.setup_flags[3]
      << ",\"image_render_supported\":" << (v.advertised[0] ? "true" : "false")
      << ",\"smart_render_supported\":" << (v.advertised[1] ? "true" : "false")
      << ",\"nop_render_advertised\":" << (v.advertised[2] ? "true" : "false")
      << ",\"input_write_advertised\":" << (v.advertised[3] ? "true" : "false")
      << ",\"input_buffer_writable\":" << (v.advertised[3] ? "true" : "false")
      << ",\"wide_time_checkout_allowed\":" << (v.runtime_flags[0] ? "true" : "false");
  append_temporal_checkout_counters(report.stream(), v.temporal_checkout_counters);
  report.stream()
      << ",\"shutter_dependency_advertised\":" << (v.runtime_flags[1] ? "true" : "false")
      << ",\"smart_pre_render_dispatched\":" << (v.runtime_flags[2] ? "true" : "false")
      << ",\"smart_render_selector_dispatched\":" << (v.runtime_flags[3] ? "true" : "false")
      << ",\"comp_bg_color_success_count\":" << v.host_context[0]
      << ",\"comp_bg_color_rejection_count\":" << v.host_context[1]
      << ",\"guid_mix_in_call_count\":" << v.host_context[2]
      << ",\"guid_mix_in_success_count\":" << v.host_context[3]
      << ",\"guid_mix_in_rejection_count\":" << v.host_context[4]
      << ",\"guid_mix_in_last_size\":" << v.host_context[5]
      << ",\"guid_mix_in_max_size\":" << v.host_context[6]
      << ",\"guid_mix_in_size_limit\":" << v.host_context[7]
      << ",\"guid_mix_in_last_result\":" << v.host_context[8]
      << ",\"depth_supported\":" << (v.depth_supported ? "true" : "false")
      << ",\"pre_render_error\":" << v.selector_errors[0]
      << ",\"smart_render_error\":" << v.selector_errors[1]
      << ",\"smart_render_selector_error\":" << v.selector_errors[2]
      << ",\"gpu_device_setup_error\":" << v.selector_errors[3]
      << ",\"gpu_device_setdown_error\":" << v.selector_errors[4]
      << ",\"gpu_device_setdown_exception_code\":" << v.selector_errors[5]
      << ",\"gpu_render_possible\":" << (v.gpu_flags[0] ? "true" : "false")
      << ",\"gpu_render_dispatched\":" << (v.gpu_flags[1] ? "true" : "false")
      << ",\"checkout_time\":" << v.checkout_time[0]
      << ",\"checkout_time_step\":" << v.checkout_time[1]
      << ",\"checkout_time_scale\":" << v.checkout_time[2]
      << ",\"roi_contract_valid\":" << (v.roi_contract_valid ? "true" : "false")
      << ",\"input_checkout_request\":[" << v.input_checkout[0] << ',' << v.input_checkout[1]
      << ',' << v.input_checkout[2] << ',' << v.input_checkout[3] << ']'
      << ",\"map_checkout_request\":[" << v.map_checkout[0] << ',' << v.map_checkout[1]
      << ',' << v.map_checkout[2] << ',' << v.map_checkout[3] << ']'
      << ",\"input_checkout_result_rect\":[" << v.input_checkout_result[0] << ','
      << v.input_checkout_result[1] << ',' << v.input_checkout_result[2] << ','
      << v.input_checkout_result[3] << ']'
      << ",\"map_checkout_result_rect\":[" << v.map_checkout_result[0] << ','
      << v.map_checkout_result[1] << ',' << v.map_checkout_result[2] << ','
      << v.map_checkout_result[3] << ']'
      << ",\"malformed_checkout_request_count\":" << v.malformed_checkout_requests
      << ",\"empty_checkout_pixel_denial_count\":" << v.empty_checkout_pixel_denials
      << ",\"empty_layer_param_checkout_count\":" << v.empty_layer_param_checkouts
      << ",\"empty_layer_param_pixel_checkout_count\":"
      << v.empty_layer_param_pixel_checkouts
      << ",\"returns_extra_pixels\":" << (v.returns_extra_pixels ? "true" : "false")
      << ",\"result_within_request\":" << (v.result_within_request ? "true" : "false")
      << ",\"extra_pixels_contract_violation\":"
      << (v.extra_pixels_contract_violation ? "true" : "false")
      << ",\"empty_result_rect\":" << (v.empty_result_rect ? "true" : "false")
      << ",\"global_setdown_error\":" << v.global_setdown_error
      << ",\"case_id\":\"" << v.case_id << "\",\"pixel_format\":\"" << v.pixel_format
      << "\",\"width\":" << v.dimensions[0] << ",\"height\":" << v.dimensions[1]
      << ",\"rowbytes\":" << v.dimensions[2]
      << ",\"premultiplication\":\"" << g_conformance_premultiplication << "\""
      << ",\"input_world\":{\"width\":" << v.input_world_dimensions[0]
      << ",\"height\":" << v.input_world_dimensions[1]
      << ",\"row_bytes\":" << v.input_world_dimensions[0] * v.pixel_bytes
      << ",\"pixel_format\":\"" << v.pixel_format
      << "\",\"premultiplication\":\"" << g_conformance_premultiplication
      << "\",\"extent_hint\":{\"left\":0,\"top\":0,\"right\":"
      << v.input_world_dimensions[0] << ",\"bottom\":" << v.input_world_dimensions[1]
      << "}}"
      << ",\"output_world\":{\"width\":" << v.dimensions[0]
      << ",\"height\":" << v.dimensions[1] << ",\"row_bytes\":" << v.dimensions[2]
      << ",\"pixel_format\":\"" << v.pixel_format
      << "\",\"premultiplication\":\"" << g_conformance_premultiplication
      << "\",\"extent_hint\":{\"left\":"
      << v.output_extent_hint[0] << ",\"top\":" << v.output_extent_hint[1]
      << ",\"right\":" << v.output_extent_hint[2] << ",\"bottom\":"
      << v.output_extent_hint[3] << "}}"
      << ",\"bytes_written_per_row\":" << v.dimensions[2]
      << ",\"undefined_tail_bytes_per_row\":0"
      << ",\"input_sha256\":\"" << v.input_sha256 << "\",\"output_sha256\":\""
      << v.output_sha256 << "\",\"result_rects_valid\":"
      << (v.result_rects_valid ? "true" : "false")
      << aexcompat::callback_diagnostics::report_field_json() << v.world_debug_json;
}

void append_smart_context(ReportSnapshot& report, const SmartReport::Context& v) {
  report.stream()
      << ",\"result_rect\":[" << v.result_rect[0] << ',' << v.result_rect[1] << ','
      << v.result_rect[2] << ',' << v.result_rect[3] << ']'
      << ",\"max_result_rect\":[" << v.max_result_rect[0] << ',' << v.max_result_rect[1]
      << ',' << v.max_result_rect[2] << ',' << v.max_result_rect[3] << ']'
      << ",\"guard_bytes_intact\":" << (v.validity[0] ? "true" : "false")
      << ",\"output_pixels_valid\":" << (v.validity[1] ? "true" : "false")
      << ",\"param_checkouts_balanced\":" << (v.validity[2] ? "true" : "false")
      << ",\"param_checkout_calls\":" << v.parameter_checkouts[0]
      << ",\"param_checkin_calls\":" << v.parameter_checkouts[1]
      << ",\"automatic_param_checkins\":" << v.parameter_checkouts[2]
      << ",\"invalid_param_checkins\":" << v.parameter_checkouts[3]
      << ",\"request_mode\":" << (v.request_mode ? "true" : "false")
      << ",\"downsample_x\":[" << v.downsample_x[0] << ',' << v.downsample_x[1] << ']'
      << ",\"downsample_y\":[" << v.downsample_y[0] << ',' << v.downsample_y[1] << ']'
      << ",\"pixel_aspect_ratio\":[" << v.pixel_aspect_ratio[0] << ',' << v.pixel_aspect_ratio[1] << ']'
      << ",\"full_resolution_dimensions\":[" << v.full_resolution_dimensions[0] << ','
      << v.full_resolution_dimensions[1] << ']'
      << ",\"quality\":" << v.scalar_metadata[0]
      << ",\"in_data_num_params\":" << v.scalar_metadata[1]
      << ",\"local_time_step\":" << v.scalar_metadata[2]
      << ",\"field\":" << v.scalar_metadata[3]
      << ",\"shutter_angle_fixed\":" << v.scalar_metadata[4]
      << ",\"shutter_phase_fixed\":" << v.scalar_metadata[5]
      << ",\"in_data_dimensions\":[" << v.input_dimensions[0] << ',' << v.input_dimensions[1] << ']'
      << ",\"pre_effect_source_origin\":[" << v.pre_effect_source_origin[0] << ','
      << v.pre_effect_source_origin[1] << ']'
      << ",\"output_origin\":[" << v.output_origin[0] << ',' << v.output_origin[1] << ']';
}

void append_smart_lifetimes(ReportSnapshot& report, const SmartReport::Lifetimes& v) {
  report.stream()
      << ",\"mask_scene_id\":\"" << v.mask_scene_id << '"'
      << ",\"mask_count\":" << v.mask_geometry[0]
      << ",\"mask_open_count\":" << v.mask_geometry[1]
      << ",\"mask_tangent_vertex_count\":" << v.mask_geometry[2]
      << ",\"mask_lifetimes_balanced\":" << (v.mask_balanced ? "true" : "false")
      << ",\"mask_handles_acquired\":" << v.mask_handles[0]
      << ",\"mask_handles_disposed\":" << v.mask_handles[1]
      << ",\"stream_handles_acquired\":" << v.mask_handles[2]
      << ",\"stream_handles_disposed\":" << v.mask_handles[3]
      << ",\"stream_values_acquired\":" << v.mask_handles[4]
      << ",\"stream_values_disposed\":" << v.mask_handles[5]
      << ",\"lifetime_fault_observed\":" << (v.lifetime_fault ? "true" : "false")
      << ",\"suite_leases_balanced\":" << (v.suites_balanced ? "true" : "false")
      << ",\"suite_lease_warning\":" << (!v.suites_balanced ? "true" : "false")
      << ",\"suite_acquires\":" << v.suites[0] << ",\"suite_releases\":" << v.suites[1]
      << v.missing_suites_json
      << ",\"live_suite_lease_count\":" << v.suites[2]
      << ",\"live_suite_reference_count\":" << v.suites[3]
      << ",\"live_suite_leases\":\"" << v.live_suite_leases << '"'
      << ",\"suite_fault_observed\":" << (v.suite_fault ? "true" : "false")
      << ",\"handle_lifetimes_balanced\":" << (v.handles_balanced ? "true" : "false")
      << ",\"handles_created\":" << v.handles[0] << ",\"handles_disposed\":" << v.handles[1]
      << ",\"arbitrary_copy_calls\":" << v.arbitrary[0]
      << ",\"arbitrary_dispose_calls\":" << v.arbitrary[1]
      << ",\"arbitrary_print_calls\":" << v.arbitrary[2]
      << ",\"arbitrary_print_failures\":" << v.arbitrary[3]
      << ",\"arbitrary_roundtrip_calls\":" << v.arbitrary[4]
      << ",\"arbitrary_roundtrip_failures\":" << v.arbitrary[5]
      << ",\"arbitrary_scan_calls\":" << v.arbitrary[6]
      << ",\"arbitrary_scan_failures\":" << v.arbitrary[7]
      << ",\"arbitrary_compare_disagreements\":" << v.arbitrary[8]
      << ",\"arbitrary_new_calls\":" << v.arbitrary[9]
      << ",\"arbitrary_interpolation_calls\":" << v.arbitrary[10]
      << ",\"arbitrary_interpolation_failures\":" << v.arbitrary[11]
      << ",\"arbitrary_interpolation_amount\":" << v.arbitrary_interpolation_amount
      << ",\"invalid_arbitrary_operations\":" << v.arbitrary[12]
      << ",\"automatic_pre_render_handle_disposals\":" << v.handle_details[0]
      << ",\"handle_locks\":" << v.handle_details[1]
      << ",\"handle_unlocks\":" << v.handle_details[2]
      << ",\"live_handle_count\":" << v.handle_details[3]
      << ",\"live_handle_bytes\":" << v.handle_details[4]
      << ",\"invalid_handle_operations\":" << v.handle_details[5]
      << ",\"handle_fault_observed\":" << (v.handle_fault ? "true" : "false")
      << ",\"world_fault_observed\":" << (v.world_fault ? "true" : "false")
      << ",\"world_lifetimes_balanced\":" << (v.worlds_balanced ? "true" : "false")
      << ",\"worlds_created\":" << v.worlds[0] << ",\"worlds_disposed\":" << v.worlds[1]
      << ",\"live_world_count\":" << v.worlds[2] << ",\"live_world_bytes\":" << v.worlds[3]
      << ",\"invalid_world_operations\":" << v.worlds[4]
      << ",\"gpu_memory_lifetimes_balanced\":" << (v.gpu_balanced ? "true" : "false")
      << ",\"gpu_allocations_created\":" << v.gpu_allocations[0]
      << ",\"gpu_allocations_freed\":" << v.gpu_allocations[1]
      << ",\"live_gpu_allocation_count\":" << v.gpu_allocations[2]
      << ",\"live_gpu_memory_bytes\":" << v.gpu_allocations[3]
      << ",\"gpu_exclusive_access_depth\":" << v.gpu_allocations[4]
      << ",\"invalid_gpu_memory_operations\":" << v.gpu_allocations[5];
}

void append_smart_faults(ReportSnapshot& report, const SmartReport::Faults& v) {
  report.stream()
      << ",\"cuda_context_used\":" << (v.cuda[0] > 0 ? "true" : "false")
      << ",\"cuda_upload_bytes\":" << v.cuda[0] << ",\"cuda_download_bytes\":" << v.cuda[1]
      << ",\"cuda_sync_failures\":" << v.cuda[2] << ",\"cuda_device_count\":" << v.cuda[3]
      << ",\"cuda_device_index\":" << v.cuda[4]
      << ",\"opencl_context_used\":" << (v.opencl[0] > 0 ? "true" : "false")
      << ",\"opencl_upload_bytes\":" << v.opencl[0] << ",\"opencl_download_bytes\":" << v.opencl[1]
      << ",\"opencl_sync_failures\":" << v.opencl[2] << ",\"opencl_device_count\":" << v.opencl[3]
      << ",\"opencl_device_index\":" << v.opencl[4]
      << ",\"directx_context_used\":" << (v.directx_context_used ? "true" : "false")
      << ",\"directx_device_count\":" << v.directx[0] << ",\"directx_device_index\":" << v.directx[1]
      << ",\"directx_upload_bytes\":" << v.directx[2] << ",\"directx_download_bytes\":" << v.directx[3]
      << ",\"directx_sync_failures\":" << v.directx[4]
      << ",\"pixel_format_fault_observed\":" << (v.pixel_format_fault ? "true" : "false")
      << ",\"pixel_format_add_calls\":" << v.pixel_format[0]
      << ",\"pixel_format_clear_calls\":" << v.pixel_format[1]
      << ",\"supported_pixel_format_count\":" << v.pixel_format[2]
      << ",\"invalid_pixel_format_operations\":" << v.pixel_format[3]
      << ",\"outline_fault_observed\":" << (v.faults[0] ? "true" : "false")
      << ",\"outline_mutations\":" << v.operations[0] << ",\"invalid_outline_operations\":" << v.operations[1]
      << ",\"mask_attribute_fault_observed\":" << (v.faults[1] ? "true" : "false")
      << ",\"mask_mutations\":" << v.operations[2] << ",\"invalid_mask_operations\":" << v.operations[3]
      << ",\"stream_metadata_fault_observed\":" << (v.faults[2] ? "true" : "false")
      << ",\"stream_metadata_queries\":" << v.operations[4] << ",\"stream_duplicates\":" << v.operations[5]
      << ",\"invalid_stream_operations\":" << v.operations[6]
      << ",\"keyframe_fault_observed\":" << (v.faults[3] ? "true" : "false")
      << ",\"keyframe_mutations\":" << v.operations[7] << ",\"invalid_keyframe_operations\":" << v.operations[8]
      << ",\"dynamic_stream_fault_observed\":" << (v.faults[4] ? "true" : "false")
      << ",\"dynamic_stream_queries\":" << v.operations[9]
      << ",\"dynamic_stream_mutations\":" << v.operations[10]
      << ",\"invalid_dynamic_stream_operations\":" << v.operations[11]
      << ",\"aegp_memory_fault_observed\":" << (v.faults[5] ? "true" : "false")
      << ",\"aegp_memory_created\":" << v.aegp_memory[0]
      << ",\"aegp_memory_freed\":" << v.aegp_memory[1]
      << ",\"live_aegp_memory_handles\":" << v.aegp_memory[2]
      << ",\"live_aegp_memory_bytes\":" << v.aegp_memory[3]
      << ",\"invalid_aegp_memory_operations\":" << v.aegp_memory[4];
}

void append_smart_session(ReportSnapshot& report, const SmartReport::Session& v) {
  report.stream()
      << ",\"session_mode\":true"
      << ",\"session_frames_attempted\":" << v.frames_attempted
      << ",\"session_sequence_setup_error\":" << v.sequence_setup_error
      << ",\"session_sequence_setdown_error\":" << v.sequence_setdown_error
      << ",\"session_render_error\":" << v.render_error
      << ",\"session_protocol_violation\":" << (v.protocol_violation ? "true" : "false")
      << ",\"session_invariant_failure\":" << (v.invariant_failure ? "true" : "false");
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
      << ",\"missing_dependency\":\"" << value.escaped_missing_dependency << '"'
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
      << ",\"suite_fault_observed\":" << (value.suite_fault ? "true" : "false")
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

// Suite registry accounting and the JSON escape helper stay owned by the
// worker entry; the diagnostics builders below read them through these
// cross-TU declarations.
namespace aexcompat::l2_detail {
std::string escape(const std::string&);
uint32_t suite_acquire_count();
uint32_t suite_release_count();
std::size_t live_suite_lease_count();
uint32_t live_suite_reference_count();
bool suite_leases_balanced();
std::string missing_suites_report_json();
std::string unsupported_suite_calls_report_json();
std::string suite_timeline_report_json();
std::string live_suite_lease_summary();
}  // namespace aexcompat::l2_detail

namespace aexcompat::worker_render_report {

namespace gpu_transport = aexcompat::gpu_runtime::memory_world_transport;
namespace opencl = aexcompat::gpu_runtime::opencl;
namespace directx_backend = aexcompat::gpu_runtime::directx_backend;

GpuDiagnosticsSnapshot capture_gpu_diagnostics() {
  const auto& directx = directx_backend::diagnostics();
  const auto i64 = [](auto value) { return static_cast<int64_t>(value); };
  return {
      gpu_transport::gpu_memory_lifetimes_balanced(), gpu_transport::cuda_upload_bytes > 0,
      {i64(gpu_transport::cuda_upload_bytes), i64(gpu_transport::cuda_download_bytes),
       i64(gpu_transport::cuda_sync_failures),
       i64(gpu_transport::last_cuda_device_count), i64(gpu_transport::last_cuda_device_index)},
      gpu_transport::opencl_upload_bytes > 0,
      {i64(gpu_transport::opencl_upload_bytes), i64(gpu_transport::opencl_download_bytes),
       i64(gpu_transport::opencl_sync_failures),
       i64(opencl::last_device_count()), i64(opencl::last_device_index())},
      directx.context_used,
      {i64(directx.device_count), i64(directx.device_index), i64(directx.upload_bytes),
       i64(directx.download_bytes), i64(directx.sync_failures)},
      {i64(gpu_transport::allocations_created), i64(gpu_transport::allocations_freed),
       i64(gpu_transport::live_allocation_count()), i64(gpu_transport::live_memory_bytes()),
       i64(gpu_transport::exclusive_access_depth()),
       i64(gpu_transport::invalid_memory_operations)}};
}

SehDiagnosticsSnapshot capture_seh_diagnostics() {
  auto& telemetry = worker_runtime::selector_dispatch_telemetry();
  return {telemetry.seh_code, telemetry.seh_address,
          l2_detail::escape(telemetry.seh_module), l2_detail::escape(telemetry.selector),
          l2_detail::escape(telemetry.missing_dependency),
          telemetry.error};
}

ClassicSubsystemDiagnostics capture_classic_subsystems() {
  const auto i64 = [](auto value) { return static_cast<int64_t>(value); };
  const auto& handle_stats = worker_runtime::handles::statistics();
  const auto& world_stats = aexcompat::world_registry::statistics();
  const auto& receipt_stats = aexcompat::render_receipts::statistics();
  const auto path = aexcompat::pf_path_runtime::snapshot();
  const auto& arbitrary = worker_runtime::parameters::state().arbitrary;
  return {
      l2_detail::suite_leases_balanced(),
      {i64(l2_detail::suite_acquire_count()), i64(l2_detail::suite_release_count()),
       i64(l2_detail::live_suite_lease_count()),
       i64(l2_detail::live_suite_reference_count())},
      l2_detail::missing_suites_report_json() +
          l2_detail::unsupported_suite_calls_report_json() +
          l2_detail::suite_timeline_report_json() +
          worker_runtime::suite_call_slot_probe::report_json(),
      l2_detail::live_suite_lease_summary(),
      // #1182 (owner-directed): a rejected suite release - the plug-in releasing
      // a suite it never acquired (Basic_Text/Path_Text/Numbers release "PF Param
      // Utils Suite" v3 without acquiring it) - is contained by the registry as a
      // no-op that returns rejection and touches no host state, so it cannot
      // corrupt anything. It is recorded in the suite_timeline (result != 0) as a
      // reproducible diagnostic and counted below, but it is a benign warning, not
      // a session-failing fault. Handle and world double-dispose faults are
      // separate fields and stay fail-closed.
      false,
      worker_runtime::handles::handle_lifetimes_balanced(),
      aexcompat::pf_path_runtime::lifetimes_balanced(),
      {i64(path.checkout_calls), i64(path.checkin_calls), i64(path.mask_calls),
       i64(path.preps_created), i64(path.preps_disposed),
       i64(path.invalid_operations), i64(path.reject_reason), i64(path.live_preps)},
      {path.last_feather_x, path.last_feather_y}, path.last_opacity,
      i64(path.last_quality),
      {i64(path.last_bounds[0]), i64(path.last_bounds[1]),
       i64(path.last_bounds[2]), i64(path.last_bounds[3])},
      {i64(handle_stats.created), i64(handle_stats.disposed)},
      {i64(arbitrary.copy_calls), i64(arbitrary.dispose_calls), i64(arbitrary.print_calls),
       i64(arbitrary.print_failures), i64(arbitrary.roundtrip_calls),
       i64(arbitrary.roundtrip_failures), i64(arbitrary.scan_calls),
       i64(arbitrary.scan_failures), i64(arbitrary.compare_disagreements),
       i64(arbitrary.new_calls), i64(arbitrary.interpolation_calls),
       i64(arbitrary.interpolation_failures), i64(arbitrary.invalid_operations), 0, 0},
      arbitrary.last_interpolation_amount,
      aexcompat::world_registry::lifetimes_balanced(),
      {i64(world_stats.created), i64(world_stats.disposed)},
      aexcompat::render_receipts::lifetimes_balanced(),
      {i64(receipt_stats.created), i64(receipt_stats.checked_in), i64(receipt_stats.live_count),
       i64(receipt_stats.live_bytes), i64(receipt_stats.invalid_operations)},
      aexcompat::aegp_async_layer::balanced(),
      {i64(aexcompat::aegp_async_layer::diagnostics().created),
       i64(aexcompat::aegp_async_layer::diagnostics().completed),
       i64(aexcompat::aegp_async_layer::diagnostics().canceled),
       i64(aexcompat::aegp_async_layer::diagnostics().callback_failures),
       i64(aexcompat::aegp_async_layer::diagnostics().callback_exceptions),
       i64(aexcompat::aegp_async_layer::diagnostics().live),
       i64(aexcompat::aegp_async_layer::diagnostics().reserved_bytes)}};
}

}  // namespace aexcompat::worker_render_report
