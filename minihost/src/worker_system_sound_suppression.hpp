#pragma once

#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::system_sound_suppression {

// Redirects the process-local USER32 system-alert entry to a silent success
// result. Dialogs remain intact for the broker's private-desktop sweep.
bool install(std::string& diagnostic);

// Exercises the production redirect without reaching Windows audio handling.
bool selftest();

std::uint64_t intercepted_calls();

}  // namespace aexcompat::worker_runtime::system_sound_suppression
