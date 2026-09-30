#pragma once

#include "worker_aefx_ace_suite.hpp"
#include "worker_pf_private_effect_suite.hpp"

#include <cstddef>
#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::suite_call_slot_probe {

// Same single source of truth the ACE probe target uses below: the probe and
// the implementation must name the suite identically or the probe silently
// stops overriding it (issue #1283).
inline constexpr const char* kPrivateEffectSuiteName = pf_private_effect::kSuiteName;
inline constexpr int32_t kPrivateEffectSuiteVersion3 =
    pf_private_effect::kSuiteVersion3;
inline constexpr int32_t kPrivateEffectSuiteVersion5 =
    pf_private_effect::kSuiteVersion5;
// The probe target for the implemented `AEFX ACE Suite` (issue #776) shares
// that suite's name and version, so both spellings cannot drift apart.
inline constexpr int32_t kAefxAceSuiteVersion1 = aefx_ace::kSuiteVersion1;
inline constexpr std::size_t kProbeSlotCount = 32;
// The implemented private-effect table publishes the same number of slots, so
// every slot the probe can observe is a slot that table answers - which is
// what lets one catalog entry serve either (issue #1283). Asserted here
// because this header sees both; the suite header is a leaf and cannot see
// this one without a cycle.
static_assert(pf_private_effect::kSlotCount == kProbeSlotCount,
              "the probe cannot observe a slot the published table lacks");
inline constexpr std::size_t kMaxProbeTargets = 8;
inline constexpr uint32_t kProbeExceptionBase = 0xE0427800u;
inline constexpr uint32_t kProbeExceptionTargetStride = 0x100u;

// This is an opt-in diagnostic table, not a suite implementation. When it is
// not armed the catalog falls through to the implementation for the suites
// this host implements (`AEFX ACE Suite`, `PF AE Private Effect Suite`), and
// keeps reporting the rest as missing.
// Still declared although the catalog no longer asks them directly (the
// implemented suite's provider does that internally since #1283):
// tests/native/worker_suite_registry_selftest.cpp arms the probe through the
// environment and checks that both targets report themselves available.
bool private_effect_probe3_available(void*) noexcept;
bool private_effect_probe5_available(void*) noexcept;
const void* provide_private_effect_probe3(void*) noexcept;
const void* provide_private_effect_probe5(void*) noexcept;
// Returns the trampoline table only while the probe names this target; the
// catalog entry falls through to the implementation on null.
const void* provide_aefx_ace_probe1(void*) noexcept;

std::string report_json();

}  // namespace aexcompat::worker_runtime::suite_call_slot_probe
