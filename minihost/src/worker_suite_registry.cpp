#include "worker_suite_registry.hpp"

#include "trace_writer.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <iostream>
#include <iomanip>
#include <sstream>

namespace aexcompat::worker_runtime {
namespace {

constexpr std::size_t kMaxMissingSuites = 16;
constexpr std::size_t kMaxSuiteNameBytes = 96;
constexpr std::size_t kMaxSuiteTimeline = 65536;
thread_local const char* g_suite_selector = "HOST";

struct SuiteNameCopy {
  std::array<char, kMaxSuiteNameBytes + 1> text{};
  std::size_t length{};
  bool readable{};
  bool terminated{};
};

SuiteNameCopy copy_bounded_suite_name(const char* source) noexcept {
  SuiteNameCopy copy;
  if (!source) return copy;
  __try {
    for (; copy.length < kMaxSuiteNameBytes; ++copy.length) {
      const unsigned char character = static_cast<unsigned char>(source[copy.length]);
      if (character == '\0') {
        copy.terminated = true;
        break;
      }
      copy.text[copy.length] = character >= 0x20 && character <= 0x7e
          ? static_cast<char>(character) : '?';
    }
    copy.readable = true;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    copy.readable = false;
  }
  copy.text[copy.length] = '\0';
  return copy;
}

std::string escape_json(const std::string& input) {
  std::ostringstream escaped;
  for (const unsigned char character : input) {
    switch (character) {
      case '\\': escaped << "\\\\"; break;
      case '"': escaped << "\\\""; break;
      case '\b': escaped << "\\b"; break;
      case '\f': escaped << "\\f"; break;
      case '\n': escaped << "\\n"; break;
      case '\r': escaped << "\\r"; break;
      case '\t': escaped << "\\t"; break;
      default:
        if (character < 0x20) {
          escaped << "\\u00" << std::hex << std::setw(2) << std::setfill('0')
                  << static_cast<int>(character) << std::dec;
        } else {
          escaped << static_cast<char>(character);
        }
    }
  }
  return escaped.str();
}

}  // namespace

int32_t SuiteRegistry::acquire(const char* name, int32_t version,
                               const void** suite, SuiteResolver resolver,
                               void* resolver_context,
                               TraceWriter* trace_writer) {
  const SuiteNameCopy owned_name = copy_bounded_suite_name(name);
  const auto record = [&](int32_t result) {
    std::lock_guard<std::mutex> lock(timeline_mutex_);
    if (suite_timeline_.size() >= kMaxSuiteTimeline) return;
    suite_timeline_.push_back({static_cast<uint32_t>(suite_timeline_.size()), true,
        std::string(owned_name.text.data(), owned_name.length), version,
        g_suite_selector ? g_suite_selector : "HOST", result});
  };
  if (!suite) { record(4); return 4; }
  *suite = nullptr;
  if (!name || !resolver || !owned_name.readable || !owned_name.terminated) {
    record(4);
    return 4;
  }
  const char* const safe_name = owned_name.text.data();

  switch (resolver(resolver_context, safe_name, version, suite)) {
    case SuiteResolveResult::acquired:
      lease_tracker_.acquire(safe_name, version);
      if (trace_writer) trace_writer->suite_acquire(safe_name, version, true);
      record(0);
      return 0;
    case SuiteResolveResult::rejected_bad_param:
      *suite = nullptr;
      record(4);
      return 4;
    case SuiteResolveResult::not_found:
      *suite = nullptr;
      {
        const int32_t result = reject_unknown(safe_name, version, trace_writer);
        record(result);
        return result;
      }
  }
  *suite = nullptr;
  record(4);
  return 4;
}

int32_t SuiteRegistry::release(const char* name, int32_t version,
                               TraceWriter* trace_writer) {
  const SuiteNameCopy owned_name = copy_bounded_suite_name(name);
  const char* const safe_name = owned_name.text.data();
  const bool valid_name = owned_name.readable && owned_name.terminated;
  const bool released = valid_name && lease_tracker_.release(safe_name, version);
  {
    std::lock_guard<std::mutex> lock(timeline_mutex_);
    if (suite_timeline_.size() < kMaxSuiteTimeline)
      suite_timeline_.push_back({static_cast<uint32_t>(suite_timeline_.size()), false,
          std::string(owned_name.text.data(), owned_name.length), version,
          g_suite_selector ? g_suite_selector : "HOST", released ? 0 : 1});
  }
  if (trace_writer && valid_name && owned_name.length != 0)
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
  if (trace_writer && !safe_name.empty()) {
    trace_writer->suite_acquire(safe_name, std::max<int32_t>(version, 0), false);
    // An unknown suite is an unimplemented host capability (issue #17). Record
    // it as an explicit trace error so compatibility gaps surface as diagnostics
    // rather than only a granted=false acquire. Unsafe names are dropped by the
    // writer's own safe_string guard.
    trace_writer->error("unimplemented_suite", safe_name, /*unimplemented=*/true);
  }
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

std::string SuiteRegistry::suite_timeline_report_json() const {
  std::lock_guard<std::mutex> lock(timeline_mutex_);
  std::ostringstream json;
  json << ",\"suite_timeline\":[";
  for (std::size_t index = 0; index < suite_timeline_.size(); ++index) {
    if (index != 0) json << ',';
    const auto& event = suite_timeline_[index];
    json << "{\"sequence\":" << event.sequence
         << ",\"action\":\"" << (event.acquire ? "acquire" : "release")
         << "\",\"name\":\"" << escape_json(event.name)
         << "\",\"version\":" << event.version
         << ",\"selector\":\"" << escape_json(event.selector)
         << "\",\"result\":" << event.result << '}';
  }
  json << ']';
  return json.str();
}

SuiteRegistry& suite_registry() {
  static SuiteRegistry registry;
  return registry;
}

const char* set_suite_timeline_selector(const char* selector) noexcept {
  const char* previous = g_suite_selector;
  g_suite_selector = selector;
  return previous;
}

}  // namespace aexcompat::worker_runtime
