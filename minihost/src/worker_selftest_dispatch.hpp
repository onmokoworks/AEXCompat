#pragma once

#include <cstdint>
#include <cstddef>
#include <optional>
#include <string_view>

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

struct SimpleCommand {
  std::wstring_view command;
  std::string_view result_key;
  bool (*run)(){};
  int failure_exit{1};
  // Valid JSON object members including a leading comma, or empty.
  std::string_view metadata_json;
};

std::optional<int> dispatch_simple(int argc, wchar_t** argv,
                                   const SimpleCommand* commands,
                                   std::size_t command_count);

struct HostCommand {
  std::wstring_view command;
  int argc{};
  int (*run)(int argc, wchar_t** argv){};
};

// Owns exact command admission for host-specific tests whose implementation
// must remain behind l2_main's private ABI boundary.
std::optional<int> dispatch_host(int argc, wchar_t** argv,
                                 const HostCommand* commands,
                                 std::size_t command_count);

}  // namespace aexcompat::worker_runtime::selftest
