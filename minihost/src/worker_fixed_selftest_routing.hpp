#pragma once

namespace aexcompat::worker_runtime::fixed_selftests {

struct Request {
  int argc{};
  wchar_t** argv{};
  bool render_worker{};
};

struct HostHooks {
  int (*render_output_safety)(int, wchar_t**){};
  int (*crash_minidump)(int, wchar_t**){};
  int (*pf_adv_time_suite1)(int, wchar_t**){};
  int (*suite_entry_utility13)(int, wchar_t**){};
  int (*pf_adv_app_suite)(int, wchar_t**){};
  int (*aegp_effect_param_union_suite4)(int, wchar_t**){};
};

struct SimpleHooks {
  bool (*aegp_installed_effect_catalog)(){};
  bool (*parameter_animation)(){};
  bool (*pf_param_utils)(){};
  bool (*pf_pre_checkout_result)(){};
  bool (*pf_checkout_intersection)(){};
  bool (*pf_smart_geometry_rects)(){};
  bool (*smart_runtime_concurrency)(){};
  bool (*smart_result_skipped)(){};
  bool (*pf_pixel_data)(){};
  bool (*pf_fill_matte_legacy)(){};
  bool (*pf_ae_channel_suite)(){};
  bool (*pf_color_suite)(){};
  bool (*pf_color_param_suite)(){};
  bool (*pf_iterate)(){};
  bool (*world_transform_composite)(){};
  bool (*world_transform_affine)(){};
  bool (*world_transform_blend)(){};
  bool (*world_transform_transfer_mask)(){};
  bool (*aegp_world_suite3)(){};
  bool (*pf_batch_sampling_suite)(){};
  bool (*pf_ae_channel_native_provider)(){};
  bool (*aegp_layer_render_options_suite2)(){};
};

struct Hooks {
  HostHooks host;
  SimpleHooks simple;
};

struct Result {
  bool handled{};
  int exit_code{};
};

// Owns the fixed self-test command catalog, exact command arity, protocol keys,
// metadata, and failure exit codes. Private host implementations remain behind
// explicit hooks so this translation unit does not cross l2_main's ABI boundary.
Result dispatch(const Request& request, const Hooks& hooks);

}  // namespace aexcompat::worker_runtime::fixed_selftests
