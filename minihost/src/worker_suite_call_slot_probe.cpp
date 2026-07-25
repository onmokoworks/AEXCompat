#include "worker_suite_call_slot_probe.hpp"

#include <windows.h>
#include <intrin.h>

#include <array>
#include <atomic>
#include <cwchar>
#include <iomanip>
#include <limits>
#include <mutex>
#include <sstream>
#include <utility>

namespace aexcompat::worker_runtime::suite_call_slot_probe {
namespace {

inline constexpr wchar_t kProbeEnvironment[] =
    L"AEXCOMPAT_SUITE_CALL_SLOT_PROBE";
inline constexpr std::size_t kMaxProbeEnvironmentChars = 256;
inline constexpr std::size_t kCapturedArgumentCount = 8;

enum class ProbeTarget : std::size_t {
  private_effect_3,
  private_effect_5,
  count,
};

struct TargetDescriptor {
  const char* name{};
  const wchar_t* token{};
  int32_t version{};
};

inline constexpr std::array<TargetDescriptor,
                            static_cast<std::size_t>(ProbeTarget::count)>
    kTargets{{
        {kPrivateEffectSuiteName, L"PF AE Private Effect Suite@3",
         kPrivateEffectSuiteVersion3},
        {kPrivateEffectSuiteName, L"PF AE Private Effect Suite@5",
         kPrivateEffectSuiteVersion5},
    }};

struct ProbeConfiguration {
  std::array<bool, kTargets.size()> enabled{};
  std::size_t target_count{};
  bool valid{};
  bool truncated{};
};

ProbeConfiguration read_configuration() noexcept {
  ProbeConfiguration configuration;
  std::array<wchar_t, kMaxProbeEnvironmentChars + 1> value{};
  const DWORD length = GetEnvironmentVariableW(
      kProbeEnvironment, value.data(), static_cast<DWORD>(value.size()));
  if (length == 0) {
    configuration.valid = true;
    return configuration;
  }
  if (length >= value.size()) {
    configuration.truncated = true;
    return configuration;
  }
  std::size_t start = 0;
  while (start < length) {
    std::size_t end = start;
    while (end < length && value[end] != L';') ++end;
    if (end == start || configuration.target_count >= kMaxProbeTargets) {
      configuration.truncated = true;
      return configuration;
    }
    bool recognized = false;
    for (std::size_t target = 0; target < kTargets.size(); ++target) {
      const std::size_t token_length = std::wcslen(kTargets[target].token);
      if (token_length == end - start &&
          std::wmemcmp(value.data() + start, kTargets[target].token,
                       token_length) == 0) {
        configuration.enabled[target] = true;
        recognized = true;
        break;
      }
    }
    if (!recognized) {
      configuration.truncated = true;
      return configuration;
    }
    ++configuration.target_count;
    start = end + 1;
  }
  configuration.valid = true;
  return configuration;
}

bool target_enabled(ProbeTarget target) noexcept {
  const ProbeConfiguration configuration = read_configuration();
  return configuration.valid &&
      configuration.enabled[static_cast<std::size_t>(target)];
}

struct ProbeCall {
  uint32_t slot{};
  uint32_t call_count{};
  uint8_t argument_nonzero_mask{};
  uintptr_t caller_rva{};
  bool caller_rva_valid{};
  uint32_t exception_code{};
};

struct ProbeState {
  std::mutex mutex;
  std::array<ProbeCall, kProbeSlotCount> calls{};
  std::array<bool, kProbeSlotCount> observed{};
  std::atomic_bool truncated{};
};

std::array<ProbeState, kTargets.size()>& states() {
  static std::array<ProbeState, kTargets.size()> values;
  return values;
}

std::pair<uintptr_t, bool> caller_rva(uintptr_t return_address) noexcept {
  HMODULE module{};
  if (!return_address ||
      !GetModuleHandleExW(
          GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
              GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
          reinterpret_cast<LPCWSTR>(return_address), &module) ||
      !module) {
    return {};
  }
  const uintptr_t base = reinterpret_cast<uintptr_t>(module);
  if (return_address < base) return {};
  return {return_address - base, true};
}

void record_call(ProbeTarget target, uint32_t slot,
                 const std::array<uintptr_t, kCapturedArgumentCount>& arguments,
                 uintptr_t return_address, uint32_t exception_code) noexcept {
  if (slot >= kProbeSlotCount) return;
  const auto [rva, valid_rva] = caller_rva(return_address);
  uint8_t nonzero_mask{};
  for (std::size_t index = 0; index < arguments.size(); ++index) {
    if (arguments[index] != 0)
      nonzero_mask |= static_cast<uint8_t>(1u << index);
  }
  ProbeState& probe = states()[static_cast<std::size_t>(target)];
  try {
    std::lock_guard<std::mutex> lock(probe.mutex);
    ProbeCall& call = probe.calls[slot];
    if (!probe.observed[slot]) {
      probe.observed[slot] = true;
      call = {slot, 1, nonzero_mask, rva, valid_rva, exception_code};
    } else if (call.call_count != std::numeric_limits<uint32_t>::max()) {
      ++call.call_count;
    } else {
      probe.truncated.store(true, std::memory_order_relaxed);
    }
  } catch (...) {
    probe.truncated.store(true, std::memory_order_relaxed);
  }
}

template <ProbeTarget Target, std::size_t Slot>
int32_t __cdecl identifying_trap(
    uintptr_t rcx, uintptr_t rdx, uintptr_t r8, uintptr_t r9,
    uintptr_t stack0, uintptr_t stack1, uintptr_t stack2, uintptr_t stack3) {
  static_assert(Slot < kProbeSlotCount);
  constexpr uint32_t exception_code =
      kProbeExceptionBase +
      static_cast<uint32_t>(Target) * kProbeExceptionTargetStride +
      static_cast<uint32_t>(Slot);
  const auto* return_slot =
      reinterpret_cast<const uintptr_t*>(_AddressOfReturnAddress());
  const uintptr_t return_address = return_slot ? *return_slot : 0;
  record_call(
      Target, static_cast<uint32_t>(Slot),
      {rcx, rdx, r8, r9, stack0, stack1, stack2, stack3},
      return_address, exception_code);
  // Do not copy process-local values into exception metadata: dumps and
  // artifact collectors can persist RaiseException arguments. The target,
  // slot, and zero/nonzero shape retain the bounded ABI evidence.
  uint8_t nonzero_mask{};
  const uintptr_t arguments[] = {rcx, rdx, r8, r9, stack0, stack1, stack2,
                                 stack3};
  for (std::size_t index = 0; index < std::size(arguments); ++index) {
    if (arguments[index] != 0)
      nonzero_mask |= static_cast<uint8_t>(1u << index);
  }
  const ULONG_PTR exception_arguments[] = {
      static_cast<ULONG_PTR>(Target), static_cast<ULONG_PTR>(Slot),
      static_cast<ULONG_PTR>(nonzero_mask)};
  RaiseException(exception_code, EXCEPTION_NONCONTINUABLE,
                 static_cast<DWORD>(std::size(exception_arguments)),
                 exception_arguments);
  return 4;
}

template <ProbeTarget Target, std::size_t... Slots>
std::array<void*, sizeof...(Slots)> make_probe_table(
    std::index_sequence<Slots...>) {
  return {{reinterpret_cast<void*>(&identifying_trap<Target, Slots>)...}};
}

template <ProbeTarget Target>
const std::array<void*, kProbeSlotCount>& probe_table() {
  static const auto table =
      make_probe_table<Target>(std::make_index_sequence<kProbeSlotCount>{});
  return table;
}

void hex_value(std::ostringstream& json, uintptr_t value) {
  json << "\"0x" << std::hex << std::setw(sizeof(uintptr_t) * 2)
       << std::setfill('0') << value << std::dec << '"';
}

const char* argument_shape(const ProbeCall& call, std::size_t index) noexcept {
  return (call.argument_nonzero_mask & static_cast<uint8_t>(1u << index)) != 0
      ? "nonzero"
      : "zero";
}

uint32_t nonzero_argument_count(const ProbeCall& call) noexcept {
  uint32_t count{};
  for (std::size_t index = 0; index < kCapturedArgumentCount; ++index) {
    if ((call.argument_nonzero_mask & static_cast<uint8_t>(1u << index)) != 0)
      ++count;
  }
  return count;
}

void append_target_report(std::ostringstream& json, ProbeTarget target,
                          bool enabled) {
  const std::size_t target_index = static_cast<std::size_t>(target);
  const TargetDescriptor& descriptor = kTargets[target_index];
  ProbeState& probe = states()[target_index];
  std::lock_guard<std::mutex> lock(probe.mutex);
  json << "{\"name\":\"" << descriptor.name
       << "\",\"version\":" << descriptor.version
       << ",\"enabled\":" << (enabled ? "true" : "false")
       << ",\"calls\":[";
  bool first = true;
  for (std::size_t slot = 0; slot < kProbeSlotCount; ++slot) {
    if (!probe.observed[slot]) continue;
    if (!first) json << ',';
    first = false;
    const ProbeCall& call = probe.calls[slot];
    json << "{\"slot\":" << call.slot
         << ",\"call_count\":" << call.call_count
         << ",\"exception_code\":" << call.exception_code
         << ",\"argument_word_count\":" << kCapturedArgumentCount
         << ",\"nonzero_word_count\":" << nonzero_argument_count(call)
         << ",\"registers\":{";
    const char* register_names[] = {"rcx", "rdx", "r8", "r9"};
    for (std::size_t index = 0; index < 4; ++index) {
      if (index != 0) json << ',';
      json << '"' << register_names[index] << "\":\""
           << argument_shape(call, index) << '"';
    }
    json << "},\"stack\":[";
    for (std::size_t index = 4; index < kCapturedArgumentCount; ++index) {
      if (index != 4) json << ',';
      json << '"' << argument_shape(call, index) << '"';
    }
    json << "],\"caller_rva\":";
    if (call.caller_rva_valid) {
      hex_value(json, call.caller_rva);
    } else {
      json << "null";
    }
    json << '}';
  }
  json << "],\"truncated\":"
       << (probe.truncated.load(std::memory_order_relaxed) ? "true" : "false")
       << '}';
}

}  // namespace

bool private_effect_probe3_available(void*) noexcept {
  return target_enabled(ProbeTarget::private_effect_3);
}

bool private_effect_probe5_available(void*) noexcept {
  return target_enabled(ProbeTarget::private_effect_5);
}

const void* provide_private_effect_probe3(void*) noexcept {
  return private_effect_probe3_available(nullptr)
      ? probe_table<ProbeTarget::private_effect_3>().data() : nullptr;
}

const void* provide_private_effect_probe5(void*) noexcept {
  return private_effect_probe5_available(nullptr)
      ? probe_table<ProbeTarget::private_effect_5>().data() : nullptr;
}

std::string report_json() {
  const ProbeConfiguration configuration = read_configuration();
  const bool enabled = configuration.valid &&
      (configuration.enabled[0] || configuration.enabled[1]);
  std::ostringstream json;
  json << ",\"suite_call_slot_probe\":{"
       << "\"enabled\":" << (enabled ? "true" : "false")
       << ",\"slot_count\":" << kProbeSlotCount
       << ",\"maximum_targets\":" << kMaxProbeTargets
       << ",\"targets\":[";
  for (std::size_t target = 0; target < kTargets.size(); ++target) {
    if (target != 0) json << ',';
    append_target_report(
        json, static_cast<ProbeTarget>(target),
        configuration.valid && configuration.enabled[target]);
  }
  json << "],\"configuration_truncated\":"
       << (configuration.truncated ? "true" : "false") << '}';
  return json.str();
}

}  // namespace aexcompat::worker_runtime::suite_call_slot_probe
