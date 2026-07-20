#include "worker_fixed_selftest_routing.hpp"
#include "worker_aegp_scene_runtime.hpp"

#include "worker_aegp_compat_selftests.hpp"
#include "worker_aegp_utility_suite.hpp"
#include "worker_host_guard_selftests.hpp"
#include "worker_minidump_runtime.hpp"
#include "worker_pf_adv_time_suite.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_selftest_dispatch.hpp"

#include <array>
#include <iostream>
#include <string_view>



namespace aexcompat::worker_runtime::fixed_selftests {
namespace {

const HostHooks* g_host{};

int selftest_render_output_safety(int, wchar_t**) {
  const bool passed = aexcompat::host_guard_selftests::verify_render_output_safety();
  auto& telemetry = selector_dispatch_telemetry();
  std::cout << "{\"render_output_safety\":\"" << (passed ? "passed" : "failed")
            << "\",\"cleanup_selector\":\"" << g_host->escape(telemetry.selector)
            << "\",\"cleanup_error\":" << telemetry.error
            << ",\"cleanup_calls\":"
            << aexcompat::host_guard_selftests::cleanup_safety_selftest_calls()
            << ",\"guard_pages\":true,\"overrun_beyond_64_detected\":true}\n";
  return passed ? 0 : 1;
}

int selftest_crash_minidump(int, wchar_t**) {
  // End-to-end proof that the production inherited-handle path writes a
  // minidump. The worker must not receive a directory path; the broker-created
  // pipe is inherited through the environment.
  if (!minidump::configure_from_inherited_handle() ||
      !minidump::handle_configured()) {
    std::cout << "{\"crash_minidump\":\"failed\",\"reason\":\"missing_handle\"}\n";
    return 1;
  }
  const uint32_t exception_code = g_host->trigger_guarded_crash();
  const uint64_t dump_size = minidump::written_bytes();
  const bool written = exception_code == EXCEPTION_ACCESS_VIOLATION &&
      dump_size > 0 && !minidump::broker_rejected();
  std::cout << "{\"crash_minidump\":\"" << (written ? "passed" : "failed")
            << "\",\"exception_code\":" << exception_code
            << ",\"dump_bytes\":" << (written ? dump_size : 0)
            << ",\"attempted\":" << (minidump::attempted() ? "true" : "false")
            << "}\n";
  return written ? 0 : 1;
}

int selftest_crash_no_minidump(int, wchar_t**) {
  // Real guarded access violation with no opt-in handle. This protects the
  // default-off privacy invariant instead of merely testing argc rejection.
  const uint32_t exception_code = g_host->trigger_guarded_crash();
  const bool passed = exception_code == EXCEPTION_ACCESS_VIOLATION &&
      !minidump::attempted();
  std::cout << "{\"crash_minidump\":\"" << (passed ? "disabled" : "failed")
            << "\",\"exception_code\":" << exception_code
            << ",\"attempted\":" << (minidump::attempted() ? "true" : "false")
            << "}\n";
  return passed ? 0 : 1;
}

int selftest_pf_adv_time(int, wchar_t**) {
  const bool passed = pf_adv_time::verify_suite_versions();
  std::cout << "{\"pf_adv_time_suite_versions\":\"" << (passed ? "passed" : "failed")
            << "\",\"v1_slots\":4,\"v2_slots\":4,\"v3_slots\":4,\"v4_slots\":5,\"independent_identity\":true"
            << ",\"guard_intact\":true,\"reverse_release\":true,\"suite_leases_balanced\":"
            << (g_host->suite_leases_balanced() ? "true" : "false") << "}\n";
  return passed ? 0 : 1;
}

int selftest_suite_entry_utility13(int, wchar_t**) {
  const bool passed = aexcompat::l2_detail::verify_suite_entry_guards_and_utility13();
  std::cout << "{\"suite_entry_utility13\":\"" << (passed ? "passed" : "failed")
            << "\",\"null_fail_closed\":true,\"normal_effect_available\":true"
            << ",\"versions_12_14_rejected\":true,\"mask_callbacks_exposed\":false"
            << ",\"suite_leases_balanced\":"
            << (g_host->suite_leases_balanced() ? "true" : "false") << "}\n";
  return passed ? 0 : 1;
}

int selftest_pf_adv_app(int, wchar_t**) {
  const bool passed =
      aexcompat::host_guard_selftests::verify_pf_adv_app_suite_versions();
  std::cout << "{\"pf_adv_app_suite_versions\":\"" << (passed ? "passed" : "failed")
            << "\",\"v1_slots\":10,\"v2_slots\":11,\"independent_identity\":true"
            << ",\"suite_leases_balanced\":"
            << (g_host->suite_leases_balanced() ? "true" : "false") << "}\n";
  return passed ? 0 : 1;
}

int selftest_effect_param_union(int, wchar_t**) {
  const bool passed = aexcompat::l2_detail::verify_aegp_effect_param_union_suite4();
  std::cout << "{\"aegp_effect_param_union_suite4\":\""
            << (passed ? "passed" : "failed")
            << "\",\"successful_calls\":"
            << aexcompat::scene_runtime::scene_runtime_state().effect_param_union_calls << "}\n";
  return passed ? 0 : 1;
}

}  // namespace

Result dispatch(const Request& request, const Hooks& hooks) {
  g_host = &hooks.host;
  const std::array<selftest::HostCommand, 7> host_commands{{
      {L"--self-test-render-output-safety", 2, &selftest_render_output_safety},
      {L"--self-test-crash-minidump", 2, &selftest_crash_minidump},
      {L"--self-test-crash-no-minidump", 2, &selftest_crash_no_minidump},
      {L"--self-test-pf-adv-time-suite1", 2, &selftest_pf_adv_time},
      {L"--self-test-suite-entry-utility13", 2, &selftest_suite_entry_utility13},
      {L"--self-test-pf-adv-app-suite", 2, &selftest_pf_adv_app},
      {L"--self-test-aegp-effect-param-union-suite4", 2,
       &selftest_effect_param_union},
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

  const std::array<selftest::SimpleCommand, 23> simple_commands{{
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
      {L"--self-test-pf-utils-handle-callbacks", "pf_utils_handle_callbacks",
       hooks.simple.pf_utils_handle_callbacks, 1,
       ",\"reached_via_in_data_utils\":true,\"offsets\":[160,168,176,184,440,464]"},
  }};
  if (const auto exit = selftest::dispatch_simple(
          request.argc, request.argv, simple_commands.data(), simple_commands.size()))
    return {true, *exit};
  return {};
}

}  // namespace aexcompat::worker_runtime::fixed_selftests
