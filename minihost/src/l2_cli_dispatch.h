#pragma once

namespace aexcompat::l2cli {

// Keep command admission outside the host implementation.  The callbacks make
// the stateful, authenticated host actions explicit rather than sharing globals
// with this translation unit.
struct AuxiliaryOptionHooks {
  void* context{};
  bool (*set_dump_worlds_dir)(void* context, const wchar_t* value){};
  bool (*enable_checksum_detail)(void* context){};
  bool (*load_aux_manifest)(void* context, const wchar_t* value){};
  bool (*parse_alpha_coverage)(void* context, const wchar_t* value){};
  bool (*load_parameter_animation)(void* context, const wchar_t* value){};
  bool (*parse_conformance_render_settings)(void* context, const wchar_t* value){};
  // Optional (#290/#300): captures the `--runtime-module-authorization-v1`
  // manifest basename on a GPU render so the render/smart worker authorizes the
  // GPU runtime DLLs for the module audit. Null on paths that never carry it.
  bool (*set_runtime_module_authorization)(void* context, const wchar_t* value){};
};

struct AuxiliaryOptionResult {
  int effective_argc{};
  bool accepted{};
};

AuxiliaryOptionResult strip_auxiliary_options(
    int argc, wchar_t** argv, const AuxiliaryOptionHooks& hooks);

enum class WorkerKind { Render, Smart };

struct WorkerMode {
  int external_pixel_bytes{4};
  int transport_argc{};
  int image_click_argc{};
  int image_environment_argc{};
  int image_trailer_argc{};
  int image_argc{};
  bool request_mode{};
  bool command_accepted{};
  bool audio_mode{};
  bool image_audio_mode{};
  bool image_mode{};
  bool render_session_mode{};
  // Resident audio render session (`--render-audio-session-v1`, protocol §10).
  bool audio_session_mode{};
  // Session secondary-layer trailer (`session-layers:v2|`) present; when set,
  // it is the positional argument at index `image_argc - 1` (issue #98 W1-4,
  // per-layer inherited file handles #268).
  bool session_layers{};
  // Session audio-source trailer (`session-audio:v1|<samples>|<rate>|<path>`)
  // present; when set, it is the positional argument at index
  // `session_audio_argc`. The one-shot spends three bare slots on the same three
  // values under its own command word (`--render-image-audio`); a session cannot,
  // because its tail is shared with the other optional trailers, so the values
  // ride one marked argument peeled like the rest (issue #339).
  bool session_audio{};
  int session_audio_argc{};
  bool layered_image_mode{};
  bool image_click_context{};
  bool image_draw_context{};
  bool image_render_environment{};
  bool image_spatial_context{};
  bool image_mask_context{};
  bool force_cpu{};
  bool opencl{};
  bool directx{};
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
  bool mask_model_enabled{};
};

WorkerMode classify_worker_mode(
    WorkerKind kind, int argc, wchar_t** argv, int effective_argc);

}  // namespace aexcompat::l2cli
