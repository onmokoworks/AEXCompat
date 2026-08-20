#pragma once

#include "worker_parameter_execution.hpp"

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::ui_event_execution {

using parameter_execution::BufferIn;
using parameter_execution::BufferOut;
using parameter_execution::EffectEntry;
using parameters::RequestedAssignments;

struct Request {
  EffectEntry entry{};
  BufferIn* input{};
  BufferOut* output{};
  int32_t params_error{};
  bool parameter_count_contract_valid{};
  const RequestedAssignments* assignments{};
  bool assignment_mode{};
  bool adjust_cursor{};
  bool draw{};
  bool click{};
  bool drag{};
  bool lifecycle{};
  bool idle{};
  bool keydown{};
  bool mouse_exited{};
  int32_t click_x{};
  int32_t click_y{};
  int32_t drag_end_x{};
  int32_t drag_end_y{};
  int32_t drag_steps{};
  uint32_t keydown_code{};
  uint32_t keydown_modifiers{};
  int32_t window_type{};
  void* context_slot{};
  void* ui_context{};
  intptr_t* plugin_state{};
  void* transform_point{};
  void* transform_point_simple{};
};

struct Hooks {
  int32_t (*invoke_entry)(EffectEntry, int32_t, void*, void*, void**, void*,
                          void*, uint32_t*){};
  void* (*enter_ui_context)(int32_t){};
  void (*leave_ui_context)(void*){};
  bool (*context_stable)(){};
  void (*set_context_tool)(int32_t){};
};

struct Result {
  int32_t event_error{-1};
  int32_t cursor{};
  int32_t event_out_flags{};
  bool changed_value{};
  std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
  std::array<uintptr_t, 4> plugin_state_before_close{};
  bool lifecycle_context_stable{true};
  bool lifecycle_host_state_cleared{};
  bool event_assignments_applied{};
  bool drag_requested{};
  uint32_t drag_calls{};
  bool drag_terminated{};
};

bool dispatch(const Request&, const Hooks&, Result&);

// Custom-UI registration block captured from PF_REGISTER_UI.
struct CustomUiRegistration {
  uint32_t events{};
  int32_t comp_width{};
  int32_t comp_height{};
  int32_t comp_alignment{};
  int32_t layer_width{};
  int32_t layer_height{};
  int32_t layer_alignment{};
  int32_t preview_width{};
  int32_t preview_height{};
  int32_t preview_alignment{};
};

// Custom-UI / Drawbot / App suite telemetry (issue #126 Phase D): the
// plain-data counters, flags, and captured values recorded by worker_main's
// Drawbot, App, and custom-UI event callbacks, and read back by the classic
// and smart completion reports. The opaque Drawbot/App object tables and the
// HostUiContext stay in worker_main with the callback ABI that owns them.
// Lifetime: process-lifetime, zero/default-initialized, never torn down.
struct CustomUiTelemetry {
  uint32_t register_ui_calls{};
  CustomUiRegistration registration{};
  uint32_t invalid_custom_ui_registrations{};
  bool render_ui_context_active{};
  uint32_t drawbot_objects_created{};
  uint32_t drawbot_objects_released{};
  uint32_t drawbot_paint_rect_calls{};
  uint32_t drawbot_fill_path_calls{};
  uint32_t drawbot_stroke_path_calls{};
  uint32_t drawbot_invalid_operations{};
  uint32_t drawbot_get_supplier_calls{};
  uint32_t drawbot_get_surface_calls{};
  uint32_t drawbot_get_drawing_ref_calls{};
  uint32_t overlay_stroke_path_calls{};
  uint32_t app_get_background_color_calls{};
  uint32_t app_color_picker_calls{};
  uint32_t app_invalidate_rect_calls{};
  uint32_t app_progress_dialogs_created{};
  uint32_t app_progress_dialogs_disposed{};
  std::array<float, 4> app_picker_color{1.0f, 0.25f, 0.75f, 0.5f};
  std::array<int32_t, 4> app_invalidated_rect{};
  uint32_t ui_drag_calls{};
  bool ui_drag_requested{};
  bool ui_drag_terminated{};
  uint32_t ui_coordinate_transform_calls{};
  bool render_click_enabled{};
  bool render_draw_enabled{};
  int32_t render_click_x{};
  int32_t render_click_y{};
  int32_t render_click_error{-1};
  int32_t render_click_out_flags{};
  bool render_click_changed_value{};
  int32_t render_draw_error{-1};
  int32_t render_draw_out_flags{};
  std::array<int32_t, 4> render_ui_lifecycle_errors{-1, -1, -1, -1};
  bool render_ui_context_closed{};
  std::vector<std::array<float, 4>> drawbot_fill_colors;
};
CustomUiTelemetry& custom_ui_telemetry();

struct InfoTextTelemetrySnapshot {
  uint32_t calls{};
  std::string last_text;
};
void record_info_text(std::string text);
InfoTextTelemetrySnapshot snapshot_info_text_telemetry();
void reset_info_text_telemetry();

}  // namespace aexcompat::worker_runtime::ui_event_execution
