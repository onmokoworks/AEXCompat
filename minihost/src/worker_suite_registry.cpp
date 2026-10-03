#include "worker_suite_registry.hpp"

#include "trace_writer.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <iostream>
#include <iomanip>
#include <limits>
#include <sstream>
#include <string_view>

namespace aexcompat::worker_runtime {
namespace {

constexpr std::size_t kMaxMissingSuites = 16;
constexpr std::size_t kMaxUnsupportedSuiteCalls = 32;
constexpr std::size_t kMaxSuiteNameBytes = 96;
constexpr std::size_t kMaxTelemetrySuiteNameBytes = 64;
constexpr std::size_t kMaxSuiteSelectorBytes = 64;
constexpr std::size_t kMaxSuiteTimeline = 512;
constexpr std::size_t kMaxFailedAcquireKeys = 512;
constexpr int32_t kMaxSuiteVersion = 65535;
constexpr uint32_t kMaxUnsupportedSuiteSlot = 1023;
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

bool valid_schema_text(const std::string& text, std::size_t maximum,
                       bool require_alpha_first) {
  return !text.empty() && text.size() <= maximum &&
      (!require_alpha_first ||
       std::isalpha(static_cast<unsigned char>(text.front()))) &&
      std::isalnum(static_cast<unsigned char>(text.back())) &&
      std::all_of(text.begin(), text.end(), [](unsigned char character) {
        return std::isalnum(character) || character == ' ' ||
            character == '_' || character == '-' || character == '.';
      });
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

struct UnsupportedSuiteDescriptor {
  const char* name{};
  int32_t version{};
};

UnsupportedSuiteDescriptor unsupported_suite_descriptor(
    UnsupportedSuiteId suite) noexcept {
  switch (suite) {
    case UnsupportedSuiteId::aegp_proj_9: return {"AEGP Proj Suite", 9};
    case UnsupportedSuiteId::aegp_item_14: return {"AEGP Item Suite", 14};
    case UnsupportedSuiteId::aegp_item_13: return {"AEGP Item Suite", 13};
    case UnsupportedSuiteId::aegp_item_11: return {"AEGP Item Suite", 11};
    case UnsupportedSuiteId::aegp_item_10: return {"AEGP Item Suite", 10};
    case UnsupportedSuiteId::aegp_item_3: return {"AEGP Item Suite", 3};
    case UnsupportedSuiteId::aegp_comp_25: return {"AEGP Comp Suite", 25};
    case UnsupportedSuiteId::aegp_comp_26: return {"AEGP Comp Suite", 26};
    case UnsupportedSuiteId::aegp_comp_21: return {"AEGP Comp Suite", 21};
    case UnsupportedSuiteId::aegp_comp_9: return {"AEGP Comp Suite", 9};
    case UnsupportedSuiteId::aegp_comp_4: return {"AEGP Comp Suite", 4};
    case UnsupportedSuiteId::aegp_layer_15: return {"AEGP Layer Suite", 15};
    case UnsupportedSuiteId::aegp_layer_11: return {"AEGP Layer Suite", 11};
    case UnsupportedSuiteId::aegp_layer_14: return {"AEGP Layer Suite", 14};
    case UnsupportedSuiteId::aegp_layer_13: return {"AEGP Layer Suite", 13};
    case UnsupportedSuiteId::aegp_layer_5: return {"AEGP Layer Suite", 5};
    case UnsupportedSuiteId::aegp_layer_8: return {"AEGP Layer Suite", 8};
    case UnsupportedSuiteId::aegp_collection_2: return {"AEGP Collection Suite", 2};
    case UnsupportedSuiteId::aegp_effect_4: return {"AEGP Effect Suite", 4};
    case UnsupportedSuiteId::aegp_effect_2: return {"AEGP Effect Suite", 2};
    case UnsupportedSuiteId::aegp_effect_3: return {"AEGP Effect Suite", 3};
    case UnsupportedSuiteId::aegp_effect_1: return {"AEGP Effect Suite", 1};
    case UnsupportedSuiteId::aegp_stream_11: return {"AEGP Stream Suite", 11};
    case UnsupportedSuiteId::aegp_stream_7: return {"AEGP Stream Suite", 7};
    case UnsupportedSuiteId::aegp_stream_8: return {"AEGP Stream Suite", 8};
    case UnsupportedSuiteId::aegp_stream_9: return {"AEGP Stream Suite", 9};
    case UnsupportedSuiteId::aegp_stream_4: return {"AEGP Stream Suite", 4};
    case UnsupportedSuiteId::aegp_iterate_1: return {"AEGP Iterate Suite", 1};
    case UnsupportedSuiteId::aegp_keyframe_5: return {"AEGP Keyframe Suite", 5};
    case UnsupportedSuiteId::aegp_keyframe_4: return {"AEGP Keyframe Suite", 4};
    case UnsupportedSuiteId::aegp_utility_5: return {"AEGP Utility Suite", 5};
    case UnsupportedSuiteId::aegp_utility_7: return {"AEGP Utility Suite", 7};
    case UnsupportedSuiteId::aegp_utility_13: return {"AEGP Utility Suite", 13};
    case UnsupportedSuiteId::pf_ae_adv_app_1: return {"PF AE Adv App Suite", 1};
    case UnsupportedSuiteId::pf_ae_adv_app_2: return {"PF AE Adv App Suite", 2};
    case UnsupportedSuiteId::drawbot_supplier_1: return {"DRAWBOT Supplier Suite", 1};
    case UnsupportedSuiteId::drawbot_surface_2: return {"DRAWBOT Surface Suite", 2};
    case UnsupportedSuiteId::drawbot_path_1: return {"DRAWBOT Path Suite", 1};
    case UnsupportedSuiteId::pf_effect_custom_ui_overlay_theme_1:
      return {"PF Effect Custom UI Overlay Theme Suite", 1};
    case UnsupportedSuiteId::aegp_dynamic_stream_2:
      return {"AEGP Dynamic Stream Suite", 2};
    case UnsupportedSuiteId::pf_batch_sampling_1:
      return {"PF Batch Sampling Suite", 1};
    case UnsupportedSuiteId::aefx_ace_1: return {"AEFX ACE Suite", 1};
    // Versions 3, 5 and 6 share one table, so this names the lowest rather
    // than the version the caller asked for (issue #1283).
    case UnsupportedSuiteId::pf_ae_private_effect:
      return {"PF AE Private Effect Suite", 3};
    case UnsupportedSuiteId::ae_timecode_helper_1:
      return {"AE Timecode Helper Suite", 1};
    case UnsupportedSuiteId::bee_av_layer_vtable:
      return {"BEE_AVLayer vtable", 1};
    case UnsupportedSuiteId::bee_item_vtable: return {"BEE_CompItem vtable", 1};
    case UnsupportedSuiteId::bee_footage_item_vtable:
      return {"BEE_FootageItem vtable", 1};
    case UnsupportedSuiteId::bee_project_vtable:
      return {"BEE_Project vtable", 1};
    case UnsupportedSuiteId::pf_world_vtable: return {"PF_World vtable", 1};
  }
  return {};
}

}  // namespace

int32_t SuiteRegistry::acquire(const char* name, int32_t version,
                               const void** suite, SuiteResolver resolver,
                               void* resolver_context,
                               TraceWriter* trace_writer) {
  const SuiteNameCopy owned_name = copy_bounded_suite_name(name);
  const auto record = [&](int32_t result) {
    record_suite_timeline(true, owned_name.text.data(), owned_name.length,
                          owned_name.readable && owned_name.terminated,
                          version, result);
  };
  if (!suite) { record(4); return 4; }
  *suite = nullptr;
  if (!name || !resolver || !owned_name.readable || !owned_name.terminated ||
      version <= 0 || version > kMaxSuiteVersion) {
    record(4);
    return 4;
  }
  const char* const safe_name = owned_name.text.data();

  switch (resolver(resolver_context, safe_name, version, suite)) {
    case SuiteResolveResult::acquired:
      lease_tracker_.acquire(safe_name, version);
      record_successful_acquire(safe_name, version);
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
  // Some plug-ins pair every AcquireSuite attempt with ReleaseSuite, including
  // versions the host reported as unavailable. Preserve the rejected return
  // and timeline event, but do not misclassify that exact, previously failed
  // acquisition as a release-without-acquire host fault. A release with no
  // matching successful or failed acquisition remains a fault.
  const bool matched_failed_acquire = !released && valid_name &&
      consume_failed_acquire(safe_name, version);
  // A bounded close-time duplicate is a plug-in cleanup defect, but not a host
  // lifetime fault when this exact suite/version was successfully acquired and
  // released to zero earlier in the same session. Keep returning rejection and
  // recording it in the timeline. Only the first duplicate per key is
  // contained; never-acquired, non-close, and repeated duplicates remain
  // faults.
  const bool contained_close_duplicate = !released && !matched_failed_acquire &&
      valid_name && contain_close_duplicate_release(safe_name, version);
  if (!released && !matched_failed_acquire && !contained_close_duplicate)
    rejected_releases_.fetch_add(1, std::memory_order_relaxed);
  record_suite_timeline(false, owned_name.text.data(), owned_name.length,
                        valid_name, version, released ? 0 : 1);
  if (trace_writer && valid_name && owned_name.length != 0)
    trace_writer->suite_release(safe_name, std::max<int32_t>(version, 0), released);
  return released ? 0 : 1;
}

void SuiteRegistry::record_suite_timeline(bool acquire, const char* name,
                                          std::size_t name_length,
                                          bool valid_name, int32_t version,
                                          int32_t result) {
  const std::string owned_name(name ? std::string(name, name_length) : std::string());
  const std::string selector(g_suite_selector ? g_suite_selector : "HOST");
  std::lock_guard<std::mutex> lock(timeline_mutex_);
  if (!valid_name ||
      !valid_schema_text(owned_name, kMaxTelemetrySuiteNameBytes, true) ||
      !valid_schema_text(selector, kMaxSuiteSelectorBytes, false) ||
      version <= 0 || version > kMaxSuiteVersion ||
      suite_timeline_.size() >= kMaxSuiteTimeline) {
    suite_timeline_truncated_ = true;
    return;
  }
  suite_timeline_.push_back({
      static_cast<uint32_t>(suite_timeline_.size()), acquire, owned_name,
      version, selector, result});
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
  std::lock_guard<std::mutex> lock(missing_suites_mutex_);
  if (!valid_schema_text(name, kMaxTelemetrySuiteNameBytes, true) ||
      version <= 0 || version > kMaxSuiteVersion) {
    missing_suites_truncated_ = true;
    return;
  }
  const auto entry = std::make_pair(name, version);
  if (std::find(missing_suites_.begin(), missing_suites_.end(), entry) !=
      missing_suites_.end()) return;
  if (missing_suites_.size() >= kMaxMissingSuites) {
    missing_suites_truncated_ = true;
    return;
  }
  missing_suites_.push_back(entry);
}

int32_t SuiteRegistry::reject_unknown(const char* name, int32_t version,
                                      TraceWriter* trace_writer) {
  const std::string safe_name = safe_missing_name(name);
  record_missing_suite(safe_name, version);
  record_failed_acquire(safe_name, version);
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

void SuiteRegistry::record_failed_acquire(const std::string& name,
                                          int32_t version) {
  if (!valid_schema_text(name, kMaxTelemetrySuiteNameBytes, true) ||
      version <= 0 || version > kMaxSuiteVersion)
    return;
  std::lock_guard<std::mutex> lock(failed_acquires_mutex_);
  const auto key = std::make_pair(name, version);
  auto found = failed_acquires_.find(key);
  if (found != failed_acquires_.end()) {
    if (found->second != std::numeric_limits<uint32_t>::max()) ++found->second;
    return;
  }
  if (failed_acquires_.size() < kMaxFailedAcquireKeys)
    failed_acquires_.emplace(key, 1);
}

bool SuiteRegistry::consume_failed_acquire(const std::string& name,
                                           int32_t version) {
  std::lock_guard<std::mutex> lock(failed_acquires_mutex_);
  const auto found = failed_acquires_.find(std::make_pair(name, version));
  if (found == failed_acquires_.end()) return false;
  if (--found->second == 0) failed_acquires_.erase(found);
  return true;
}

void SuiteRegistry::record_successful_acquire(const std::string& name,
                                              int32_t version) {
  std::lock_guard<std::mutex> lock(failed_acquires_mutex_);
  const auto key = std::make_pair(name, version);
  if (successful_acquires_.find(key) != successful_acquires_.end() ||
      successful_acquires_.size() < kMaxFailedAcquireKeys)
    successful_acquires_[key] = true;
}

bool SuiteRegistry::contain_close_duplicate_release(const std::string& name,
                                                    int32_t version) {
  if (!g_suite_selector ||
      std::string_view(g_suite_selector) != "GLOBAL_SETDOWN")
    return false;
  const auto key = std::make_pair(name, version);
  std::lock_guard<std::mutex> lock(failed_acquires_mutex_);
  if (successful_acquires_.find(key) == successful_acquires_.end() ||
      contained_close_releases_.find(key) != contained_close_releases_.end())
    return false;
  contained_close_releases_[key] = true;
  return true;
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

uint32_t SuiteRegistry::rejected_release_count() const {
  return rejected_releases_.load(std::memory_order_relaxed);
}
std::string SuiteRegistry::live_summary() const {
  return lease_tracker_.live_summary();
}
suite_runtime::SuiteLeaseSnapshot SuiteRegistry::snapshot() const {
  return lease_tracker_.snapshot();
}
uint32_t SuiteRegistry::release_since(
    const suite_runtime::SuiteLeaseSnapshot& baseline,
    TraceWriter* trace_writer) {
  const auto current = lease_tracker_.snapshot();
  uint32_t released = 0;
  for (const auto& [key, count] : current.live_leases) {
    uint32_t baseline_count = 0;
    for (const auto& [baseline_key, candidate] :
         baseline.live_leases) {
      if (baseline_key == key) {
        baseline_count = candidate;
        break;
      }
    }
    for (uint32_t index = baseline_count; index < count; ++index) {
      if (release(key.first.c_str(), key.second, trace_writer) != 0)
        return released;
      ++released;
    }
  }
  return released;
}
uint32_t SuiteRegistry::force_release_all() noexcept {
  return lease_tracker_.force_release_all();
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
  json << "],\"missing_suites_truncated\":"
       << (missing_suites_truncated_ ? "true" : "false");
  return json.str();
}

void SuiteRegistry::note_unsupported_suite_call(UnsupportedSuiteId suite,
                                                uint32_t slot) noexcept {
  const auto descriptor = unsupported_suite_descriptor(suite);
  if (!descriptor.name || descriptor.version <= 0 ||
      slot > kMaxUnsupportedSuiteSlot) return;
  bool inserted = false;
  try {
    std::lock_guard<std::mutex> lock(unsupported_suite_calls_mutex_);
    const auto found = std::find_if(
        unsupported_suite_calls_.begin(), unsupported_suite_calls_.end(),
        [suite, slot](const UnsupportedSuiteCall& call) {
          return call.suite == suite && call.slot == slot;
        });
    if (found != unsupported_suite_calls_.end()) {
      if (found->call_count != std::numeric_limits<uint32_t>::max())
        ++found->call_count;
      return;
    }
    if (unsupported_suite_calls_.size() >= kMaxUnsupportedSuiteCalls) {
      unsupported_suite_calls_truncated_ = true;
      return;
    }
    unsupported_suite_calls_.push_back({suite, slot, 1});
    inserted = true;
  } catch (...) {
    return;
  }
  if (inserted) {
    try {
      std::cerr << "stage:suite_slot_unsupported suite=" << descriptor.name
                << " version=" << descriptor.version << " slot=" << slot
                << "\n" << std::flush;
    } catch (...) {
    }
  }
}

std::string SuiteRegistry::unsupported_suite_calls_report_json() const {
  std::lock_guard<std::mutex> lock(unsupported_suite_calls_mutex_);
  std::ostringstream json;
  json << ",\"unsupported_suite_calls\":[";
  for (std::size_t index = 0; index < unsupported_suite_calls_.size(); ++index) {
    if (index != 0) json << ',';
    const auto& call = unsupported_suite_calls_[index];
    const auto descriptor = unsupported_suite_descriptor(call.suite);
    json << "{\"name\":\"" << escape_json(descriptor.name)
         << "\",\"version\":" << descriptor.version
         << ",\"slot\":" << call.slot
         << ",\"call_count\":" << call.call_count << '}';
  }
  json << "],\"unsupported_suite_calls_truncated\":"
       << (unsupported_suite_calls_truncated_ ? "true" : "false");
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
  json << "],\"suite_timeline_truncated\":"
       << (suite_timeline_truncated_ ? "true" : "false");
  return json.str();
}

SuiteRegistry& suite_registry() {
  static SuiteRegistry registry;
  return registry;
}

int32_t record_unsupported_suite_call(UnsupportedSuiteId suite,
                                      uint32_t slot) noexcept {
  suite_registry().note_unsupported_suite_call(suite, slot);
  return 4;
}

const char* set_suite_timeline_selector(const char* selector) noexcept {
  const char* previous = g_suite_selector;
  g_suite_selector = selector;
  return previous;
}

const char* current_suite_timeline_selector() noexcept {
  return g_suite_selector;
}

}  // namespace aexcompat::worker_runtime
