#pragma once

#include <cstddef>
#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::suite_call_slot_probe {

inline constexpr char kPrivateEffectSuiteName[] = "PF AE Private Effect Suite";
inline constexpr int32_t kPrivateEffectSuiteVersion3 = 3;
inline constexpr int32_t kPrivateEffectSuiteVersion5 = 5;
inline constexpr std::size_t kProbeSlotCount = 32;
inline constexpr std::size_t kMaxProbeTargets = 8;
inline constexpr uint32_t kProbeExceptionBase = 0xE0427800u;
inline constexpr uint32_t kProbeExceptionTargetStride = 0x100u;

// This is an opt-in diagnostic table, not a suite implementation. Normal
// workers keep reporting the private suite as missing.
bool private_effect_probe3_available(void*) noexcept;
bool private_effect_probe5_available(void*) noexcept;
const void* provide_private_effect_probe3(void*) noexcept;
const void* provide_private_effect_probe5(void*) noexcept;

std::string report_json();

}  // namespace aexcompat::worker_runtime::suite_call_slot_probe
