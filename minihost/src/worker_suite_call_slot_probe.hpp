#pragma once

#include "worker_aefx_ace_suite.hpp"

#include <cstddef>
#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::suite_call_slot_probe {

inline constexpr char kPrivateEffectSuiteName[] = "PF AE Private Effect Suite";
inline constexpr int32_t kPrivateEffectSuiteVersion3 = 3;
inline constexpr int32_t kPrivateEffectSuiteVersion5 = 5;
// The probe target for the implemented `AEFX ACE Suite` (issue #776) shares
// that suite's name and version, so both spellings cannot drift apart.
inline constexpr int32_t kAefxAceSuiteVersion1 = aefx_ace::kSuiteVersion1;
inline constexpr std::size_t kProbeSlotCount = 32;
inline constexpr std::size_t kMaxProbeTargets = 8;
inline constexpr uint32_t kProbeExceptionBase = 0xE0427800u;
inline constexpr uint32_t kProbeExceptionTargetStride = 0x100u;

// This is an opt-in diagnostic table, not a suite implementation. Normal
// workers keep reporting the probed suites as missing.
bool private_effect_probe3_available(void*) noexcept;
bool private_effect_probe5_available(void*) noexcept;
const void* provide_private_effect_probe3(void*) noexcept;
const void* provide_private_effect_probe5(void*) noexcept;
// Returns the trampoline table only while the probe names this target; the
// catalog entry falls through to the implementation on null.
const void* provide_aefx_ace_probe1(void*) noexcept;

std::string report_json();

}  // namespace aexcompat::worker_runtime::suite_call_slot_probe
