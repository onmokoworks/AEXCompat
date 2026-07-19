#include "worker_parameter_selftest_routing.hpp"

#include "parameter_animation_transport.hpp"

#include <filesystem>
#include <sstream>
#include <string_view>
#include <vector>

namespace aexcompat::worker_runtime::parameter_selftests {

Result dispatch(const Request& request, const Hooks& hooks) {
  if (!request.argv) return {};

  if (request.argc == 2 && request.argv[1] &&
      std::wstring_view(request.argv[1]) ==
          L"--self-test-aegp-keyframe-mutations") {
    const bool passed = hooks.verify_keyframe_mutations(
        hooks.keyframe_abi_wiring_valid());
    std::ostringstream output;
    output << "{\"aegp_keyframe_mutations\":\""
           << (passed ? "passed" : "failed")
           << "\",\"mutations\":" << *hooks.keyframe_mutations
           << ",\"ownership_rejections\":"
           << *hooks.invalid_keyframe_operations
           << ",\"lifetimes_balanced\":"
           << (hooks.mask_lifetimes_balanced() ? "true" : "false") << "}\n";
    return {true, passed ? 0 : 1, output.str()};
  }

  if (request.argc == 3 && request.argv[1] && request.argv[2] &&
      std::wstring_view(request.argv[1]) ==
          L"--self-test-parameter-animation-sidecar") {
    std::vector<aexcompat::parameter_animation::ParameterTimeline> timelines;
    const bool accepted = aexcompat::parameter_animation::load_parameter_animation(
        std::filesystem::path(request.argv[2]), timelines);
    std::ostringstream output;
    output << "{\"parameter_animation_sidecar\":\""
           << (accepted ? "accepted" : "rejected")
           << "\",\"timelines\":" << timelines.size() << "}\n";
    return {true, accepted ? 0 : 3, output.str()};
  }

  return {};
}

}  // namespace aexcompat::worker_runtime::parameter_selftests
