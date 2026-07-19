#include "worker_suite_registry.hpp"

#include "trace_writer.hpp"

#include <algorithm>
#include <cctype>
#include <iostream>
#include <sstream>

namespace aexcompat::worker_runtime {
namespace {

constexpr std::size_t kMaxMissingSuites = 16;
constexpr std::size_t kMaxSuiteNameBytes = 96;

}  // namespace

int32_t SuiteRegistry::acquire(const char* name, int32_t version,
                               const void** suite, SuiteResolver resolver,
                               void* resolver_context,
                               TraceWriter* trace_writer) {
  if (!suite) return 4;
  *suite = nullptr;
  if (!name || !resolver) return 4;

  switch (resolver(resolver_context, name, version, suite)) {
    case SuiteResolveResult::acquired:
      lease_tracker_.acquire(name, version);
      if (trace_writer) trace_writer->suite_acquire(name, version, true);
      return 0;
    case SuiteResolveResult::rejected_bad_param:
      *suite = nullptr;
      return 4;
    case SuiteResolveResult::not_found:
      *suite = nullptr;
      return reject_unknown(name, version, trace_writer);
  }
  *suite = nullptr;
  return 4;
}

int32_t SuiteRegistry::release(const char* name, int32_t version,
                               TraceWriter* trace_writer) {
  const bool released = lease_tracker_.release(name, version);
  if (trace_writer && name)
    trace_writer->suite_release(name, std::max<int32_t>(version, 0), released);
  return released ? 0 : 1;
}

std::string SuiteRegistry::safe_missing_name(const char* name) {
  std::string safe_name;
  if (name) {
    for (std::size_t index = 0;
         name[index] && index < kMaxSuiteNameBytes; ++index) {
      const unsigned char character = static_cast<unsigned char>(name[index]);
      safe_name.push_back(character >= 0x20 && character <= 0x7e
                              ? name[index]
                              : '?');
    }
  }
  return safe_name;
}

void SuiteRegistry::record_missing_suite(const std::string& name,
                                         int32_t version) {
  const bool valid_name = !name.empty() && name.size() <= kMaxSuiteNameBytes &&
      std::all_of(name.begin(), name.end(), [](unsigned char character) {
        return std::isalnum(character) || character == ' ' || character == '.' ||
            character == '_' || character == '-';
      });
  if (!valid_name || version <= 0) return;
  std::lock_guard<std::mutex> lock(missing_suites_mutex_);
  const auto entry = std::make_pair(name, version);
  if (std::find(missing_suites_.begin(), missing_suites_.end(), entry) ==
          missing_suites_.end() &&
      missing_suites_.size() < kMaxMissingSuites) {
    missing_suites_.push_back(entry);
  }
}

int32_t SuiteRegistry::reject_unknown(const char* name, int32_t version,
                                      TraceWriter* trace_writer) {
  const std::string safe_name = safe_missing_name(name);
  record_missing_suite(safe_name, version);
  if (trace_writer && !safe_name.empty())
    trace_writer->suite_acquire(safe_name, std::max<int32_t>(version, 0), false);
  std::cerr << "stage:suite_acquire_failed name=" << safe_name
            << " version=" << version << "\n" << std::flush;
  return 1;
}

bool SuiteRegistry::balanced() const { return lease_tracker_.balanced(); }
std::size_t SuiteRegistry::live_lease_count() const {
  return lease_tracker_.live_lease_count();
}
uint32_t SuiteRegistry::live_reference_count() const {
  return lease_tracker_.live_reference_count();
}
uint32_t SuiteRegistry::acquire_count() const {
  return lease_tracker_.acquire_count();
}
uint32_t SuiteRegistry::release_count() const {
  return lease_tracker_.release_count();
}
std::string SuiteRegistry::live_summary() const {
  return lease_tracker_.live_summary();
}
suite_runtime::SuiteLeaseSnapshot SuiteRegistry::snapshot() const {
  return lease_tracker_.snapshot();
}

std::string SuiteRegistry::missing_suites_report_json() const {
  std::lock_guard<std::mutex> lock(missing_suites_mutex_);
  std::ostringstream json;
  json << ",\"missing_suites\":[";
  for (std::size_t index = 0; index < missing_suites_.size(); ++index) {
    if (index != 0) json << ',';
    json << "{\"name\":\"" << missing_suites_[index].first
         << "\",\"version\":" << missing_suites_[index].second << '}';
  }
  json << ']';
  return json.str();
}

SuiteRegistry& suite_registry() {
  static SuiteRegistry registry;
  return registry;
}

}  // namespace aexcompat::worker_runtime
