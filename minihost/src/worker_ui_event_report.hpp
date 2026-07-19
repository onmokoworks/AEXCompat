#pragma once

#include <array>
#include <cstdint>
#include <string>

namespace aexcompat::l2_detail {

// Completion-report inputs captured from worker_main_impl's custom-UI event
// locals. Value-only boundary plus the requested-assignment view the JSON
// echoes back; Drawbot/App/drag telemetry is read by the owner TU through
// ui_event_execution::custom_ui_telemetry().
struct UiEventCompletionInputs {
  bool ui_lifecycle_mode{};
  bool ui_idle_mode{};
  bool ui_keydown_mode{};
  bool ui_mouse_exited_mode{};
  bool draw_event_mode{};
  bool drag_event_mode{};
  bool click_event_mode{};
  int32_t drag_steps{};
  uint32_t keydown_code{};
  uint32_t keydown_modifiers{};
  const char* event_target{};
  int32_t event_error{};
  int32_t cursor{};
  int32_t event_out_flags{};
  bool changed_value{};
  std::array<int32_t, 5> lifecycle_errors{};
  std::array<uintptr_t, 4> plugin_state_before_close{};
  bool lifecycle_context_stable{};
  bool lifecycle_host_state_cleared{};
  bool event_assignments_applied{};
  bool arbitrary_values_disposed{};
  bool defaults_disposed{};
  bool drawbot_objects_empty{};
  int32_t event_sequence_setdown_error{};
  int32_t event_setdown_error{};
  // Pre-serialized by worker_main_impl from the invocation's ui_event
  // assignments so the boundary stays value-only.
  std::string requested_parameters_json_text;
};

// Emits the custom_ui_event completion JSON and returns the pass verdict that
// worker_main_impl maps onto the process exit code.
bool emit_ui_event_completion_report(const UiEventCompletionInputs& inputs);

}  // namespace aexcompat::l2_detail
