#pragma once

// Explicit worker-request diagnostics. No environment switch, allocator metadata,
// arbitrary payload, freed storage, or plug-in stack is ever modified here.
#include "generated/aex_abi_contract.hpp"
#include <array>
#include <cstddef>
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <mutex>
#include <sstream>
#include <string>
#include <vector>

namespace aexcompat::memory_fill {
enum Scope { Padding, Output, Parameter, Handle, Suite, Count };
inline constexpr const char* names[] = {"world_padding", "fresh_output_pixels",
    "parameter_unused_storage", "fresh_nonclear_handles", "fresh_nonclear_suite_allocations"};
struct Config {
  bool enabled{};
  std::vector<unsigned char> pattern;
  std::string hex;
  std::array<bool, Count> selected{};
};
inline Config config;
inline std::mutex mutex;
inline uint64_t next_id{};
inline std::array<uint64_t, Count> applied{}, unsupported{};
inline std::vector<std::string> events;
inline uint64_t dropped{};
inline thread_local const char* selector = "HOST";
inline bool journal_failed{};
inline void (*journal_write)(const std::string&){};
bool configure_journal(const wchar_t* value);
inline bool selected(Scope scope) { return config.enabled && config.selected[scope]; }
inline unsigned char byte(std::size_t offset) { return config.pattern[offset % config.pattern.size()]; }

inline bool configure(const wchar_t* text) {
  if (!text || config.enabled) return false;
  const std::wstring wire(text);
  if (wire.size() < 11 || wire.size() > 137 || wire.substr(0, 3) != L"v1|") return false;
  const auto sep = wire.find(L'|', 3);
  if (sep == std::wstring::npos || sep == 3 || (sep - 3) % 2 || wire.size() != sep + 6) return false;
  Config parsed;
  auto nibble = [](wchar_t ch) { return ch >= L'0' && ch <= L'9' ? ch - L'0' :
      ch >= L'a' && ch <= L'f' ? ch - L'a' + 10 : -1; };
  for (std::size_t i = 3; i < sep; i += 2) {
    const int hi = nibble(wire[i]), lo = nibble(wire[i + 1]);
    if (hi < 0 || lo < 0) return false;
    parsed.pattern.push_back(static_cast<unsigned char>(hi * 16 + lo));
    parsed.hex += static_cast<char>(wire[i]); parsed.hex += static_cast<char>(wire[i + 1]);
  }
  for (int i = 0; i < Count; ++i) {
    if (wire[sep + 1 + i] != L'0' && wire[sep + 1 + i] != L'1') return false;
    parsed.selected[i] = wire[sep + 1 + i] == L'1';
  }
  parsed.enabled = true;
  config = std::move(parsed);
  return true;
}

inline uint64_t allocation_id() {
  if (!config.enabled) return 0;
  std::lock_guard lock(mutex);
  return ++next_id;
}
inline std::string hash(const unsigned char* data, std::size_t size) {
  uint64_t value = 14695981039346656037ULL;
  for (std::size_t i = 0; i < size; ++i) value = (value ^ data[i]) * 1099511628211ULL;
  std::ostringstream out; out << std::hex << std::setw(16) << std::setfill('0') << value;
  return out.str();
}
inline std::string protected_hash(const unsigned char* data, std::size_t size,
    std::size_t offset, std::size_t length, std::size_t count, std::size_t stride, bool clear) {
  uint64_t value = 14695981039346656037ULL;
  for (std::size_t at = 0; at < size; ++at) {
    const bool filled = !clear && count && at >= offset &&
        (stride ? (at - offset) / stride < count && (at - offset) % stride < length
                : at - offset < length);
    if (!filled) value = (value ^ data[at]) * 1099511628211ULL;
  }
  std::ostringstream out; out << std::hex << std::setw(16) << std::setfill('0') << value;
  return out.str();
}
inline void emit(Scope scope, const std::string& event, bool changed, bool unavailable = false) {
  std::lock_guard lock(mutex);
  applied[scope] += changed; unsupported[scope] += unavailable;
  // Incremental records survive a selector fault/timeout in captured stderr.
  // The bound protects worker memory and the broker's stderr budget.
  if (events.size() < 2048) {
    events.push_back(event);
    if (journal_write) journal_write(event);
    std::cerr << "memory_fill:" << event << '\n' << std::flush;
  } else {
    if (!dropped) {
      const std::string marker = "{\"event\":\"truncated\",\"limit\":2048,\"status\":\"unsupported\"}";
      if (journal_write) journal_write(marker);
      std::cerr << "memory_fill:" << marker << '\n' << std::flush;
    }
    ++dropped;
  }
}
inline void unavailable(Scope scope, const char* owner) {
  if (!selected(scope)) return;
  emit(scope, std::string("{\"scope\":\"") + names[scope] +
      "\",\"owner\":\"" + owner + "\",\"status\":\"unsupported\"}", false, true);
}

// Ranges are [offset + i*stride, offset + i*stride + length), i in [0,count).
// The complement is the initialized/protected mask. Call only after the host's
// normal initialization so hashing never reads indeterminate C++ storage.
inline void fill(Scope scope, void* memory, std::size_t size, std::size_t offset,
                 std::size_t length, std::size_t count, std::size_t stride,
                 const char* owner, uint64_t id = 0, bool clear = false,
                 bool xor_pattern = false) {
  if (!selected(scope)) return;
  if (!id) id = allocation_id();
  if ((!memory && size) || offset > size || length > size - offset ||
      (count && stride && count - 1 > (size - offset - length) / stride)) {
    unavailable(scope, owner); return;
  }
  auto* data = static_cast<unsigned char*>(memory);
  const auto before = hash(data, size);
  const auto protected_before = protected_hash(data, size, offset, length, count, stride, clear);
  if (!clear) for (std::size_t i = 0; i < count; ++i)
    for (std::size_t j = 0; j < length; ++j) {
      const auto at = offset + i * stride + j;
      data[at] = xor_pattern ? data[at] ^ byte(j) : byte(at);
    }
  const bool changed = !clear && length && count;
  std::ostringstream out;
  out << "{\"scope\":\"" << names[scope] << "\",\"owner\":\"" << owner
      << "\",\"selector\":\"" << (selector ? selector : "HOST") << "\",\"allocation_id\":" << id << ",\"size\":" << size
      << ",\"range\":{\"offset\":" << offset << ",\"length\":" << length
      << ",\"count\":" << count << ",\"stride\":" << stride
      << "},\"initialized_mask\":\"range_complement\",\"clear\":" << (clear ? "true" : "false")
      << ",\"pattern_hex\":\"" << config.hex << "\",\"mode\":\"" << (xor_pattern ? "coverage_xor" : "repeat")
      << "\",\"hash_algorithm\":\"fnv1a64\",\"before_hash\":\"" << before
      << "\",\"after_hash\":\"" << hash(data, size)
      << "\",\"initialized_before_hash\":\"" << protected_before
      << "\",\"initialized_after_hash\":\"" << protected_hash(data, size, offset, length, count, stride, clear)
      << "\",\"guard_verification\":\"host_bounds_checked\",\"status\":\""
      << (changed ? "applied" : "not_present") << "\"}";
  emit(scope, out.str(), changed);
}
inline void padding(void* data, std::size_t size, int width, int height, int rowbytes, int pixel_bytes, const char* owner, uint64_t id = 0) {
  if (!selected(Padding)) return;
  if (width < 0 || height < 0 || rowbytes < 0 || pixel_bytes <= 0 ||
      uint64_t(width) * pixel_bytes > uint64_t(rowbytes) || uint64_t(rowbytes) * height > size) {
    unavailable(Padding, owner); return;
  }
  fill(Padding, data, size, std::size_t(width) * pixel_bytes,
       rowbytes - std::size_t(width) * pixel_bytes, height, rowbytes, owner, id);
}
inline void parameter(void* definition, int type) {
  if (!selected(Parameter)) return;
  using namespace aexcompat::abi::x86_64_windows;
  std::size_t active{};
  switch (type) {
    case 1: active = PF_SLIDER_DEF_SIZE; break;
    case 4: active = PF_CHECKBOX_DEF_SIZE; break;
    case 7: active = PF_POPUP_DEF_SIZE; break;
    case 10: active = PF_FLOAT_SLIDER_DEF_SIZE; break;
    case 11: active = PF_ARBITRARY_DEF_SIZE; break;
    // The full layer union is meaningful; other variants need a measured ABI
    // size before their tail can safely be distinguished from reserved members.
    case 0: return;
    default: unavailable(Parameter, "unmeasured_parameter_variant"); return;
  }
  fill(Parameter, definition, PARAM_U_OFFSET + PARAM_U_SIZE,
       PARAM_U_OFFSET + active, PARAM_U_SIZE - active, 1, 0, "PF_ParamDef");
}
inline std::string report() {
  if (!config.enabled) return {};
  std::lock_guard lock(mutex);
  std::ostringstream out;
  out << ",\"memory_fill\":{\"pattern_hex\":\"" << config.hex << "\",\"scopes\":{";
  for (int i = 0; i < Count; ++i) {
    if (i) out << ',';
    out << '"' << names[i] << "\":{\"selected\":" << (config.selected[i] ? "true" : "false")
        << ",\"status\":\"" << (unsupported[i] || dropped || journal_failed ? "unsupported" : applied[i] ? "applied" : "not_present")
        << "\",\"applied_count\":" << applied[i] << ",\"unsupported_count\":" << unsupported[i] << '}';
  }
  out << "},\"journal_failed\":" << (journal_failed ? "true" : "false")
      << ",\"dropped_events\":" << dropped << ",\"events\":[";
  for (std::size_t i = 0; i < events.size(); ++i) { if (i) out << ','; out << events[i]; }
  out << "]}"; return out.str();
}
}  // namespace aexcompat::memory_fill
