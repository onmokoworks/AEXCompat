#include "suite_lease_tracker.hpp"

#include <algorithm>
#include <sstream>

namespace aexcompat::suite_runtime {

void SuiteLeaseTracker::acquire(const char* name, int32_t version) {
  std::lock_guard<std::mutex> lock(mutex_);
  ++leases_[{name, version}];
  ++acquires_;
}

bool SuiteLeaseTracker::release(const char* name, int32_t version) {
  if (!name) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  const auto found = leases_.find({name, version});
  if (found == leases_.end() || found->second == 0) return false;
  --found->second;
  ++releases_;
  return true;
}

bool SuiteLeaseTracker::balanced() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return acquires_ == releases_ &&
      std::all_of(leases_.begin(), leases_.end(),
                  [](const auto& lease) { return lease.second == 0; });
}

std::size_t SuiteLeaseTracker::live_lease_count() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return static_cast<std::size_t>(std::count_if(
      leases_.begin(), leases_.end(),
      [](const auto& lease) { return lease.second != 0; }));
}

uint32_t SuiteLeaseTracker::live_reference_count() const {
  std::lock_guard<std::mutex> lock(mutex_);
  uint32_t count = 0;
  for (const auto& lease : leases_) count += lease.second;
  return count;
}

uint32_t SuiteLeaseTracker::acquire_count() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return acquires_;
}

uint32_t SuiteLeaseTracker::release_count() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return releases_;
}

std::string SuiteLeaseTracker::live_summary() const {
  std::lock_guard<std::mutex> lock(mutex_);
  std::ostringstream summary;
  for (const auto& [key, count] : leases_) {
    if (count == 0) continue;
    if (summary.tellp() > 0) summary << ';';
    summary << key.first << '@' << key.second << '=' << count;
  }
  return summary.str();
}

SuiteLeaseSnapshot SuiteLeaseTracker::snapshot() const {
  std::lock_guard<std::mutex> lock(mutex_);
  SuiteLeaseSnapshot result;
  result.acquires = acquires_;
  result.releases = releases_;
  for (const auto& lease : leases_)
    if (lease.second != 0) result.live_leases.push_back(lease);
  return result;
}

uint32_t SuiteLeaseTracker::force_release_all() noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  uint32_t released = 0;
  for (const auto& entry : leases_) released += entry.second;
  releases_ += released;
  leases_.clear();
  return released;
}

}  // namespace aexcompat::suite_runtime
