#include "worker_selftest_dispatch.hpp"

#include <iostream>
#include <string_view>

namespace aexcompat::worker_runtime::selftest {

std::optional<int> dispatch_aegp(int argc, wchar_t** argv,
                                 const AegpHooks& hooks) {
  if (argc != 2 || !argv || !argv[1]) return std::nullopt;
  const std::wstring_view command(argv[1]);
  bool passed{};
  int failure{};
  if (command == L"--self-test-aegp-projector-levels" && hooks.projector_levels) {
    passed = hooks.projector_levels(); failure = 71;
    std::cout << "{\"projector_levels\":\"" << (passed ? "passed" : "failed")
      << "\",\"catalog\":[\"ADBE Easy Levels\",\"ADBE Pro Levels\"],\"stream_suite\":{\"version\":7,\"slots\":22,\"size_x64\":176},\"index_zero_input\":true,\"simultaneous_stream_refs\":true,\"parameters\":[\"Input\",\"Input Black\",\"Input White\"],\"value_roundtrip\":true,\"reverse_dispose\":true,\"fail_closed\":true}\n";
  } else if (command == L"--self-test-aegp-effect-stack" && hooks.effect_stack) {
    passed = hooks.effect_stack(); failure = 70;
    std::cout << "{\"stack_mutation\":\"" << (passed ? "passed" : "failed")
      << "\",\"suite_versions\":[2,3,4],\"slots\":{\"set_flags\":5,\"reorder\":6,\"delete\":10,\"duplicate\":16},\"fail_closed\":true}\n";
  } else if (command == L"--self-test-aegp-apply-effect" && hooks.apply_effect) {
    passed = hooks.apply_effect(); failure = 69;
    std::cout << "{\"aegp_apply_effect\":\"" << (passed ? "passed" : "failed")
      << "\",\"suite_versions\":[2,3,4],\"apply_slot\":9,\"apply_offset_x64\":72,\"table_sizes_x64\":[136,136,176],\"instance_capacity\":"
      << hooks.effect_instance_capacity << ",\"lease_capacity\":"
      << hooks.effect_lease_capacity << ",\"fail_closed\":true}\n";
  } else if (command == L"--self-test-aegp-resizer-3d" && hooks.resizer_3d) {
    passed = hooks.resizer_3d(); failure = 68;
    std::cout << "{\"aegp_resizer_3d\":\"" << (passed ? "passed" : "failed")
      << "\",\"layer_slot\":38,\"layer_offset_x64\":304,\"stream_slot\":16,\"stream_offset_x64\":128,\"comp_slot\":1,\"comp_offset_x64\":8,\"item_slot\":16,\"item_offset_x64\":128,\"zoom\":1920,\"dimensions\":[1920,1080]}\n";
  } else if (command == L"--self-test-aegp-get-effect-camera" && hooks.get_effect_camera) {
    passed = hooks.get_effect_camera(); failure = 67;
    std::cout << "{\"aegp_get_effect_camera\":\"" << (passed ? "passed" : "failed")
      << "\",\"camera_slot\":3,\"camera_offset_x64\":24,\"matrix_slot\":4,\"matrix_offset_x64\":32,\"classic\":\"tested\",\"smart\":\"tested\"}\n";
  } else if (command == L"--self-test-legacy-effect-compat" && hooks.legacy_effect_compat) {
    passed = hooks.legacy_effect_compat(); failure = 39;
    std::cout << "{\"legacy_effect_compat\":\"" << (passed ? "passed" : "failed")
      << "\",\"comp_suite_version\":21,\"comp_slots\":41,\"pf_interface_slots\":5,\"helper_v1_slots\":1}\n";
  } else {
    return std::nullopt;
  }
  return passed ? 0 : failure;
}

std::optional<int> dispatch_simple(int argc, wchar_t** argv,
                                   const SimpleCommand* commands,
                                   std::size_t command_count) {
  if (argc != 2 || !argv || !argv[1] || !commands) return std::nullopt;
  const std::wstring_view requested(argv[1]);
  for (std::size_t index = 0; index < command_count; ++index) {
    const auto& command = commands[index];
    if (requested != command.command) continue;
    if (!command.run) return command.failure_exit;
    const bool passed = command.run();
    std::cout << "{\"" << command.result_key << "\":\""
              << (passed ? "passed" : "failed") << "\""
              << command.metadata_json << "}\n";
    return passed ? 0 : command.failure_exit;
  }
  return std::nullopt;
}

}  // namespace aexcompat::worker_runtime::selftest
