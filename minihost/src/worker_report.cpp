#include "worker_report.hpp"

#include <algorithm>
#include <cmath>
#include <iomanip>
#include <sstream>

namespace aexcompat::worker_report {
namespace {
void boolean(std::ostringstream& o, bool value) { o << (value ? "true" : "false"); }
void number(std::ostringstream& o, double value) {
  if (std::isfinite(value)) o << std::setprecision(17) << value;
  else o << "null";
}
void color(std::ostringstream& o, const std::array<unsigned char, 4>& v) {
  o << "{\"alpha\":" << static_cast<unsigned>(v[0]) << ",\"red\":"
    << static_cast<unsigned>(v[1]) << ",\"green\":" << static_cast<unsigned>(v[2])
    << ",\"blue\":" << static_cast<unsigned>(v[3]) << '}';
}
void fingerprint(std::ostringstream& o, uint64_t value) {
  const auto flags = o.flags();
  const auto fill = o.fill();
  o << '"' << std::hex << std::setw(16) << std::setfill('0') << value << '"';
  o.flags(flags);
  o.fill(fill);
}
}

std::string bounded_diagnostic_text(const std::string& value, std::size_t cap) {
  std::string result;
  result.reserve(std::min(value.size(), cap));
  for (unsigned char ch : value) {
    if (result.size() == cap) break;
    if (ch == '"' || ch == '\\') result.push_back('\\');
    if (ch >= 0x20 && ch < 0x7f) result.push_back(static_cast<char>(ch));
  }
  return result;
}

std::string serialize_l2_report(const L2ReportContext& c) {
  std::ostringstream o;
  o << "{\"schema_version\":1,\"stage\":\"L2\",\"status\":\"" << bounded_diagnostic_text(c.status, 96)
    << "\",\"global_setup_error\":" << c.global_error << ",\"params_setup_error\":" << c.params_error
    << ",\"global_setdown_error\":" << c.setdown_error << ",\"reported_num_params\":" << c.reported_num_params
    << ",\"register_ui_calls\":" << c.register_ui_calls << ",\"invalid_custom_ui_registrations\":" << c.invalid_custom_ui_registrations
    << ",\"custom_ui\":{\"events\":" << c.custom_ui_events << ",\"comp_size\":[" << c.custom_ui_comp_size[0] << ',' << c.custom_ui_comp_size[1]
    << "],\"layer_size\":[" << c.custom_ui_layer_size[0] << ',' << c.custom_ui_layer_size[1] << "],\"preview_size\":[" << c.custom_ui_preview_size[0] << ',' << c.custom_ui_preview_size[1] << ']'
    << "},\"out_flags\":" << c.out_flags << ",\"out_flags2\":" << c.out_flags2
    << ",\"update_params_ui_advertised\":"; boolean(o, c.update_params_ui_advertised);
  o << ",\"query_dynamic_flags_advertised\":"; boolean(o, c.query_dynamic_flags_advertised);
  o << ",\"conditional_ui_selectors_dispatched\":"; boolean(o, c.conditional_ui_selectors_dispatched);
  o << ",\"update_params_ui_error\":" << c.update_params_ui_error << ",\"query_dynamic_flags_error\":" << c.query_dynamic_flags_error
    << ",\"update_param_ui_calls\":" << c.update_param_ui_calls << ",\"pf_get_current_state_calls\":" << c.pf_get_current_state_calls
    << ",\"pf_are_states_identical_calls\":" << c.pf_are_states_identical_calls << ",\"suite_leases_balanced\":"; boolean(o, c.suite_leases_balanced);
  o << ",\"utility_undo_groups\":{\"starts\":" << c.utility_undo_group_starts
    << ",\"ends\":" << c.utility_undo_group_ends
    << ",\"invalid_operations\":" << c.utility_undo_group_invalid_operations
    << ",\"depth\":" << c.utility_undo_group_depth << ",\"balanced\":";
  boolean(o, c.utility_undo_groups_balanced);
  o << ",\"operations_valid\":";
  boolean(o, c.utility_undo_group_operations_valid);
  o << "},\"mask_scene\":{\"observed\":";
  boolean(o, c.mask_scene_observed);
  o << ",\"id\":\"" << bounded_diagnostic_text(c.mask_scene_id, 96)
    << "\",\"fingerprint_before\":";
  fingerprint(o, c.mask_scene_fingerprint_before);
  o << ",\"fingerprint_after\":";
  fingerprint(o, c.mask_scene_fingerprint_after);
  o << ",\"changed\":";
  boolean(o, c.mask_scene_changed);
  o << ",\"statistics\":{\"active_masks\":" << c.mask_scene_active_masks
    << ",\"mask_mutations\":" << c.mask_scene_mask_mutations
    << ",\"invalid_mask_operations\":" << c.mask_scene_invalid_mask_operations
    << ",\"outline_mutations\":" << c.mask_scene_outline_mutations
    << ",\"invalid_outline_operations\":" << c.mask_scene_invalid_outline_operations
    << ",\"keyframe_mutations\":" << c.mask_scene_keyframe_mutations
    << ",\"invalid_keyframe_operations\":" << c.mask_scene_invalid_keyframe_operations
    << ",\"dynamic_stream_mutations\":" << c.mask_scene_dynamic_stream_mutations
    << ",\"invalid_dynamic_stream_operations\":" << c.mask_scene_invalid_dynamic_stream_operations
    << "},\"curves\":[";
  for (std::size_t curve_index = 0;
       curve_index < c.mask_scene_curves.size(); ++curve_index) {
    if (curve_index) o << ',';
    const auto& curve = c.mask_scene_curves[curve_index];
    o << "{\"id\":" << curve.id << ",\"open\":";
    boolean(o, curve.open);
    o << ",\"vertices\":[";
    for (std::size_t vertex_index = 0;
         vertex_index < curve.vertices.size(); ++vertex_index) {
      if (vertex_index) o << ',';
      const auto& vertex = curve.vertices[vertex_index];
      o << "{\"x\":"; number(o, vertex.x);
      o << ",\"y\":"; number(o, vertex.y);
      o << ",\"tangent_in_x\":"; number(o, vertex.tangent_in_x);
      o << ",\"tangent_in_y\":"; number(o, vertex.tangent_in_y);
      o << ",\"tangent_out_x\":"; number(o, vertex.tangent_out_x);
      o << ",\"tangent_out_y\":"; number(o, vertex.tangent_out_y);
      o << '}';
    }
    o << "]}";
  }
  o << "]}";
  o << c.unsupported_suite_calls_json << c.suite_call_slot_probe_json
    << c.compute_cache_timeline_json
    << c.selector_invocations_json << c.plugin_data_json;
  if (!c.module_audit_failure_json.empty())
    o << ",\"module_audit_failure\":" << c.module_audit_failure_json;
  o << c.missing_suites_json << c.suite_timeline_json
    << ",\"user_changed_param_requested\":"; boolean(o, c.user_changed_param_requested);
  o << ",\"user_changed_param_forced\":"; boolean(o, c.user_changed_param_forced);
  o << ",\"user_changed_param_slot\":" << c.user_changed_param_slot << ",\"user_changed_param_error\":" << c.user_changed_param_error
    << ",\"user_changed_parameters\":" << c.user_changed_parameters_json << ",\"return_message\":\"" << bounded_diagnostic_text(c.return_message, 256)
    << "\",\"about_message\":\"" << bounded_diagnostic_text(c.about_message, 256) << "\",\"about_selector_dispatched\":"; boolean(o, c.about_selector_dispatched);
  o << ",\"last_seh_selector\":\"" << bounded_diagnostic_text(c.last_seh_selector, 96) << "\",\"last_seh_error\":" << c.last_seh_error
    << ",\"last_seh_exception_code\":" << c.last_seh_exception_code
    << ",\"sequence_setup_error\":" << c.lifecycle_errors[0] << ",\"sequence_resetup_error\":" << c.lifecycle_errors[1]
    << ",\"frame_setup_error\":" << c.lifecycle_errors[2] << ",\"frame_setdown_error\":" << c.lifecycle_errors[3]
    << ",\"sequence_setdown_error\":" << c.lifecycle_errors[4] << ",\"lifecycle_data_null\":"; boolean(o, c.lifecycle_data_null);
  o << ",\"parameters\":[";
  for (std::size_t n = 0; n < c.parameters.size(); ++n) {
    if (n) o << ',';
    const auto& p = c.parameters[n];
    o << "{\"index\":" << p.index << ",\"disk_id\":" << p.disk_id << ",\"type\":" << p.type
      << ",\"ui_flags\":" << p.ui_flags << ",\"ui_width\":" << p.ui_width << ",\"ui_height\":" << p.ui_height
      << ",\"flags\":" << p.flags << ",\"name\":\"" << bounded_diagnostic_text(p.name, 32) << '"';
    if (p.has_numeric) {
      o << ",\"valid_min\":"; number(o, p.valid_min);
      o << ",\"valid_max\":"; number(o, p.valid_max);
      o << ",\"slider_min\":"; number(o, p.slider_min);
      o << ",\"slider_max\":"; number(o, p.slider_max);
      o << ",\"default\":"; number(o, p.default_value);
    }
    if (p.precision >= 0) o << ",\"precision\":" << p.precision;
    if (p.has_current) {
      o << ",\"current\":"; number(o, p.current_value);
      o << ",\"current_default_mismatch\":";
      boolean(o, p.current_value != p.default_value);
    }
    if (p.has_color) { o << ",\"default_color\":"; color(o, p.default_color); o << ",\"current_color\":"; color(o, p.current_color); }
    if (p.component_count > 0) {
      o << ",\"default_components\":[";
      for (int i = 0; i < p.component_count; ++i) { if (i) o << ','; number(o, p.default_components[i]); }
      o << "],\"current_components\":[";
      for (int i = 0; i < p.component_count; ++i) { if (i) o << ','; number(o, p.current_components[i]); }
      o << ']';
    }
    if (!p.choices.empty()) o << ",\"choices\":\"" << bounded_diagnostic_text(p.choices, 4096) << '"';
    if (!p.label.empty()) o << ",\"label\":\"" << bounded_diagnostic_text(p.label, 4096) << '"';
    if (!p.arbitrary_summary.empty()) o << ",\"arbitrary_summary\":\"" << bounded_diagnostic_text(p.arbitrary_summary, 4096) << '"';
    if (p.type == 0) o << ",\"layer_default\":" << p.layer_default;
    o << '}';
  }
  std::string out = o.str() + "],\"selectors_executed\":true,\"render_performed\":false";
  // The discovery session embeds this report in its inspect_done message with
  // the module audit carried by the session's own epoch/final report instead
  // (closure-session design §4.2); an empty module_audit_json omits the key.
  if (!c.module_audit_json.empty()) out += ",\"module_audit\":" + c.module_audit_json;
  return out + "}\n";
}
}  // namespace aexcompat::worker_report
