#pragma once

#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::fixed_selftests {

struct Request {
  int argc{};
  wchar_t** argv{};
  bool render_worker{};
};

// The verification bodies for the six former host wrappers live in their
// owner TUs (host guard, utility suite, compat selftests, pf adv time,
// minidump runtime); only l2_main-private helpers remain hooked.
struct HostHooks {
  std::string (*escape)(const std::string&){};
  uint32_t (*trigger_guarded_crash)(){};
  bool (*suite_leases_balanced)(){};
};

struct SimpleHooks {
  bool (*aegp_installed_effect_catalog)(){};
  bool (*aegp_layer_suite1_slots)(){};
  bool (*aegp_loaded_plugin_effect_streams)(){};
  bool (*parameter_animation)(){};
  bool (*parameter_registry_capacity)(){};
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
  bool (*world_transform_convolve)(){};
  bool (*world_transform_transfer_mask)(){};
  bool (*aegp_world_suite3)(){};
  bool (*pf_batch_sampling_suite)(){};
  bool (*pf_ae_channel_native_provider)(){};
  bool (*aegp_layer_render_options_suite2)(){};
  bool (*pf_utils_handle_callbacks)(){};
  bool (*utility_callback_table)(){};
  bool (*pf_utils_composite_rect)(){};
  bool (*checkout_param_beyond_table)(){};
  bool (*pf_private_callbacks)(){};
  bool (*flt_blur_suite1)(){};
  bool (*aefx_ace_suite1)(){};
  bool (*aegp_persistent_data_suite3)(){};
  bool (*native_stdout_routing)(){};
  bool (*aegp_persistent_data_suite4)(){};
  bool (*headless_system_sound_suppression)(){};
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
