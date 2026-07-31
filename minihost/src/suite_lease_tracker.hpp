#pragma once

#include <cstddef>
#include <cstdint>
#include <map>
#include <mutex>
#include <string>
#include <utility>
#include <vector>

namespace aexcompat::suite_runtime {

struct SuiteLeaseSnapshot {
  uint32_t acquires{};
  uint32_t releases{};
  std::vector<std::pair<std::pair<std::string, int32_t>, uint32_t>> live_leases;
};

class SuiteLeaseTracker {
 public:
  void acquire(const char* name, int32_t version);
  bool release(const char* name, int32_t version);
  bool balanced() const;
  std::size_t live_lease_count() const;
  uint32_t live_reference_count() const;
  uint32_t acquire_count() const;
  uint32_t release_count() const;
  std::string live_summary() const;
  SuiteLeaseSnapshot snapshot() const;
  uint32_t force_release_all() noexcept;

 private:
  mutable std::mutex mutex_;
  std::map<std::pair<std::string, int32_t>, uint32_t> leases_;
  uint32_t acquires_{};
  uint32_t releases_{};
};

}  // namespace aexcompat::suite_runtime
