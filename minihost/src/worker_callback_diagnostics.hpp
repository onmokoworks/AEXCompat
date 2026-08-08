#pragma once

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <mutex>
#include <sstream>
#include <string>

namespace aexcompat::callback_diagnostics {

enum class Callback : std::size_t {
  PreCheckoutLayer,
  CheckoutPixels,
  CheckinPixels,
  CheckoutOutput,
  Iterate,
  IterateOrigin,
  Sampling,
  BeginSampling,
  EndSampling,
  GetCallbackAddr,
  Blend,
  Convolve,
  Copy,
  Fill,
  Premultiply,
  TransferRect,
  TransformWorld,
  NewWorld,
  DisposeWorld,
  Handle,
  PlatformData,
  PixelData,
  App,
  Ansi,
  CheckoutParam,
  CheckinParam,
  Count,
};

enum class Reason : std::size_t {
  None,
  NoActiveState,
  InvalidArguments,
  TemporalCheckoutDenied,
  MalformedRequest,
  CapacityExceeded,
  UnknownLayer,
  UnknownCheckout,
  MissingWorld,
  AlreadyCheckedOut,
  NotCheckedOut,
  EmptyResult,
  InvalidArea,
  PixelCallback,
  ProgressCallback,
  AbortCallback,
  Unsupported,
  CallbackError,
  Count,
};

inline constexpr std::array<const char*, static_cast<std::size_t>(Callback::Count)>
    CALLBACK_NAMES{{"pre_checkout_layer", "checkout_pixels", "checkin_pixels",
                    "checkout_output", "iterate", "iterate_origin", "sampling",
                    "begin_sampling", "end_sampling", "get_callback_addr", "blend", "convolve", "copy",
                    "fill", "premultiply", "transfer_rect", "transform_world",
                    "new_world", "dispose_world", "handle", "platform_data", "pixel_data",
                    "app", "ansi", "checkout_param", "checkin_param"}};
inline constexpr std::array<const char*, static_cast<std::size_t>(Reason::Count)>
    REASON_NAMES{{"none", "no_active_state", "invalid_arguments",
                  "temporal_checkout_denied", "malformed_request", "capacity_exceeded",
                  "unknown_layer", "unknown_checkout", "missing_world",
                  "already_checked_out", "not_checked_out", "empty_result",
                  "invalid_area", "pixel_callback", "progress_callback",
                  "abort_callback", "unsupported", "callback_error"}};

struct Entry {
  std::atomic<uint32_t> calls{};
  std::atomic<uint32_t> successes{};
  std::atomic<uint32_t> failures{};
  std::atomic<int32_t> last_result{};
  std::array<std::atomic<uint32_t>, static_cast<std::size_t>(Reason::Count)> reasons{};
};

struct HistoryEntry {
  uint64_t sequence{};
  Callback callback{};
  int32_t result{};
  Reason reason{};
};

inline constexpr std::size_t HISTORY_CAPACITY = 32;
inline std::array<HistoryEntry, HISTORY_CAPACITY>& history_entries() {
  static std::array<HistoryEntry, HISTORY_CAPACITY> value{};
  return value;
}
inline std::mutex& history_mutex() { static std::mutex value; return value; }
inline uint64_t& history_next_sequence() { static uint64_t value{}; return value; }
inline std::size_t& history_count() { static std::size_t value{}; return value; }

inline std::array<Entry, static_cast<std::size_t>(Callback::Count)>& entries() {
  static std::array<Entry, static_cast<std::size_t>(Callback::Count)> value{};
  return value;
}

inline void reset() noexcept {
  for (auto& entry : entries()) {
    entry.calls.store(0, std::memory_order_relaxed);
    entry.successes.store(0, std::memory_order_relaxed);
    entry.failures.store(0, std::memory_order_relaxed);
    entry.last_result.store(0, std::memory_order_relaxed);
    for (auto& reason : entry.reasons) reason.store(0, std::memory_order_relaxed);
  }
  std::lock_guard<std::mutex> lock(history_mutex());
  history_entries() = {};
  history_next_sequence() = 0;
  history_count() = 0;
}

inline void increment_saturating(std::atomic<uint32_t>& value) noexcept {
  uint32_t current = value.load(std::memory_order_relaxed);
  while (current != std::numeric_limits<uint32_t>::max() &&
         !value.compare_exchange_weak(current, current + 1,
                                      std::memory_order_relaxed,
                                      std::memory_order_relaxed)) {}
}

inline int32_t record(Callback callback, int32_t result, Reason reason = Reason::None) noexcept {
  auto& entry = entries()[static_cast<std::size_t>(callback)];
  increment_saturating(entry.calls);
  entry.last_result.store(result, std::memory_order_relaxed);
  if (result == 0) {
    increment_saturating(entry.successes);
  } else {
    increment_saturating(entry.failures);
    increment_saturating(entry.reasons[static_cast<std::size_t>(reason)]);
  }
  {
    std::lock_guard<std::mutex> lock(history_mutex());
    const uint64_t sequence = history_next_sequence()++;
    history_entries()[sequence % HISTORY_CAPACITY] =
        {sequence, callback, result, reason};
    if (history_count() < HISTORY_CAPACITY) ++history_count();
  }
  return result;
}

inline std::string history_json() {
  std::lock_guard<std::mutex> lock(history_mutex());
  std::ostringstream out;
  out << '[';
  const uint64_t next = history_next_sequence();
  const uint64_t first = next - history_count();
  for (uint64_t sequence = first; sequence < next; ++sequence) {
    if (sequence != first) out << ',';
    const auto& entry = history_entries()[sequence % HISTORY_CAPACITY];
    out << "{\"sequence\":" << entry.sequence << ",\"callback\":\""
        << CALLBACK_NAMES[static_cast<std::size_t>(entry.callback)]
        << "\",\"result\":" << entry.result << ",\"reason\":\""
        << REASON_NAMES[static_cast<std::size_t>(entry.reason)] << "\"}";
  }
  out << ']';
  return out.str();
}

inline std::string snapshot_json() {
  std::ostringstream out;
  out << '{';
  bool first_callback = true;
  for (std::size_t callback = 0; callback < entries().size(); ++callback) {
    const auto& entry = entries()[callback];
    const uint32_t calls = entry.calls.load(std::memory_order_relaxed);
    if (calls == 0) continue;
    if (!first_callback) out << ',';
    first_callback = false;
    out << '"' << CALLBACK_NAMES[callback] << "\":{\"calls\":" << calls
        << ",\"successes\":" << entry.successes.load(std::memory_order_relaxed)
        << ",\"failures\":" << entry.failures.load(std::memory_order_relaxed)
        << ",\"last_result\":" << entry.last_result.load(std::memory_order_relaxed)
        << ",\"denials\":{";
    bool first_reason = true;
    for (std::size_t reason = 1; reason < entry.reasons.size(); ++reason) {
      const uint32_t count = entry.reasons[reason].load(std::memory_order_relaxed);
      if (count == 0) continue;
      if (!first_reason) out << ',';
      first_reason = false;
      out << '"' << REASON_NAMES[reason] << "\":" << count;
    }
    out << "}}";
  }
  out << '}';
  return out.str();
}

inline std::string report_field_json() {
  return ",\"callback_diagnostics\":" + snapshot_json() +
      ",\"callback_history\":" + history_json();
}

}  // namespace aexcompat::callback_diagnostics
