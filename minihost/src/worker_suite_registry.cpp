#include "worker_suite_registry.hpp"

#include "trace_writer.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <iostream>
#include <sstream>

namespace aexcompat::worker_runtime {
namespace {

constexpr std::size_t kMaxMissingSuites = 16;
constexpr std::size_t kMaxSuiteNameBytes = 96;

bool copy_bounded_suite_name(
    const char* source,
    std::array<char, kMaxSuiteNameBytes + 1>& destination) noexcept {
  if (!source) return false;
  __try {
    for (std::size_t index = 0; index <= kMaxSuiteNameBytes; ++index) {
      const char character = source[index];
      if (character == '\0') {
        destination[index] = '\0';
        return true;
      }
      if (index == kMaxSuiteNameBytes) return false;
      destination[index] = character;
    }
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
  return false;
}

}  // namespace

int32_t SuiteRegistry::acquire(const char* name, int32_t version,
                               const void** suite, SuiteResolver resolver,
                               void* resolver_context,
                               TraceWriter* trace_writer) {
  if (!suite) return 4;
  *suite = nullptr;
  if (!name || !resolver) return 4;
  std::array<char, kMaxSuiteNameBytes + 1> owned_name{};
  if (!copy_bounded_suite_name(name, owned_name)) return 4;
  const char* const safe_name = owned_name.data();

  switch (resolver(resolver_context, safe_name, version, suite)) {
    case SuiteResolveResult::acquired:
      lease_tracker_.acquire(safe_name, version);
      if (trace_writer) trace_writer->suite_acquire(safe_name, version, true);
      return 0;
    case SuiteResolveResult::rejected_bad_param:
      *suite = nullptr;
      return 4;
    case SuiteResolveResult::not_found:
      *suite = nullptr;
      return reject_unknown(safe_name, version, trace_writer);
  }
  *suite = nullptr;
  return 4;
}

int32_t SuiteRegistry::release(const char* name, int32_t version,
                               TraceWriter* trace_writer) {
  std::array<char, kMaxSuiteNameBytes + 1> owned_name{};
  if (!copy_bounded_suite_name(name, owned_name)) return 1;
  const char* const safe_name = owned_name.data();
  const bool released = lease_tracker_.release(safe_name, version);
  if (trace_writer)
    trace_writer->suite_release(safe_name, std::max<int32_t>(version, 0), released);
  return released ? 0 : 1;
}

std::string SuiteRegistry::safe_missing_name(const char* name) {
  std::string safe_name;
  if (name) {
    for (std::size_t index = 0;
         index < kMaxSuiteNameBytes && name[index]; ++index) {
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
