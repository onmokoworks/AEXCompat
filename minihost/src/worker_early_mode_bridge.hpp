#pragma once

#include "l2_mode_execution.hpp"
#include "worker_parameter_execution.hpp"

#include <array>
#include <cstddef>
#include <string>

namespace aexcompat::worker_runtime {
class WorkerSession;
}

namespace aexcompat::l2_detail {

// Value bridge between worker_main's early-mode locals and the l2mode hook
// ABI; the adapters live in worker_early_mode_bridge.cpp (issue #165).
struct EarlyModeBridge {
  worker_runtime::parameter_execution::EffectEntry entry{};
  std::array<std::byte, 408>* input{};
  std::array<std::byte, 408>* output{};
  aexcompat::worker_runtime::WorkerSession* session{};
  const std::string* about_message{};
};

const aexcompat::l2mode::Hooks& early_mode_hooks();

}  // namespace aexcompat::l2_detail
