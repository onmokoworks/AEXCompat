#pragma once

#include <cstdint>

namespace aexcompat::worker_runtime::dvacore_main_thread {

enum class Action { AlreadyMain, Register, OtherMain, InvalidThread };

// Avoid replacing an already-observed main thread registered by a plug-in or
// another runtime. An unregistered dvacore reports ID zero. The decision is
// not an atomic claim: a concurrent third-party registration after this read
// cannot be prevented through dvacore's public API.
inline Action decide(uint32_t registered_id, uint32_t dispatch_id) noexcept {
  if (dispatch_id == 0) return Action::InvalidThread;
  if (registered_id == dispatch_id) return Action::AlreadyMain;
  if (registered_id != 0) return Action::OtherMain;
  return Action::Register;
}

}  // namespace aexcompat::worker_runtime::dvacore_main_thread
