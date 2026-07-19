#pragma once

#include <cstdint>
#include <optional>

namespace aexcompat::worker_runtime::selftest {

struct AegpHooks {
  bool (*projector_levels)(){};
  bool (*effect_stack)(){};
  bool (*apply_effect)(){};
  bool (*resizer_3d)(){};
  bool (*get_effect_camera)(){};
  bool (*legacy_effect_compat)(){};
  uint32_t effect_instance_capacity{};
  uint32_t effect_lease_capacity{};
};

// Returns empty when argv is not one of the owned self-test commands.
// Otherwise runs the test, writes its complete protocol JSON, and returns the
// exact process exit code for the caller to pass through WorkerSession.
std::optional<int> dispatch_aegp(int argc, wchar_t** argv,
                                 const AegpHooks& hooks);

}  // namespace aexcompat::worker_runtime::selftest
