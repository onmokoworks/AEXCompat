#include "worker_invocation_orchestration.hpp"

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
