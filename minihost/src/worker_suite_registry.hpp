#pragma once

#include "suite_lease_tracker.hpp"

#include <cstddef>
#include <cstdint>
#include <mutex>
#include <string>
#include <utility>
#include <vector>

namespace aexcompat {
class TraceWriter;
}

namespace aexcompat::worker_runtime {

enum class SuiteResolveResult {
  acquired,
  rejected_bad_param,
  not_found,
};

using SuiteResolver = SuiteResolveResult (*)(
    void* context, const char* name, int32_t version, const void** suite);

class SuiteRegistry final {
 public:
  int32_t acquire(const char* name, int32_t version, const void** suite,
                  SuiteResolver resolver, void* resolver_context,
                  TraceWriter* trace_writer);
  int32_t release(const char* name, int32_t version,
                  TraceWriter* trace_writer);

  bool balanced() const;
  std::size_t live_lease_count() const;
  uint32_t live_reference_count() const;
  uint32_t acquire_count() const;
  uint32_t release_count() const;
  std::string live_summary() const;
  suite_runtime::SuiteLeaseSnapshot snapshot() const;
  std::string missing_suites_report_json() const;

 private:
  static std::string safe_missing_name(const char* name);
  void record_missing_suite(const std::string& name, int32_t version);
  int32_t reject_unknown(const char* name, int32_t version,
                         TraceWriter* trace_writer);

  suite_runtime::SuiteLeaseTracker lease_tracker_;
  mutable std::mutex missing_suites_mutex_;
  std::vector<std::pair<std::string, int32_t>> missing_suites_;
};

SuiteRegistry& suite_registry();

}  // namespace aexcompat::worker_runtime
