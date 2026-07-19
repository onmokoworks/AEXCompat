#pragma once
#include "worker_parameter_runtime.hpp"
#include "worker_request_parser.hpp"
#include <array>
#include <cstdint>
#include <filesystem>
#include <vector>
namespace aexcompat::worker_runtime::invocation {
using parameters::RequestedAssignments;
struct InvocationState {
    bool request_mode{};
    bool audio_mode{};
    bool image_audio_mode{};
    bool image_mode{};
    bool render_session_mode{};
    bool layered_image_mode{};
    bool smart_force_cpu{};
    bool smart_opencl{};
    bool smart_directx{};
    bool smart_image_mode{};
    bool smart_layered_image_mode{};
    int32_t external_pixel_bytes{4};
    int transport_argc{};
    int image_click_argc{};
    int image_environment_argc{};
    int image_trailer_argc{};
    int image_argc{};
    int smart_image_click_argc{};
    int smart_image_environment_argc{};
    int smart_image_trailer_argc{};
    int smart_image_argc{};
    bool image_click_context{};
    bool image_draw_context{};
    bool image_render_environment{};
    bool image_spatial_context{};
    bool image_mask_context{};
    bool smart_image_click_context{};
    bool smart_image_draw_context{};
    bool smart_image_render_environment{};
    bool smart_image_spatial_context{};
    bool smart_image_mask_context{};
    bool mask_request_mode{};
    bool mask_scene_request_mode{};
    bool mask_context_request_mode{};
    bool mask_count_error_mode{};
    bool mask_count_crash_mode{};
    bool mask_double_dispose_mode{};
    bool stream_live_value_dispose_mode{};
    bool stream_metadata_ownership_mode{};
    bool keyframe_ownership_mode{};
    bool dynamic_stream_tree_mode{};
    bool aegp_memory_strings_mode{};
    bool suite_release_without_acquire_mode{};
    bool handle_resize_while_locked_mode{};
    bool world_double_dispose_mode{};
    bool world_allocation_limit_mode{};
    bool pixel_format_registry_mode{};
    bool outline_mutation_mode{};
    bool mask_attribute_mode{};
    bool user_changed_mode{};
    bool params_only_mode{};
    bool runtime_module_authorization_mode{};
    bool external_dependencies_mode{};
    bool do_dialog_mode{};
    bool auto_dialog_mode{};
    bool adjust_cursor_mode{};
    bool draw_event_mode{};
    bool click_event_mode{};
    bool drag_event_mode{};
    bool ui_lifecycle_mode{};
    bool ui_idle_mode{};
    bool ui_keydown_mode{};
    bool ui_mouse_exited_mode{};
    bool ui_event_assignment_mode{};
    bool aegp_update_menu_mode{};
    bool aegp_idle_mode{};
    bool aegp_command_roundtrip_mode{};
    bool aegp_active_idle_roundtrip_mode{};
    bool aegp_keyframe_roundtrip_mode{};
    bool aegp_seek_roundtrip_mode{};
    bool aegp_trim_roundtrip_mode{};
    bool aegp_switch_roundtrip_mode{};
    bool aegp_comp_idle_roundtrip_mode{};
    bool aegp_init_mode{};
    bool skip_about_mode{};
    bool user_changed_param_requested{};
    int32_t user_changed_param_slot{-1};
    std::array<float, 4> picker_color{1.0f, 0.25f, 0.75f, 0.5f};
    RequestedAssignments user_changed_parameters;
    RequestedAssignments requested_parameters;
    RequestedAssignments ui_event_assignments;
    std::vector<unsigned char> external_rgba;
    std::vector<request_parser::LayerInput> external_layers;
    std::filesystem::path external_output;
    std::vector<float> external_audio;
    std::filesystem::path external_audio_output;
    int32_t external_width{};
    int32_t external_height{};
    int32_t external_current_time{};
    int32_t external_time_step{1};
    int32_t external_total_time{1};
    uint32_t external_time_scale{1};
    int32_t external_audio_samples{};
    int32_t external_audio_rate{};
    int32_t click_x{101};
    int32_t click_y{101};
    int32_t drag_end_x{101};
    int32_t drag_end_y{101};
    int32_t drag_steps{};
    uint32_t keydown_code{};
    uint32_t keydown_modifiers{};
  };

struct ApplyHooks {
  void (*set_click)(int32_t, int32_t, const std::array<float, 4>&){};
  void (*enable_draw)(){};
  void (*set_mask_mode)(bool){};
  void (*set_mask_fault)(bool, bool){};
  void (*set_audio_source)(std::vector<float>*, int32_t){};
};
struct L2ModeHooks {
  bool (*parse_parameter_payload)(const wchar_t*, RequestedAssignments&){};
  std::size_t max_params{};
};
int parse_l2_modes(int argc, wchar_t** argv, InvocationState&, const L2ModeHooks&);
void apply_render(const request_parser::WorkerInvocation&, InvocationState&, const ApplyHooks&);
void apply_smart(const request_parser::WorkerInvocation&, InvocationState&, const ApplyHooks&);
}  // namespace aexcompat::worker_runtime::invocation

