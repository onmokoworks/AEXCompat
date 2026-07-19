#pragma once

#include <atomic>
#include <cstdint>

namespace aexcompat::worker_runtime::pf_adv_time {

// PF Adv Item suite telemetry (issue #126 Phase D): touch/re-render counters
// recorded by worker_main's suite callbacks. Atomics because the suite can be
// driven from plug-in render threads. Lifetime: process-lifetime.
struct ItemTelemetry {
  std::atomic<uint64_t> touches{};
  std::atomic<uint64_t> rerenders{};
};
ItemTelemetry& item_telemetry();

struct VerificationHooks {
  int32_t (*acquire)(const char*, int32_t, const void**){};
  int32_t (*release)(const char*, int32_t){};
  uint32_t (*acquire_count)(){};
  uint32_t (*release_count)(){};
  bool (*leases_balanced)(){};
};

const void* suite(int32_t version) noexcept;
bool configure_verification_hooks(const VerificationHooks& hooks) noexcept;
bool verify_suite_versions();

}  // namespace aexcompat::worker_runtime::pf_adv_time
