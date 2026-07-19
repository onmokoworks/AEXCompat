#include "worker_fixed_selftest_routing.hpp"

#include "worker_selftest_dispatch.hpp"

#include <array>
#include <string_view>

namespace aexcompat::worker_runtime::fixed_selftests {

Result dispatch(const Request& request, const Hooks& hooks) {
  const std::array<selftest::HostCommand, 6> host_commands{{
      {L"--self-test-render-output-safety", 2, hooks.host.render_output_safety},
      {L"--self-test-crash-minidump", 3, hooks.host.crash_minidump},
      {L"--self-test-pf-adv-time-suite1", 2, hooks.host.pf_adv_time_suite1},
      {L"--self-test-suite-entry-utility13", 2, hooks.host.suite_entry_utility13},
      {L"--self-test-pf-adv-app-suite", 2, hooks.host.pf_adv_app_suite},
      {L"--self-test-aegp-effect-param-union-suite4", 2,
       hooks.host.aegp_effect_param_union_suite4},
  }};
  if (const auto exit = selftest::dispatch_host(
          request.argc, request.argv, host_commands.data(), host_commands.size()))
    return {true, *exit};

  constexpr std::wstring_view kRenderOptionsCommand =
      L"--self-test-aegp-layer-render-options-suite2";
  if (request.argc == 2 && request.argv && request.argv[1] &&
      std::wstring_view(request.argv[1]) == kRenderOptionsCommand &&
      !request.render_worker)
    return {};

  const std::array<selftest::SimpleCommand, 22> simple_commands{{
      {L"--self-test-aegp-installed-effect-catalog", "aegp_installed_effect_catalog",
       hooks.simple.aegp_installed_effect_catalog},
      {L"--self-test-parameter-animation", "parameter_animation_transport",
       hooks.simple.parameter_animation},
      {L"--self-test-pf-param-utils-suite", "pf_param_utils_suite3",
       hooks.simple.pf_param_utils},
      {L"--self-test-pf-pre-checkout-result", "pf_pre_checkout_result",
       hooks.simple.pf_pre_checkout_result},
      {L"--self-test-pf-checkout-intersection", "pf_checkout_intersection",
       hooks.simple.pf_checkout_intersection},
      {L"--self-test-pf-smart-geometry-rects", "pf_smart_geometry_rects",
       hooks.simple.pf_smart_geometry_rects},
      {L"--self-test-smart-runtime-concurrency", "smart_runtime_concurrency",
       hooks.simple.smart_runtime_concurrency},
      {L"--self-test-smart-result-skipped", "smart_result_skipped",
       hooks.simple.smart_result_skipped},
      {L"--self-test-pf-pixel-data", "pf_pixel_data_suite",
       hooks.simple.pf_pixel_data},
      {L"--self-test-pf-fill-matte-legacy", "pf_fill_matte_legacy_callbacks",
       hooks.simple.pf_fill_matte_legacy},
      {L"--self-test-pf-ae-channel-suite", "pf_ae_channel_suite",
       hooks.simple.pf_ae_channel_suite},
      {L"--self-test-pf-color-suite", "pf_color_suite", hooks.simple.pf_color_suite},
      {L"--self-test-pf-color-param-suite", "pf_color_param_suite",
       hooks.simple.pf_color_param_suite},
      {L"--self-test-pf-iterate", "pf_iterate_suite", hooks.simple.pf_iterate},
      {L"--self-test-world-transform-composite", "world_transform_composite_rect",
       hooks.simple.world_transform_composite},
      {L"--self-test-world-transform-affine", "world_transform_affine",
       hooks.simple.world_transform_affine},
      {L"--self-test-world-transform-blend", "world_transform_blend",
       hooks.simple.world_transform_blend},
      {L"--self-test-world-transform-transfer-mask", "world_transform_transfer_mask",
       hooks.simple.world_transform_transfer_mask},
      {L"--self-test-aegp-world-suite3", "aegp_world_suite3",
       hooks.simple.aegp_world_suite3},
      {L"--self-test-pf-batch-sampling-suite", "pf_batch_sampling_suite",
       hooks.simple.pf_batch_sampling_suite, 35,
       ",\"opaque_callable_exposed\":false"},
      {L"--self-test-pf-ae-channel-native-provider", "pf_ae_channel_native_provider",
       hooks.simple.pf_ae_channel_native_provider, 37,
       ",\"coverage_depths\":[8,16,32],\"mfr_checkouts\":2048,\"fabricated_planes\":false"},
      {kRenderOptionsCommand, "aegp_layer_render_options_suite2",
       hooks.simple.aegp_layer_render_options_suite2, 1,
       ",\"downstream_cycle_rejected\":true"},
  }};
  if (const auto exit = selftest::dispatch_simple(
          request.argc, request.argv, simple_commands.data(), simple_commands.size()))
    return {true, *exit};
  return {};
}

}  // namespace aexcompat::worker_runtime::fixed_selftests
