#include "l2_cli_dispatch.h"

#include <cwchar>
#include <string>

namespace aexcompat::l2cli {
namespace {

bool equals(const wchar_t* value, const wchar_t* expected) {
  return value && std::wcscmp(value, expected) == 0;
}

bool starts_with(const wchar_t* value, const wchar_t* prefix) {
  if (!value) return false;
  const std::wstring text(value);
  return text.compare(0, std::wcslen(prefix), prefix) == 0;
}

bool has_command(int argc, wchar_t** argv, const wchar_t* command) {
  return argc > 1 && equals(argv[1], command);
}

}  // namespace

AuxiliaryOptionResult strip_auxiliary_options(
    int argc, wchar_t** argv, const AuxiliaryOptionHooks& hooks) {
  if (argc < 0 || !argv || !hooks.set_dump_worlds_dir ||
      !hooks.enable_checksum_detail || !hooks.load_aux_manifest ||
      !hooks.parse_alpha_coverage || !hooks.load_parameter_animation)
    return {argc, false};

  int effective_argc = argc;
  bool saw_aux = false, saw_animation = false, saw_coverage = false;
  bool saw_dump_worlds = false, saw_checksum_detail = false;
  while (effective_argc >= 3) {
    const wchar_t* flag = argv[effective_argc - 2];
    const wchar_t* value = argv[effective_argc - 1];
    bool accepted = false;
    if (equals(flag, L"--dump-worlds-v1") && !saw_dump_worlds) {
      accepted = hooks.set_dump_worlds_dir(hooks.context, value);
      saw_dump_worlds = accepted;
    } else if (equals(flag, L"--output-checksum-detail-v1") && !saw_checksum_detail) {
      accepted = equals(value, L"1") && hooks.enable_checksum_detail(hooks.context);
      saw_checksum_detail = accepted;
    } else if (equals(flag, L"--aux-manifest-v1") && !saw_aux) {
      accepted = hooks.load_aux_manifest(hooks.context, value);
      saw_aux = accepted;
    } else if (equals(flag, L"--alpha-as-coverage-v1") && !saw_coverage) {
      accepted = hooks.parse_alpha_coverage(hooks.context, value);
      saw_coverage = accepted;
    } else if (equals(flag, L"--parameter-animation-v1") && !saw_animation) {
      accepted = hooks.load_parameter_animation(hooks.context, value);
      saw_animation = accepted;
    } else {
      break;
    }
    if (!accepted) return {effective_argc, false};
    effective_argc -= 2;
  }
  return {effective_argc, true};
}

WorkerMode classify_worker_mode(
    WorkerKind kind, int argc, wchar_t** argv, int effective_argc) {
  WorkerMode mode{};
  if (argc < 0 || !argv || effective_argc < 0) return mode;
  const wchar_t* command = argc > 1 ? argv[1] : L"";

  if (kind == WorkerKind::Render) {
    const bool image16 = equals(command, L"--render-image16") ||
        equals(command, L"--render-image16-layer");
    const bool image32 = equals(command, L"--render-image32") ||
        equals(command, L"--render-image32-layer");
    mode.audio_mode = effective_argc == 9 && equals(command, L"--render-audio");
    mode.image_audio_mode = effective_argc == 16 && equals(command, L"--render-image-audio");
    mode.external_pixel_bytes = image32 ? 16 : (image16 ? 8 : 4);
    mode.transport_argc = mode.image_audio_mode ? 13 : effective_argc;
    mode.image_click_context = mode.transport_argc >= 14 &&
        starts_with(argv[mode.transport_argc - 1], L"click:v1|");
    mode.image_draw_context = mode.transport_argc >= 14 &&
        equals(argv[mode.transport_argc - 1], L"draw:v1");
    mode.image_click_argc = mode.transport_argc -
        ((mode.image_click_context || mode.image_draw_context) ? 1 : 0);
    mode.image_render_environment = mode.image_click_argc >= 14 &&
        starts_with(argv[mode.image_click_argc - 1], L"render:v1|");
    mode.image_environment_argc = mode.image_click_argc -
        (mode.image_render_environment ? 1 : 0);
    mode.image_spatial_context = mode.image_environment_argc >= 14 &&
        starts_with(argv[mode.image_environment_argc - 1], L"spatial:v");
    mode.image_trailer_argc = mode.image_environment_argc -
        (mode.image_spatial_context ? 1 : 0);
    mode.image_mask_context = mode.image_trailer_argc >= 14 &&
        starts_with(argv[mode.image_trailer_argc - 1], L"v2|");
    mode.image_argc = mode.image_trailer_argc - (mode.image_mask_context ? 1 : 0);
    mode.layered_image_mode = mode.image_argc >= 17 &&
        (mode.image_argc - 13) % 4 == 0 && (mode.image_argc - 13) / 4 <= 64 &&
        (equals(command, L"--render-image-layer") ||
         equals(command, L"--render-image16-layer") ||
         equals(command, L"--render-image32-layer"));
    mode.image_mode = mode.image_audio_mode ||
        (mode.image_argc == 13 && (equals(command, L"--render-image") ||
         equals(command, L"--render-image16") ||
         equals(command, L"--render-image32"))) || mode.layered_image_mode;
    mode.request_mode = (effective_argc == 5 && has_command(argc, argv, L"--render-request")) ||
        mode.image_mode || mode.audio_mode;
    mode.command_accepted = mode.request_mode ||
        (effective_argc == 5 && has_command(argc, argv, L"--render"));
    return mode;
  }

  mode.force_cpu = equals(command, L"--smart-image32-cpu") ||
      equals(command, L"--smart-image32-cpu-layer");
  mode.opencl = equals(command, L"--smart-image32-opencl");
  mode.directx = equals(command, L"--smart-image32-directx");
  const bool image16 = equals(command, L"--smart-image16") ||
      equals(command, L"--smart-image16-layer");
  const bool image32 = equals(command, L"--smart-image32") ||
      equals(command, L"--smart-image32-layer") || mode.force_cpu || mode.opencl || mode.directx;
  mode.external_pixel_bytes = image32 ? 16 : (image16 ? 8 : 4);
  mode.image_click_context = effective_argc >= 14 &&
      starts_with(argv[effective_argc - 1], L"click:v1|");
  mode.image_draw_context = effective_argc >= 14 && equals(argv[effective_argc - 1], L"draw:v1");
  mode.image_click_argc = effective_argc -
      ((mode.image_click_context || mode.image_draw_context) ? 1 : 0);
  mode.image_render_environment = mode.image_click_argc >= 14 &&
      starts_with(argv[mode.image_click_argc - 1], L"render:v1|");
  mode.image_environment_argc = mode.image_click_argc - (mode.image_render_environment ? 1 : 0);
  mode.image_spatial_context = mode.image_environment_argc >= 14 &&
      starts_with(argv[mode.image_environment_argc - 1], L"spatial:v");
  mode.image_trailer_argc = mode.image_environment_argc - (mode.image_spatial_context ? 1 : 0);
  mode.image_mask_context = mode.image_trailer_argc >= 14 &&
      starts_with(argv[mode.image_trailer_argc - 1], L"v2|");
  mode.image_argc = mode.image_trailer_argc - (mode.image_mask_context ? 1 : 0);
  mode.layered_image_mode = mode.image_argc >= 17 && (mode.image_argc - 13) % 4 == 0 &&
      (mode.image_argc - 13) / 4 <= 64 &&
      (equals(command, L"--smart-image-layer") || equals(command, L"--smart-image16-layer") ||
       equals(command, L"--smart-image32-layer") ||
       equals(command, L"--smart-image32-cpu-layer"));
  mode.image_mode = (mode.image_argc == 13 &&
      (equals(command, L"--smart-image") || equals(command, L"--smart-image16") ||
       equals(command, L"--smart-image32") || equals(command, L"--smart-image32-cpu") ||
       mode.opencl || mode.directx)) || mode.layered_image_mode;
  mode.mask_request_mode = argc == 5 && has_command(argc, argv, L"--smart-mask-request");
  mode.mask_scene_request_mode = argc == 6 && has_command(argc, argv, L"--smart-mask-scene-request");
  mode.mask_context_request_mode = argc == 6 && has_command(argc, argv, L"--smart-mask-context-request");
  mode.mask_count_error_mode = argc == 5 && has_command(argc, argv, L"--smart-mask-count-error-request");
  mode.mask_count_crash_mode = argc == 5 && has_command(argc, argv, L"--smart-mask-count-crash-request");
  mode.mask_double_dispose_mode = argc == 5 && has_command(argc, argv, L"--smart-mask-double-dispose-request");
  mode.stream_live_value_dispose_mode = argc == 5 && has_command(argc, argv, L"--smart-stream-live-value-dispose-request");
  mode.stream_metadata_ownership_mode = argc == 5 && has_command(argc, argv, L"--smart-stream-metadata-ownership-request");
  mode.keyframe_ownership_mode = argc == 5 && has_command(argc, argv, L"--smart-keyframe-ownership-request");
  mode.dynamic_stream_tree_mode = argc == 5 && has_command(argc, argv, L"--smart-dynamic-stream-tree-request");
  mode.aegp_memory_strings_mode = argc == 5 && has_command(argc, argv, L"--smart-aegp-memory-strings-request");
  mode.suite_release_without_acquire_mode = argc == 5 && has_command(argc, argv, L"--smart-suite-release-without-acquire-request");
  mode.handle_resize_while_locked_mode = argc == 5 && has_command(argc, argv, L"--smart-handle-resize-while-locked-request");
  mode.world_double_dispose_mode = argc == 5 && has_command(argc, argv, L"--smart-world-double-dispose-request");
  mode.world_allocation_limit_mode = argc == 5 && has_command(argc, argv, L"--smart-world-allocation-limit-request");
  mode.pixel_format_registry_mode = argc == 5 && has_command(argc, argv, L"--smart-pixel-format-registry-request");
  mode.outline_mutation_mode = argc == 5 && has_command(argc, argv, L"--smart-outline-mutation-request");
  mode.mask_attribute_mode = argc == 5 && has_command(argc, argv, L"--smart-mask-attribute-request");
  mode.request_mode = mode.image_mode || mode.mask_request_mode || mode.mask_scene_request_mode ||
      mode.mask_context_request_mode || mode.mask_count_error_mode || mode.mask_count_crash_mode ||
      mode.mask_double_dispose_mode || mode.stream_live_value_dispose_mode ||
      mode.stream_metadata_ownership_mode || mode.keyframe_ownership_mode ||
      mode.dynamic_stream_tree_mode || mode.aegp_memory_strings_mode ||
      mode.suite_release_without_acquire_mode || mode.handle_resize_while_locked_mode ||
      mode.world_double_dispose_mode || mode.world_allocation_limit_mode ||
      mode.pixel_format_registry_mode || mode.outline_mutation_mode || mode.mask_attribute_mode ||
      (argc == 5 && has_command(argc, argv, L"--smart-request"));
  mode.command_accepted = mode.request_mode || (argc == 5 && has_command(argc, argv, L"--smart"));
  mode.mask_model_enabled = mode.mask_request_mode || mode.mask_scene_request_mode ||
      mode.mask_context_request_mode || mode.mask_count_error_mode || mode.mask_count_crash_mode ||
      mode.mask_double_dispose_mode || mode.stream_live_value_dispose_mode ||
      mode.stream_metadata_ownership_mode || mode.keyframe_ownership_mode ||
      mode.dynamic_stream_tree_mode || mode.aegp_memory_strings_mode ||
      mode.suite_release_without_acquire_mode || mode.handle_resize_while_locked_mode ||
      mode.world_double_dispose_mode || mode.world_allocation_limit_mode ||
      mode.pixel_format_registry_mode || mode.outline_mutation_mode || mode.mask_attribute_mode;
  return mode;
}

}  // namespace aexcompat::l2cli
