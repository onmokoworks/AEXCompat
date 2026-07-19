#include "worker_invocation_orchestration.hpp"

#include <cmath>
#include <cwchar>
#include <string>

namespace aexcompat::worker_runtime::invocation {
namespace {
void apply_common(const request_parser::WorkerInvocation& source,
                  InvocationState& target) {
  const auto& mode = source.mode;
  target.external_pixel_bytes = mode.external_pixel_bytes;
  target.external_rgba = source.rgba;
  target.external_layers = source.layers;
  target.external_output = source.output;
  target.external_width = source.width;
  target.external_height = source.height;
  target.external_current_time = source.current_time;
  target.external_time_step = source.time_step;
  target.external_total_time = source.total_time;
  target.external_time_scale = source.time_scale;
}
void apply_click_draw(const request_parser::WorkerInvocation& source,
                      const ApplyHooks& hooks) {
  if (source.mode.image_click_context && hooks.set_click)
    hooks.set_click(source.click_x, source.click_y, source.picker_color);
  if (source.mode.image_draw_context && hooks.enable_draw) hooks.enable_draw();
}
}  // namespace

int parse_l2_modes(int argc, wchar_t** argv, InvocationState& target,
                   const L2ModeHooks& hooks) {
  target.user_changed_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--user-changed";
  target.aegp_update_menu_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-update-menu";
  target.aegp_idle_mode = argc == 4 && std::wstring(argv[1]) == L"--aegp-idle";
  target.aegp_command_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-command-roundtrip";
  target.aegp_active_idle_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-active-idle-roundtrip";
  target.aegp_keyframe_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-keyframe-roundtrip";
  target.aegp_seek_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-seek-roundtrip";
  target.aegp_trim_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-trim-roundtrip";
  target.aegp_switch_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-switch-roundtrip";
  target.aegp_comp_idle_roundtrip_mode = argc == 4 &&
      (std::wstring(argv[1]) == L"--aegp-comp-idle-roundtrip" ||
       target.aegp_keyframe_roundtrip_mode || target.aegp_seek_roundtrip_mode ||
       target.aegp_trim_roundtrip_mode || target.aegp_switch_roundtrip_mode);
  target.aegp_init_mode = (argc == 4 && std::wstring(argv[1]) == L"--aegp-init") ||
      target.aegp_update_menu_mode || target.aegp_idle_mode ||
      target.aegp_command_roundtrip_mode || target.aegp_active_idle_roundtrip_mode ||
      target.aegp_comp_idle_roundtrip_mode;
  target.params_only_mode = (argc == 4 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-params-only";
  target.runtime_module_authorization_mode = target.params_only_mode && argc == 6 &&
      std::wstring(argv[4]) == L"--runtime-module-authorization-v1";
  target.external_dependencies_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--l2-external-dependencies";
  target.do_dialog_mode = argc == 4 && std::wstring(argv[1]) == L"--l2-do-dialog";
  target.auto_dialog_mode = argc == 4 && std::wstring(argv[1]) == L"--l2-auto-dialog";
  target.adjust_cursor_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-adjust-cursor";
  target.draw_event_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-draw-event";
  target.click_event_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-click-event";
  target.drag_event_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-drag-event";
  target.ui_lifecycle_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-ui-lifecycle";
  target.ui_idle_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-ui-idle";
  target.ui_keydown_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-ui-keydown";
  target.ui_mouse_exited_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-ui-mouse-exited";
  target.ui_event_assignment_mode =
      ((target.adjust_cursor_mode || target.draw_event_mode || target.ui_lifecycle_mode ||
        target.ui_idle_mode || target.ui_mouse_exited_mode) && argc == 5) ||
      ((target.click_event_mode || target.drag_event_mode || target.ui_keydown_mode) && argc == 6);
  if (target.ui_event_assignment_mode &&
      (!hooks.parse_parameter_payload ||
       !hooks.parse_parameter_payload(argv[argc - 1], target.ui_event_assignments))) return 3;
  if (target.click_event_mode) {
    float red{}, green{}, blue{}, alpha{};
    if (swscanf_s(argv[4], L"%d,%d,%f,%f,%f,%f", &target.click_x, &target.click_y,
                  &red, &green, &blue, &alpha) != 6 ||
        target.click_x < 0 || target.click_x > 8192 || target.click_y < 0 || target.click_y > 8192 ||
        !std::isfinite(red) || !std::isfinite(green) || !std::isfinite(blue) || !std::isfinite(alpha) ||
        red < 0 || red > 1 || green < 0 || green > 1 || blue < 0 || blue > 1 || alpha < 0 || alpha > 1) return 3;
    target.picker_color = {red, green, blue, alpha};
  }
  if (target.drag_event_mode &&
      (swscanf_s(argv[4], L"%d,%d,%d,%d,%d", &target.click_x, &target.click_y,
                 &target.drag_end_x, &target.drag_end_y, &target.drag_steps) != 5 ||
       target.click_x < 0 || target.click_x > 8192 || target.click_y < 0 || target.click_y > 8192 ||
       target.drag_end_x < 0 || target.drag_end_x > 8192 || target.drag_end_y < 0 || target.drag_end_y > 8192 ||
       target.drag_steps < 1 || target.drag_steps > 32)) return 3;
  if (target.ui_keydown_mode &&
      (swscanf_s(argv[4], L"%d,%d,%u,%u", &target.click_x, &target.click_y,
                 &target.keydown_code, &target.keydown_modifiers) != 4 ||
       target.click_x < 0 || target.click_x > 8192 || target.click_y < 0 || target.click_y > 8192 ||
       (target.keydown_code & 0x3fff0000u) != 0 || target.keydown_modifiers > 0xffffu)) return 3;
  target.skip_about_mode = (argc == 4 && std::wstring(argv[1]) == L"--l2-no-about") ||
      target.params_only_mode || target.external_dependencies_mode || target.do_dialog_mode ||
      target.auto_dialog_mode || target.adjust_cursor_mode || target.draw_event_mode ||
      target.click_event_mode || target.drag_event_mode || target.ui_lifecycle_mode ||
      target.ui_idle_mode || target.ui_keydown_mode || target.ui_mouse_exited_mode;
  if (!target.user_changed_mode && !target.aegp_init_mode && !target.skip_about_mode &&
      (argc != 4 || std::wstring(argv[1]) != L"--l2")) return 2;
  if (target.params_only_mode && argc == 6 && !target.runtime_module_authorization_mode) return 2;
  if (target.user_changed_mode) {
    try { target.user_changed_param_slot = std::stoi(argv[4]); } catch (...) { return 3; }
    if (target.user_changed_param_slot <= 0 ||
        (hooks.max_params != 0 &&
         target.user_changed_param_slot > static_cast<int32_t>(hooks.max_params))) return 3;
    if (argc == 6 && (!hooks.parse_parameter_payload ||
                      !hooks.parse_parameter_payload(argv[5], target.user_changed_parameters))) return 3;
    target.user_changed_param_requested = true;
  }
  return 0;
}

void apply_render(const request_parser::WorkerInvocation& source,
                  InvocationState& target, const ApplyHooks& hooks) {
  apply_common(source, target);
  const auto& mode = source.mode;
  target.audio_mode = mode.audio_mode;
  target.image_audio_mode = mode.image_audio_mode;
  target.transport_argc = mode.transport_argc;
  target.image_click_context = mode.image_click_context;
  target.image_draw_context = mode.image_draw_context;
  target.image_click_argc = mode.image_click_argc;
  target.image_render_environment = mode.image_render_environment;
  target.image_environment_argc = mode.image_environment_argc;
  target.image_spatial_context = mode.image_spatial_context;
  target.image_trailer_argc = mode.image_trailer_argc;
  target.image_mask_context = mode.image_mask_context;
  target.image_argc = mode.image_argc;
  target.layered_image_mode = mode.layered_image_mode;
  target.image_mode = mode.image_mode;
  target.request_mode = mode.request_mode;
  target.external_audio = source.audio;
  target.external_audio_output = source.audio_output;
  target.external_audio_samples = source.audio_samples;
  target.external_audio_rate = source.audio_rate;
  apply_click_draw(source, hooks);
  if (mode.image_audio_mode && hooks.set_audio_source)
    hooks.set_audio_source(&target.external_audio, target.external_audio_samples);
}

void apply_smart(const request_parser::WorkerInvocation& source,
                 InvocationState& target, const ApplyHooks& hooks) {
  apply_common(source, target);
  const auto& mode = source.mode;
  target.smart_force_cpu = mode.force_cpu;
  target.smart_opencl = mode.opencl;
  target.smart_directx = mode.directx;
  target.smart_image_click_context = mode.image_click_context;
  target.smart_image_draw_context = mode.image_draw_context;
  target.smart_image_click_argc = mode.image_click_argc;
  target.smart_image_render_environment = mode.image_render_environment;
  target.smart_image_environment_argc = mode.image_environment_argc;
  target.smart_image_spatial_context = mode.image_spatial_context;
  target.smart_image_trailer_argc = mode.image_trailer_argc;
  target.smart_image_mask_context = mode.image_mask_context;
  target.smart_image_argc = mode.image_argc;
  target.smart_layered_image_mode = mode.layered_image_mode;
  target.smart_image_mode = mode.image_mode;
  target.mask_request_mode = mode.mask_request_mode;
  target.mask_scene_request_mode = mode.mask_scene_request_mode;
  target.mask_context_request_mode = mode.mask_context_request_mode;
  target.mask_count_error_mode = mode.mask_count_error_mode;
  target.mask_count_crash_mode = mode.mask_count_crash_mode;
  target.mask_double_dispose_mode = mode.mask_double_dispose_mode;
  target.stream_live_value_dispose_mode = mode.stream_live_value_dispose_mode;
  target.stream_metadata_ownership_mode = mode.stream_metadata_ownership_mode;
  target.keyframe_ownership_mode = mode.keyframe_ownership_mode;
  target.dynamic_stream_tree_mode = mode.dynamic_stream_tree_mode;
  target.aegp_memory_strings_mode = mode.aegp_memory_strings_mode;
  target.suite_release_without_acquire_mode = mode.suite_release_without_acquire_mode;
  target.handle_resize_while_locked_mode = mode.handle_resize_while_locked_mode;
  target.world_double_dispose_mode = mode.world_double_dispose_mode;
  target.world_allocation_limit_mode = mode.world_allocation_limit_mode;
  target.pixel_format_registry_mode = mode.pixel_format_registry_mode;
  target.outline_mutation_mode = mode.outline_mutation_mode;
  target.mask_attribute_mode = mode.mask_attribute_mode;
  target.request_mode = mode.request_mode;
  if (hooks.set_mask_mode) hooks.set_mask_mode(mode.mask_model_enabled);
  if (hooks.set_mask_fault)
    hooks.set_mask_fault(mode.mask_count_error_mode, mode.mask_count_crash_mode);
  apply_click_draw(source, hooks);
}
}  // namespace aexcompat::worker_runtime::invocation
