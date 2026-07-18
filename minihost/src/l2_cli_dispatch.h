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
