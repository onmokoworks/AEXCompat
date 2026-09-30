#include "worker_ui_event_report.hpp"

#include "worker_handle_runtime.hpp"
#include "worker_ui_event_execution.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <iostream>
#include <string>

namespace aexcompat::l2_detail {

using aexcompat::worker_runtime::handles::handle_lifetimes_balanced;

// Worker-entry owned diagnostic escaping and suite-lease accounting stay in
// l2_main with the registries that mutate them; the report reads them cross-TU.
std::string escape(const std::string&);
bool suite_leases_balanced();

// Custom-UI/Drawbot/App telemetry read back through its owner,
// aexcompat::worker_runtime::ui_event_execution::custom_ui_telemetry()
// (issue #126 Phase D); these references keep the g_* spellings.
namespace {
auto& g_custom_ui_telemetry =
    aexcompat::worker_runtime::ui_event_execution::custom_ui_telemetry();
auto& g_drawbot_objects_created = g_custom_ui_telemetry.drawbot_objects_created;
auto& g_drawbot_objects_released = g_custom_ui_telemetry.drawbot_objects_released;
auto& g_drawbot_paint_rect_calls = g_custom_ui_telemetry.drawbot_paint_rect_calls;
auto& g_drawbot_fill_path_calls = g_custom_ui_telemetry.drawbot_fill_path_calls;
auto& g_drawbot_stroke_path_calls = g_custom_ui_telemetry.drawbot_stroke_path_calls;
auto& g_drawbot_invalid_operations = g_custom_ui_telemetry.drawbot_invalid_operations;
auto& g_drawbot_get_supplier_calls = g_custom_ui_telemetry.drawbot_get_supplier_calls;
auto& g_drawbot_get_surface_calls = g_custom_ui_telemetry.drawbot_get_surface_calls;
auto& g_drawbot_get_drawing_ref_calls = g_custom_ui_telemetry.drawbot_get_drawing_ref_calls;
auto& g_drawbot_fill_colors = g_custom_ui_telemetry.drawbot_fill_colors;
auto& g_overlay_stroke_path_calls = g_custom_ui_telemetry.overlay_stroke_path_calls;
auto& g_app_get_background_color_calls = g_custom_ui_telemetry.app_get_background_color_calls;
auto& g_app_color_picker_calls = g_custom_ui_telemetry.app_color_picker_calls;
auto& g_app_invalidate_rect_calls = g_custom_ui_telemetry.app_invalidate_rect_calls;
auto& g_app_picker_color = g_custom_ui_telemetry.app_picker_color;
auto& g_app_invalidated_rect = g_custom_ui_telemetry.app_invalidated_rect;
auto& g_ui_drag_calls = g_custom_ui_telemetry.ui_drag_calls;
auto& g_ui_drag_requested = g_custom_ui_telemetry.ui_drag_requested;
auto& g_ui_drag_terminated = g_custom_ui_telemetry.ui_drag_terminated;
auto& g_ui_coordinate_transform_calls = g_custom_ui_telemetry.ui_coordinate_transform_calls;
}  // namespace

bool emit_ui_event_completion_report(const UiEventCompletionInputs& in) {
  const auto info_text =
      aexcompat::worker_runtime::ui_event_execution::snapshot_info_text_telemetry();
  const int32_t event_error = in.event_error;
  const int32_t cursor = in.cursor;
  const int32_t event_out_flags = in.event_out_flags;
  const bool changed_value = in.changed_value;
  const auto& lifecycle_errors = in.lifecycle_errors;
  const auto& plugin_state_before_close = in.plugin_state_before_close;
  const bool lifecycle_context_stable = in.lifecycle_context_stable;
  const bool lifecycle_host_state_cleared = in.lifecycle_host_state_cleared;
  const bool event_assignments_applied = in.event_assignments_applied;
  const bool arbitrary_values_disposed = in.arbitrary_values_disposed;
  const bool defaults_disposed = in.defaults_disposed;
  const int32_t event_sequence_setdown_error = in.event_sequence_setdown_error;
  const int32_t event_setdown_error = in.event_setdown_error;
  const char* event_target = in.event_target;
  const bool event_contract = (in.ui_lifecycle_mode || in.ui_idle_mode || in.ui_keydown_mode ||
      in.ui_mouse_exited_mode)
      ? std::all_of(lifecycle_errors.begin(),
            lifecycle_errors.begin() +
                ((in.ui_idle_mode || in.ui_keydown_mode || in.ui_mouse_exited_mode) ? 5 : 4),
            [](int32_t error) { return error == 0; }) &&
          lifecycle_context_stable && lifecycle_host_state_cleared
      : in.draw_event_mode
      ? event_error == 0 && (event_out_flags & 1) != 0 &&
          (g_drawbot_paint_rect_calls + g_drawbot_fill_path_calls +
           g_drawbot_stroke_path_calls + g_overlay_stroke_path_calls) > 0 &&
          g_drawbot_fill_colors.size() == g_drawbot_fill_path_calls &&
          std::all_of(g_drawbot_fill_colors.begin(), g_drawbot_fill_colors.end(),
              [](const auto& color) { return std::all_of(color.begin(), color.end(),
                  [](float value) { return std::isfinite(value) && value >= 0 && value <= 1; }); }) &&
          g_drawbot_objects_created == g_drawbot_objects_released && in.drawbot_objects_empty &&
          g_drawbot_invalid_operations == 0
      : in.drag_event_mode
          ? event_error == 0 && g_ui_drag_requested &&
              g_ui_drag_calls == static_cast<uint32_t>(in.drag_steps) && g_ui_drag_terminated
          : in.click_event_mode
          ? event_error == 0 && (event_out_flags & 9) == 9 &&
              g_app_color_picker_calls == 1 && g_app_invalidate_rect_calls == 1
          : event_error == 0 && cursor == 13;
  std::cout << "{\"schema_version\":1,\"stage\":\"custom_ui_event\",\"status\":\""
            << (event_contract && event_sequence_setdown_error == 0 &&
                defaults_disposed && handle_lifetimes_balanced()
                ? "event_completed" : "event_failed")
            << "\",\"event_type\":\"" << (in.ui_mouse_exited_mode ? "ui_mouse_exited" :
                (in.ui_keydown_mode ? "ui_keydown" :
                (in.ui_idle_mode ? "ui_idle" :
                (in.ui_lifecycle_mode ? "ui_lifecycle" :
                (in.draw_event_mode ? "draw" :
                (in.drag_event_mode ? "drag_sequence" :
                 (in.click_event_mode ? "do_click" : "adjust_cursor")))))))
            << "\",\"event_target\":\"" << event_target
            << "\",\"event_error\":" << event_error
            << ",\"cursor\":" << cursor << ",\"event_out_flags\":" << event_out_flags
            << ",\"adv_app_info_text_calls\":" << info_text.calls
            << ",\"adv_app_info_text\":\"" << escape(info_text.last_text)
            << "\",\"arbitrary_values_disposed\":"
            << (arbitrary_values_disposed ? "true" : "false")
            << ",\"handle_lifetimes_balanced\":"
            << (handle_lifetimes_balanced() ? "true" : "false")
            << ",\"suite_leases_balanced\":"
            << (suite_leases_balanced() ? "true" : "false")
            << ",\"drawbot_paint_rect_calls\":" << g_drawbot_paint_rect_calls
            << ",\"drawbot_fill_path_calls\":" << g_drawbot_fill_path_calls
            << ",\"drawbot_stroke_path_calls\":" << g_drawbot_stroke_path_calls
            << ",\"overlay_stroke_path_calls\":" << g_overlay_stroke_path_calls
            << ",\"drawbot_objects_created\":" << g_drawbot_objects_created
            << ",\"drawbot_objects_released\":" << g_drawbot_objects_released
            << ",\"drawbot_invalid_operations\":" << g_drawbot_invalid_operations
            << ",\"drawbot_fill_color_count\":" << g_drawbot_fill_colors.size()
            << ",\"drawbot_first_fill_color\":["
            << (g_drawbot_fill_colors.empty() ? 0.0f : g_drawbot_fill_colors[0][0]) << ','
            << (g_drawbot_fill_colors.empty() ? 0.0f : g_drawbot_fill_colors[0][1]) << ','
            << (g_drawbot_fill_colors.empty() ? 0.0f : g_drawbot_fill_colors[0][2]) << ','
            << (g_drawbot_fill_colors.empty() ? 0.0f : g_drawbot_fill_colors[0][3]) << ']'
            << ",\"drawbot_get_drawing_ref_calls\":" << g_drawbot_get_drawing_ref_calls
            << ",\"drawbot_get_supplier_calls\":" << g_drawbot_get_supplier_calls
            << ",\"drawbot_get_surface_calls\":" << g_drawbot_get_surface_calls
            << ",\"app_get_background_color_calls\":" << g_app_get_background_color_calls
            << ",\"app_color_picker_calls\":" << g_app_color_picker_calls
            << ",\"app_invalidate_rect_calls\":" << g_app_invalidate_rect_calls
            << ",\"picker_color_rgba\":[" << g_app_picker_color[0] << ','
            << g_app_picker_color[1] << ',' << g_app_picker_color[2] << ','
            << g_app_picker_color[3] << ']'
            << ",\"invalidated_rect\":[" << g_app_invalidated_rect[0] << ','
            << g_app_invalidated_rect[1] << ',' << g_app_invalidated_rect[2] << ','
            << g_app_invalidated_rect[3] << ']'
            << ",\"changed_value\":" << (changed_value ? "true" : "false")
            << ",\"drag_requested\":" << (g_ui_drag_requested ? "true" : "false")
            << ",\"drag_calls\":" << g_ui_drag_calls
            << ",\"drag_terminated\":" << (g_ui_drag_terminated ? "true" : "false")
            << ",\"coordinate_transform_calls\":" << g_ui_coordinate_transform_calls
            << ",\"lifecycle_errors\":[" << lifecycle_errors[0] << ','
            << lifecycle_errors[1] << ',' << lifecycle_errors[2] << ','
            << lifecycle_errors[3] << ',' << lifecycle_errors[4] << ']'
            << ",\"lifecycle_context_stable\":"
            << (lifecycle_context_stable ? "true" : "false")
            << ",\"plugin_state_before_close\":[" << plugin_state_before_close[0] << ','
            << plugin_state_before_close[1] << ',' << plugin_state_before_close[2] << ','
            << plugin_state_before_close[3] << ']'
            << ",\"lifecycle_host_state_cleared\":"
            << (lifecycle_host_state_cleared ? "true" : "false")
            << ",\"keydown_code\":" << in.keydown_code
            << ",\"keydown_modifiers\":" << in.keydown_modifiers
            << ",\"event_assignments_applied\":"
            << (event_assignments_applied ? "true" : "false")
            << ",\"sequence_setdown_error\":" << event_sequence_setdown_error
            << ",\"requested_parameters\":"
            << in.requested_parameters_json_text
            << ",\"global_setdown_error\":" << event_setdown_error << "}\n";
  return event_contract && event_sequence_setdown_error == 0 &&
      defaults_disposed && handle_lifetimes_balanced() && event_setdown_error == 0;
}

}  // namespace aexcompat::l2_detail
