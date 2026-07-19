#pragma once

#include <array>
#include <cstdint>
#include <vector>

namespace aexcompat::worker_runtime::custom_ui {

// Host UI/Drawbot fixture telemetry is process-local and intentionally owned
// by this runtime component.  l2_main keeps the established g_* aliases at
// callback and report sites so the SDK-facing ABI and JSON remain unchanged.
struct State {
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

State& state();

}  // namespace aexcompat::worker_runtime::custom_ui
