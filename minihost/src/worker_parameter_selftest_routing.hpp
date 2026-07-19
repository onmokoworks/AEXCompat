#pragma once

#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::parameter_selftests {

struct Request {
  int argc{};
  wchar_t** argv{};
};

struct Hooks {
  bool (*verify_keyframe_mutations)(bool){};
  bool (*keyframe_abi_wiring_valid)(){};
  const uint32_t* keyframe_mutations{};
  const uint32_t* invalid_keyframe_operations{};
  bool (*mask_lifetimes_balanced)(){};
};

struct Result {
  bool handled{};
  int exit_code{};
  std::string output;
};

// Owns exact arity, protocol output, and exit codes for parameter/keyframe
// self-tests. Private host state remains behind explicit read-only hooks.
Result dispatch(const Request& request, const Hooks& hooks);

}  // namespace aexcompat::worker_runtime::parameter_selftests
