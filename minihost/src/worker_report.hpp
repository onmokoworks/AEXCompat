#pragma once

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace aexcompat::worker_report {

// Value-only boundary: no host handles, absolute paths, pixels, or plugin bytes.
struct ParameterSnapshot {
  int32_t index{}, disk_id{}, type{};
  uint32_t ui_flags{};
  int16_t ui_width{}, ui_height{};
  uint32_t flags{};
  std::string name;
  bool has_numeric{};
  double valid_min{}, valid_max{}, slider_min{}, slider_max{}, default_value{};
  bool has_current{};
  double current_value{};
  bool has_color{};
  std::array<unsigned char, 4> default_color{}, current_color{};
  int32_t component_count{};
  std::array<double, 3> default_components{}, current_components{};
  int32_t precision{-1};
  std::string choices, label, arbitrary_summary;
  int32_t layer_default{};
};

struct L2ReportContext {
  std::string status;
  int32_t global_error{}, params_error{}, setdown_error{}, reported_num_params{};
  uint32_t register_ui_calls{}, invalid_custom_ui_registrations{}, custom_ui_events{};
  std::array<int32_t, 2> custom_ui_comp_size{}, custom_ui_layer_size{}, custom_ui_preview_size{};
  uint32_t out_flags{}, out_flags2{};
  bool update_params_ui_advertised{}, query_dynamic_flags_advertised{}, conditional_ui_selectors_dispatched{};
  int32_t update_params_ui_error{}, query_dynamic_flags_error{};
  uint32_t update_param_ui_calls{}, pf_get_current_state_calls{}, pf_are_states_identical_calls{};
  bool suite_leases_balanced{}, user_changed_param_requested{};
  int32_t user_changed_param_slot{}, user_changed_param_error{};
  std::string user_changed_parameters_json, unsupported_suite_calls_json;
  std::string suite_call_slot_probe_json;
  std::string compute_cache_timeline_json;
  std::string selector_invocations_json;
  std::string plugin_data_json;
  std::string missing_suites_json, suite_timeline_json;
  std::string module_audit_failure_json;
  std::string return_message, about_message;
  bool about_selector_dispatched{};
  std::string last_seh_selector;
  int32_t last_seh_error{};
  uint32_t last_seh_exception_code{};
  std::array<int32_t, 5> lifecycle_errors{};
  bool lifecycle_data_null{};
  std::vector<ParameterSnapshot> parameters;
  std::string module_audit_json;
};

// Preserves the existing ASCII-only escaping policy while enforcing a cap.
std::string bounded_diagnostic_text(const std::string& value, std::size_t cap);
std::string serialize_l2_report(const L2ReportContext& context);

}  // namespace aexcompat::worker_report
