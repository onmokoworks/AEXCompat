#include "worker_abi_offsets.hpp"
#include "worker_memory_fill.hpp"
#include "worker_parameter_execution.hpp"
#include "worker_parameter_limits.hpp"

#include "worker_extended_diag.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>
#include <iomanip>
#include <sstream>
#include <vector>

namespace abi_contract = aexcompat::abi::x86_64_windows;
namespace abi_offsets = aexcompat::worker_abi_offsets;

namespace aexcompat::worker_runtime::parameter_execution {
using parameters::RequestedAssignments;
using parameters::RequestedKind;
namespace {
constexpr int32_t kArbitraryCallback = 22;
constexpr std::size_t kInCurrentTime = abi_contract::IN_CURRENT_TIME_OFFSET;
Hooks g_hooks{};
Hooks& hooks() { return g_hooks; }
parameters::State& runtime() { return parameters::state(); }
bool has_default_range(const parameters::ParamRecord& record) {
  return record.type == 1 || record.type == 2 || record.type == 7 || record.type == 10;
}
bool valid_default_range(const parameters::ParamRecord& record) {
  return record.has_numeric && std::isfinite(record.valid_min) &&
      std::isfinite(record.valid_max) && record.valid_min <= record.valid_max;
}
double effective_default(const parameters::ParamRecord& record) {
  // Preserve the declaration for discovery. Normalize only the value supplied
  // to the effect, both at initialization and on a later default/reset request.
  if (has_default_range(record) && valid_default_range(record) &&
      std::isfinite(record.default_value))
    return std::clamp(record.default_value, record.valid_min, record.valid_max);
  return record.default_value;
}
template <typename T, std::size_t N> T read(const std::array<std::byte, N>& b, std::size_t o) { T v{}; std::memcpy(&v, b.data()+o, sizeof(v)); return v; }
template <typename T, std::size_t N> void write(std::array<std::byte, N>& b, std::size_t o, const T& v) { std::memcpy(b.data()+o, &v, sizeof(v)); }

bool dispose_arbitrary_handle(EffectEntry entry, BufferIn& input, BufferOut& output,
                              int16_t id, void* refcon, void* value) {
  if (!value) return true;
  std::array<std::byte, 48> extra{};
  write<int32_t>(extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 1);
  write<int16_t>(extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
  write<void*>(extra, abi_contract::ARB_DISPOSE_REFCON_OFFSET, refcon);
  write<void*>(extra, abi_contract::ARB_DISPOSE_VALUE_OFFSET, value);
  uint32_t exception_code = 0;
  return hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(),
      nullptr, nullptr, extra.data(), &exception_code) == 0 && exception_code == 0;
}

}  // namespace

bool configure_hooks(const Hooks& value) noexcept { if (!value.invoke_entry || !value.handle_is_live || !value.active_mask_count || !value.active_mask_id) return false; g_hooks=value; return true; }

bool initialize_arbitrary_values(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output,
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions) {
  for (std::size_t i = 0; i < runtime().records.size(); ++i) {
    if (runtime().records[i].type != 11) continue;
    const std::size_t u = abi_contract::PARAM_U_OFFSET;
    const int16_t id = read<int16_t>(definitions[i + 1], u);
    void* source = read<void*>(definitions[i + 1], u + abi_contract::ARBITRARY_DEFAULT_OFFSET);
    void* refcon = read<void*>(definitions[i + 1], u + abi_contract::ARBITRARY_REFCON_OFFSET);
    // PF_ADD_ARBITRARY2 initializes the current value to null independently
    // from its optional default handle.  A null default therefore represents
    // an uninitialized value; there is no source object for COPY to duplicate.
    if (!source) {
      write<void*>(definitions[i + 1], u + abi_contract::ARBITRARY_VALUE_OFFSET, nullptr);
      continue;
    }
    void* destination = nullptr;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 2);
    write<int16_t>(extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(extra, abi_contract::ARB_COPY_REFCON_OFFSET, refcon);
    write<void*>(extra, abi_contract::ARB_COPY_SOURCE_OFFSET, source);
    write<void*>(extra, abi_contract::ARB_COPY_DESTINATION_OFFSET, &destination);
    const int32_t error = entry(kArbitraryCallback, input.data(), output.data(),
                                nullptr, nullptr, extra.data());
    if (error != 0 || !destination || destination == source) {
      ++runtime().arbitrary.print_failures;
      return false;
    }
    write<void*>(definitions[i + 1], u + abi_contract::ARBITRARY_VALUE_OFFSET, destination);
    ++runtime().arbitrary.copy_calls;
  }
  return true;
}

bool apply_arbitrary_text_assignments(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output,
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions,
    const RequestedAssignments& requested) {
  for (const auto& assignment : requested) {
    if (assignment.kind != RequestedKind::ArbitraryText) continue;
    constexpr std::size_t u = abi_contract::PARAM_U_OFFSET;
    auto& definition = definitions[static_cast<std::size_t>(assignment.index)];
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + abi_contract::ARBITRARY_REFCON_OFFSET);
    void* previous = read<void*>(definition, u + abi_contract::ARBITRARY_VALUE_OFFSET);
    void* scanned = nullptr;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 10);
    write<int16_t>(extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(extra, abi_offsets::kArbScanRefcon, refcon);
    write<const char*>(extra, abi_offsets::kArbScanBuffer, assignment.text.data());
    write<uint32_t>(extra, abi_offsets::kArbScanSize, static_cast<uint32_t>(assignment.text.size()));
    write<void*>(extra, abi_offsets::kArbScanOutput, &scanned);
    uint32_t exception_code = 0;
    const bool scan_error = hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(),
        nullptr, nullptr, extra.data(), &exception_code) != 0 || exception_code != 0 ||
        !hooks().handle_is_live(scanned) || scanned == previous;
    auto dispose_scanned = [&]() {
      if (!hooks().handle_is_live(scanned) || scanned == previous) return true;
      std::array<std::byte, 48> scanned_dispose_extra{};
      write<int32_t>(scanned_dispose_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 1);
      write<int16_t>(scanned_dispose_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
      write<void*>(scanned_dispose_extra, abi_contract::ARB_DISPOSE_REFCON_OFFSET, refcon);
      write<void*>(scanned_dispose_extra, abi_contract::ARB_DISPOSE_VALUE_OFFSET, scanned);
      if (entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
                scanned_dispose_extra.data()) != 0) {
        ++runtime().arbitrary.invalid_operations;
        return false;
      }
      ++runtime().arbitrary.dispose_calls;
      return true;
    };
    if (scan_error) {
      dispose_scanned();
      ++runtime().arbitrary.scan_failures;
      return false;
    }
    std::array<std::byte, 48> dispose_extra{};
    write<int32_t>(dispose_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 1);
    write<int16_t>(dispose_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_REFCON_OFFSET, refcon);
    write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_VALUE_OFFSET, previous);
    if (entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data()) != 0) {
      dispose_scanned();
      ++runtime().arbitrary.invalid_operations;
      return false;
    }
    ++runtime().arbitrary.dispose_calls;
    ++runtime().arbitrary.scan_calls;
    write<void*>(definition, u + abi_contract::ARBITRARY_VALUE_OFFSET, scanned);
  }
  return true;
}

bool dispose_arbitrary_values(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output,
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions) {
  bool valid = true;
  for (std::size_t i = 0; i < runtime().records.size(); ++i) {
    if (runtime().records[i].type != 11) continue;
    const std::size_t u = abi_contract::PARAM_U_OFFSET;
    void* value = read<void*>(definitions[i + 1], u + abi_contract::ARBITRARY_VALUE_OFFSET);
    if (!value) continue;
    const bool disposed = dispose_arbitrary_handle(entry, input, output,
        read<int16_t>(definitions[i + 1], u),
        read<void*>(definitions[i + 1], u + abi_contract::ARBITRARY_REFCON_OFFSET), value);
    write<void*>(definitions[i + 1], u + abi_contract::ARBITRARY_VALUE_OFFSET, nullptr);
    if (disposed) ++runtime().arbitrary.dispose_calls;
    else { ++runtime().arbitrary.invalid_operations; valid = false; }
  }
  return valid;
}

bool dispose_arbitrary_defaults(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output) {
  bool valid = true;
  for (auto& param : runtime().records) {
    if (param.type != 11) continue;
    const std::size_t u = abi_contract::PARAM_U_OFFSET;
    void* value = read<void*>(param.raw, u + abi_contract::ARBITRARY_DEFAULT_OFFSET);
    if (!value) continue;
    const bool disposed = dispose_arbitrary_handle(entry, input, output,
        read<int16_t>(param.raw, u), read<void*>(param.raw, u + abi_contract::ARBITRARY_REFCON_OFFSET), value);
    write<void*>(param.raw, u + abi_contract::ARBITRARY_DEFAULT_OFFSET, nullptr);
    if (disposed) ++runtime().arbitrary.dispose_calls;
    else { ++runtime().arbitrary.invalid_operations; valid = false; }
  }
  return valid;
}

bool apply_arbitrary_parameter_animation(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output,
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions,
    int32_t time, uint32_t scale) {
  if (scale == 0) return false;
  for (const auto &timeline : runtime().timelines) {
    if (timeline.keys.front().kind != parameter_animation::AnimationValueKind::Arbitrary) continue;
    if (timeline.slot <= 0 || static_cast<std::size_t>(timeline.slot) >= definitions.size() ||
        runtime().records[timeline.slot - 1].type != 11 ||
        std::any_of(timeline.keys.begin(), timeline.keys.end(), [](const auto &key) {
          return key.kind != parameter_animation::AnimationValueKind::Arbitrary || key.arbitrary.empty() ||
                 key.arbitrary.size() > 64 * 1024;
        })) return false;
    auto &definition = definitions[timeline.slot];
    constexpr std::size_t u = abi_contract::PARAM_U_OFFSET;
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + abi_contract::ARBITRARY_REFCON_OFFSET);
    const auto dispose = [&](void* value) {
      if (!hooks().handle_is_live(value)) return false;
      const bool ok = dispose_arbitrary_handle(entry, input, output, id, refcon, value);
      if (ok) ++runtime().arbitrary.dispose_calls; else ++runtime().arbitrary.invalid_operations;
      return ok;
    };
    std::vector<void*> owned;
    const auto cleanup = [&]() {
      bool ok = true;
      for (void* value : owned) if (value) ok = dispose(value) && ok;
      owned.clear();
      return ok;
    };
    for (const auto &key : timeline.keys) {
      void* value = nullptr;
      std::array<std::byte, 48> extra{};
      write<int32_t>(extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET,
          5); write<int16_t>(extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
      write<void*>(extra, abi_contract::ARB_UNFLATTEN_REFCON_OFFSET, refcon);
      write<uint32_t>(extra, abi_contract::ARB_UNFLATTEN_SIZE_OFFSET,
          static_cast<uint32_t>(key.arbitrary.size()));
      write<const unsigned char*>(extra, abi_contract::ARB_UNFLATTEN_BUFFER_OFFSET, key.arbitrary.data());
      write<void*>(extra, abi_contract::ARB_UNFLATTEN_OUTPUT_OFFSET, &value);
      uint32_t exception_code = 0;
      if (hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
          nullptr, extra.data(), &exception_code) != 0 || exception_code != 0 ||
          !hooks().handle_is_live(value) ||
          std::find(owned.begin(), owned.end(), value) != owned.end()) {
        if (hooks().handle_is_live(value) && std::find(owned.begin(), owned.end(), value) == owned.end())
          dispose(value);
        cleanup();
        return false;
      }
      owned.push_back(value);
    }
    std::size_t selected = 0;
    void* replacement = nullptr;
    if (!parameter_animation::rational_less(timeline.keys.front().time, timeline.keys.front().scale, time, scale)) {
      replacement = owned.front();
    } else if (!parameter_animation::rational_less(time, scale, timeline.keys.back().time, timeline.keys.back().scale)) {
      selected = timeline.keys.size() - 1; replacement = owned.back();
    } else {
      std::size_t right = 1;
      while (!parameter_animation::rational_less(time, scale, timeline.keys[right].time, timeline.keys[right].scale)) ++right;
      selected = right - 1;
      const auto &left_key = timeline.keys[selected];
      if (left_key.hold) replacement = owned[selected];
      else {
        const long double now = static_cast<long double>(time) / scale;
        const long double left = static_cast<long double>(left_key.time) / left_key.scale;
        const long double right_time = static_cast<long double>(timeline.keys[right].time) /
                                       timeline.keys[right].scale;
        const double amount = static_cast<double>((now - left) / (right_time - left));
        runtime().arbitrary.last_interpolation_amount = amount;
        // AE supplies an owned NEW value for INTERP to fill. Some SDK samples
        // (including ColorGrid) require this even though the callback contract
        // also permits replacing the output handle.
        void* preallocated = nullptr;
        std::array<std::byte, 48> new_extra{};
        write<int32_t>(new_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET,
            0); write<int16_t>(new_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
        write<void*>(new_extra, abi_contract::ARB_NEW_REFCON_OFFSET,
            refcon); write<void*>(new_extra, abi_contract::ARB_NEW_OUTPUT_OFFSET, &preallocated);
        uint32_t exception_code = 0;
        if (hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
            nullptr, new_extra.data(), &exception_code) != 0 || exception_code != 0 ||
            !hooks().handle_is_live(preallocated) ||
            std::find(owned.begin(), owned.end(), preallocated) != owned.end()) {
          if (hooks().handle_is_live(preallocated) &&
              std::find(owned.begin(), owned.end(), preallocated) == owned.end())
            dispose(preallocated);
          cleanup();
          return false;
        }
        ++runtime().arbitrary.new_calls;
        replacement = preallocated;
        std::array<std::byte, 48> extra{};
        write<int32_t>(extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET,
            6); write<int16_t>(extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
        write<void*>(extra, abi_contract::ARB_INTERP_REFCON_OFFSET,
            refcon); write<void*>(extra, abi_contract::ARB_INTERP_LEFT_OFFSET, owned[selected]);
        write<void*>(extra, abi_contract::ARB_INTERP_RIGHT_OFFSET,
            owned[right]); write<double>(extra, abi_contract::ARB_INTERP_AMOUNT_OFFSET, amount);
        write<void*>(extra, abi_contract::ARB_INTERP_OUTPUT_OFFSET, &replacement);
        exception_code = 0;
        if (hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
            nullptr, extra.data(), &exception_code) != 0 || exception_code != 0 ||
            !hooks().handle_is_live(replacement) ||
            std::find(owned.begin(), owned.end(), replacement) != owned.end()) {
          if (replacement != preallocated && hooks().handle_is_live(replacement) &&
              std::find(owned.begin(), owned.end(), replacement) == owned.end()) dispose(replacement);
          if (hooks().handle_is_live(preallocated)) dispose(preallocated);
          cleanup();
          return false;
        }
        if (replacement != preallocated && hooks().handle_is_live(preallocated) &&
            !dispose(preallocated)) {
          dispose(replacement);
          cleanup();
          return false;
        }
        ++runtime().arbitrary.interpolation_calls;
      }
    }
    void* previous = read<void*>(definition, u + abi_contract::ARBITRARY_VALUE_OFFSET);
    if (!dispose(previous)) {
      if (std::find(owned.begin(), owned.end(), replacement) == owned.end()) dispose(replacement);
      cleanup();
      return false;
    }
    write<void*>(definition, u + abi_contract::ARBITRARY_VALUE_OFFSET, replacement);
    const auto transferred = std::find(owned.begin(), owned.end(), replacement);
    if (transferred != owned.end()) *transferred = nullptr;
    if (!cleanup()) return false;
  }
  return true;
}

void observe_arbitrary_defaults(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output) {
  constexpr uint32_t kMaxArbitraryPrintBytes = 64 * 1024;
  constexpr std::size_t kMaxSummaryBytes = 4096;
  for (auto& param : runtime().records) {
    if (param.type != 11) continue;
    const std::size_t u = abi_contract::PARAM_U_OFFSET;
    void* value = read<void*>(param.raw, u + abi_contract::ARBITRARY_DEFAULT_OFFSET);
    void* refcon = read<void*>(param.raw, u + abi_contract::ARBITRARY_REFCON_OFFSET);
    uint32_t print_size = 0;
    std::array<std::byte, 48> size_extra{};
    write<int32_t>(size_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 8);
    write<int16_t>(size_extra, abi_contract::ARB_EXTRA_ID_OFFSET, read<int16_t>(param.raw, u));
    write<void*>(size_extra, abi_contract::ARB_PRINT_SIZE_REFCON_OFFSET, refcon);
    write<void*>(size_extra, abi_contract::ARB_PRINT_SIZE_VALUE_OFFSET, value);
    write<void*>(size_extra, abi_contract::ARB_PRINT_SIZE_OUTPUT_OFFSET, &print_size);
    uint32_t exception_code = 0;
    if (!value || hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(),
                        nullptr, nullptr, size_extra.data(), &exception_code) != 0 ||
        exception_code != 0 || print_size == 0 ||
        print_size > kMaxArbitraryPrintBytes) {
      ++runtime().arbitrary.print_failures;
      continue;
    }
    std::vector<char> buffer(static_cast<std::size_t>(print_size) + 1, '\0');
    std::array<std::byte, 48> print_extra{};
    write<int32_t>(print_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 9);
    write<int16_t>(print_extra, abi_contract::ARB_EXTRA_ID_OFFSET, read<int16_t>(param.raw, u));
    write<void*>(print_extra, abi_contract::ARB_PRINT_REFCON_OFFSET, refcon);
    write<int32_t>(print_extra, abi_contract::ARB_PRINT_FLAGS_OFFSET, 0);
    write<void*>(print_extra, abi_contract::ARB_PRINT_VALUE_OFFSET, value);
    write<uint32_t>(print_extra, abi_contract::ARB_PRINT_SIZE_OFFSET, print_size);
    write<void*>(print_extra, abi_contract::ARB_PRINT_BUFFER_OFFSET, buffer.data());
    if (hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
              nullptr, print_extra.data(), &exception_code) != 0 || exception_code != 0) {
      ++runtime().arbitrary.print_failures;
      continue;
    }
    const auto terminator = std::find(buffer.begin(), buffer.begin() + print_size, '\0');
    if (terminator == buffer.begin() + print_size) {
      ++runtime().arbitrary.print_failures;
      continue;
    }
    const auto length = std::min<std::size_t>(
        static_cast<std::size_t>(terminator - buffer.begin()), kMaxSummaryBytes);
    param.arbitrary_summary.assign(buffer.data(), length);
    ++runtime().arbitrary.print_calls;
  }
}

ArbitraryValuesScope::~ArbitraryValuesScope() {
  if (entry && input && output && definitions)
    dispose_arbitrary_values(entry, *input, *output, *definitions);
}

void probe_arbitrary_scan(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output,
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions) {
  for (std::size_t i = 0; i < runtime().records.size(); ++i) {
    if (runtime().records[i].type != 11 || runtime().records[i].arbitrary_summary.empty()) continue;
    constexpr std::size_t u = abi_contract::PARAM_U_OFFSET;
    auto& definition = definitions[i + 1];
    const int16_t id = read<int16_t>(definition, u);
    void* source = read<void*>(definition, u + abi_contract::ARBITRARY_VALUE_OFFSET);
    void* refcon = read<void*>(definition, u + abi_contract::ARBITRARY_REFCON_OFFSET);
    void* scanned = nullptr;
    std::array<std::byte, 48> scan_extra{};
    write<int32_t>(scan_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 10);
    write<int16_t>(scan_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(scan_extra, abi_offsets::kArbScanRefcon, refcon);
    write<const char*>(scan_extra, abi_offsets::kArbScanBuffer,
        runtime().records[i].arbitrary_summary.c_str());
    write<uint32_t>(scan_extra, abi_offsets::kArbScanSize,
        static_cast<uint32_t>(runtime().records[i].arbitrary_summary.size()));
    write<void*>(scan_extra, abi_offsets::kArbScanOutput, &scanned);
    uint32_t exception_code = 0;
    const bool created = hooks().invoke_entry(entry, kArbitraryCallback, input.data(),
        output.data(), nullptr, nullptr, scan_extra.data(), &exception_code) == 0 &&
        exception_code == 0 && hooks().handle_is_live(scanned) && scanned != source;
    if (!created) {
      if (hooks().handle_is_live(scanned)) {
        std::array<std::byte, 48> dispose_extra{};
        write<int32_t>(dispose_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 1);
        write<int16_t>(dispose_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
        write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_REFCON_OFFSET, refcon);
        write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_VALUE_OFFSET, scanned);
        entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data());
        ++runtime().arbitrary.dispose_calls;
      }
      ++runtime().arbitrary.scan_failures;
      continue;
    }
    int32_t comparison = 3;
    std::array<std::byte, 48> compare_extra{};
    write<int32_t>(compare_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 7);
    write<int16_t>(compare_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(compare_extra, abi_contract::ARB_COMPARE_REFCON_OFFSET, refcon);
    write<void*>(compare_extra, abi_contract::ARB_COMPARE_LEFT_OFFSET, source);
    write<void*>(compare_extra, abi_contract::ARB_COMPARE_RIGHT_OFFSET, scanned);
    write<void*>(compare_extra, abi_contract::ARB_COMPARE_OUTPUT_OFFSET, &comparison);
    exception_code = 0;
    const bool equal = hooks().invoke_entry(entry, kArbitraryCallback, input.data(),
        output.data(), nullptr, nullptr, compare_extra.data(), &exception_code) == 0 &&
        exception_code == 0 && comparison == 0;
    std::array<std::byte, 48> dispose_extra{};
    write<int32_t>(dispose_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 1);
    write<int16_t>(dispose_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_REFCON_OFFSET, refcon);
    write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_VALUE_OFFSET, scanned);
    const bool disposed = entry(kArbitraryCallback, input.data(), output.data(),
                                nullptr, nullptr, dispose_extra.data()) == 0;
    if (disposed) ++runtime().arbitrary.dispose_calls;
    if (equal && disposed) ++runtime().arbitrary.scan_calls;
    else ++runtime().arbitrary.scan_failures;
  }
}

bool interpolate_arbitrary_values(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output,
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions) {
  const int32_t current_time = read<int32_t>(input, kInCurrentTime);
  const int32_t total_time = read<int32_t>(input, abi_contract::IN_TOTAL_TIME_OFFSET);
  const double amount = total_time > 0
      ? std::clamp(static_cast<double>(current_time) / total_time, 0.0, 1.0)
      : 0.0;
  runtime().arbitrary.last_interpolation_amount = amount;
  for (std::size_t i = 0; i < runtime().records.size(); ++i) {
    if (runtime().records[i].type != 11) continue;
    const bool timeline_controls_slot = std::any_of(
        runtime().timelines.begin(), runtime().timelines.end(),
        [i](const auto& timeline) {
          return timeline.slot == static_cast<int32_t>(i + 1) &&
              !timeline.keys.empty() &&
              timeline.keys.front().kind == parameter_animation::AnimationValueKind::Arbitrary;
        });
    if (timeline_controls_slot) continue;
    const std::size_t u = abi_contract::PARAM_U_OFFSET;
    auto& definition = definitions[i + 1];
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + abi_contract::ARBITRARY_REFCON_OFFSET);
    void* source = read<void*>(definition, u + abi_contract::ARBITRARY_VALUE_OFFSET);
    if (!source) continue;
    if (!hooks().handle_is_live(source)) {
      ++runtime().arbitrary.interpolation_failures;
      return false;
    }
    const auto dispose = [&](void* value) {
      if (!hooks().handle_is_live(value)) return false;
      const bool ok = dispose_arbitrary_handle(entry, input, output, id, refcon, value);
      if (ok) ++runtime().arbitrary.dispose_calls;
      return ok;
    };
    void* created = nullptr;
    std::array<std::byte, 48> new_extra{};
    write<int32_t>(new_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 0);
    write<int16_t>(new_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(new_extra, abi_contract::ARB_NEW_REFCON_OFFSET, refcon);
    write<void*>(new_extra, abi_contract::ARB_NEW_OUTPUT_OFFSET, &created);
    uint32_t exception_code = 0;
    if (hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
        nullptr, new_extra.data(), &exception_code) == 0 && exception_code == 0 &&
        hooks().handle_is_live(created) && created != source) {
      ++runtime().arbitrary.new_calls;
    } else if (hooks().handle_is_live(created)) {
      dispose(created);
      ++runtime().arbitrary.interpolation_failures;
      continue;
    } else {
      ++runtime().arbitrary.interpolation_failures;
      continue;
    }
    void* interpolated = created;
    std::array<std::byte, 48> interp_extra{};
    write<int32_t>(interp_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 6);
    write<int16_t>(interp_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(interp_extra, abi_contract::ARB_INTERP_REFCON_OFFSET, refcon);
    write<void*>(interp_extra, abi_contract::ARB_INTERP_LEFT_OFFSET, source);
    write<void*>(interp_extra, abi_contract::ARB_INTERP_RIGHT_OFFSET, source);
    write<double>(interp_extra, abi_contract::ARB_INTERP_AMOUNT_OFFSET, amount);
    write<void*>(interp_extra, abi_contract::ARB_INTERP_OUTPUT_OFFSET, &interpolated);
    exception_code = 0;
    if (hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
        nullptr, interp_extra.data(), &exception_code) != 0 || exception_code != 0 ||
        !hooks().handle_is_live(interpolated) || interpolated == source) {
      if (interpolated != created && hooks().handle_is_live(interpolated)) dispose(interpolated);
      if (hooks().handle_is_live(created)) dispose(created);
      ++runtime().arbitrary.interpolation_failures;
      continue;
    }
    if (interpolated != created && hooks().handle_is_live(created) && !dispose(created)) {
      dispose(interpolated);
      ++runtime().arbitrary.invalid_operations;
      return false;
    }
    if (!dispose(source)) {
      dispose(interpolated);
      ++runtime().arbitrary.invalid_operations;
      return false;
    }
    write<void*>(definition, u + abi_contract::ARBITRARY_VALUE_OFFSET, interpolated);
    ++runtime().arbitrary.interpolation_calls;
  }
  return true;
}

bool roundtrip_arbitrary_values(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output,
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions) {
  constexpr uint32_t kMaxFlatBytes = 16 * 1024 * 1024;
  constexpr std::size_t kGuardBytes = 32;
  for (std::size_t i = 0; i < runtime().records.size(); ++i) {
    if (runtime().records[i].type != 11) continue;
    const std::size_t u = abi_contract::PARAM_U_OFFSET;
    auto& definition = definitions[i + 1];
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + abi_contract::ARBITRARY_REFCON_OFFSET);
    void* source = read<void*>(definition, u + abi_contract::ARBITRARY_VALUE_OFFSET);
    if (!source) continue;
    uint32_t exception_code = 0;
    uint32_t flat_size = 0;
    std::array<std::byte, 48> size_extra{};
    write<int32_t>(size_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 3);
    write<int16_t>(size_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(size_extra, abi_contract::ARB_FLAT_SIZE_REFCON_OFFSET, refcon);
    write<void*>(size_extra, abi_contract::ARB_FLAT_SIZE_VALUE_OFFSET, source);
    write<void*>(size_extra, abi_contract::ARB_FLAT_SIZE_OUTPUT_OFFSET, &flat_size);
    if (!hooks().handle_is_live(source) || hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(),
        nullptr, nullptr, size_extra.data(), &exception_code) != 0 || exception_code != 0 ||
        flat_size == 0 || flat_size > kMaxFlatBytes) {
      ++runtime().arbitrary.roundtrip_failures;
      continue;
    }
    const auto flatten = [&](void* value, std::vector<std::byte>& guarded) {
      guarded.assign(static_cast<std::size_t>(flat_size) + kGuardBytes * 2, std::byte{0xA5});
      auto* buffer = guarded.data() + kGuardBytes;
      std::fill(buffer, buffer + flat_size, std::byte{});
      std::array<std::byte, 48> extra{};
      write<int32_t>(extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 4);
      write<int16_t>(extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
      write<void*>(extra, abi_contract::ARB_FLATTEN_REFCON_OFFSET, refcon);
      write<void*>(extra, abi_contract::ARB_FLATTEN_VALUE_OFFSET, value);
      write<uint32_t>(extra, abi_contract::ARB_FLATTEN_SIZE_OFFSET, flat_size);
      write<void*>(extra, abi_contract::ARB_FLATTEN_BUFFER_OFFSET, buffer);
      exception_code = 0;
      const int32_t error = hooks().invoke_entry(entry, kArbitraryCallback, input.data(),
          output.data(), nullptr, nullptr, extra.data(), &exception_code);
      return error == 0 && exception_code == 0 &&
          std::all_of(guarded.begin(), guarded.begin() + kGuardBytes,
              [](std::byte value) { return value == std::byte{0xA5}; }) &&
          std::all_of(guarded.end() - kGuardBytes, guarded.end(),
              [](std::byte value) { return value == std::byte{0xA5}; });
    };
    std::vector<std::byte> original_flat;
    if (!flatten(source, original_flat)) {
      ++runtime().arbitrary.roundtrip_failures;
      continue;
    }
    void* restored = nullptr;
    std::array<std::byte, 48> unflatten_extra{};
    write<int32_t>(unflatten_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 5);
    write<int16_t>(unflatten_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
    write<void*>(unflatten_extra, abi_contract::ARB_UNFLATTEN_REFCON_OFFSET, refcon);
    write<uint32_t>(unflatten_extra, abi_contract::ARB_UNFLATTEN_SIZE_OFFSET, flat_size);
    write<void*>(unflatten_extra, abi_contract::ARB_UNFLATTEN_BUFFER_OFFSET,
        original_flat.data() + kGuardBytes);
    write<void*>(unflatten_extra, abi_contract::ARB_UNFLATTEN_OUTPUT_OFFSET, &restored);
    exception_code = 0;
    const int32_t unflatten_error = hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
        nullptr, unflatten_extra.data(), &exception_code) != 0 || exception_code != 0 ||
        !hooks().handle_is_live(restored) || restored == source;
    if (unflatten_error) {
      if (hooks().handle_is_live(restored)) {
        std::array<std::byte, 48> dispose_extra{};
        write<int32_t>(dispose_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 1);
        write<int16_t>(dispose_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
        write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_REFCON_OFFSET, refcon);
        write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_VALUE_OFFSET, restored);
        entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data());
        ++runtime().arbitrary.dispose_calls;
      }
      ++runtime().arbitrary.roundtrip_failures;
      continue;
    }
    std::vector<std::byte> restored_flat;
    const bool flattened = flatten(restored, restored_flat);
    const bool bytes_equal = flattened && std::equal(
        original_flat.begin() + kGuardBytes, original_flat.begin() + kGuardBytes + flat_size,
        restored_flat.begin() + kGuardBytes);
    if (!bytes_equal) {
      std::array<std::byte, 48> dispose_extra{};
      write<int32_t>(dispose_extra, abi_contract::ARB_EXTRA_WHICH_FUNCTION_OFFSET, 1);
      write<int16_t>(dispose_extra, abi_contract::ARB_EXTRA_ID_OFFSET, id);
      write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_REFCON_OFFSET, refcon);
      write<void*>(dispose_extra, abi_contract::ARB_DISPOSE_VALUE_OFFSET, restored);
      entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr, dispose_extra.data());
      ++runtime().arbitrary.dispose_calls;
      ++runtime().arbitrary.roundtrip_failures;
      continue;
    }
    if (!dispose_arbitrary_handle(entry, input, output, id, refcon, source)) {
      ++runtime().arbitrary.invalid_operations;
      return false;
    }
    ++runtime().arbitrary.dispose_calls;
    write<void*>(definition, u + abi_contract::ARBITRARY_VALUE_OFFSET, restored);
    ++runtime().arbitrary.roundtrip_calls;
  }
  return true;
}



const char* requested_kind_name(parameters::RequestedKind kind) noexcept {
  switch (kind) {
    case RequestedKind::Integer: return "integer";
    case RequestedKind::Float: return "float";
    case RequestedKind::Color: return "color";
    case RequestedKind::Angle: return "angle";
    case RequestedKind::Point: return "point";
    case RequestedKind::Point3D: return "point3d";
    case RequestedKind::ArbitraryText: return "arbitrary_text";
  }
  return "unknown";
}

bool validate_requested_assignments(const parameters::RequestedAssignments& requested) {
  return validate_requested_assignments(requested, nullptr);
}

bool validate_requested_assignments(const parameters::RequestedAssignments& requested,
                                    AssignmentValidationFailure* failure) {
  if (failure) *failure = {};
  for (const auto& assignment : requested) {
    const auto reject = [&](const char* reason, int32_t type = -1) {
      if (failure) *failure = {assignment.index, type, assignment.kind, reason};
      return false;
    };
    if (assignment.index < 1 || static_cast<std::size_t>(assignment.index) > runtime().records.size())
      return reject("slot_out_of_range");
    const auto& descriptor = runtime().records[static_cast<std::size_t>(assignment.index - 1)];
    const bool integer_compatible = descriptor.type == 1 || descriptor.type == 4 ||
        descriptor.type == 7 || descriptor.type == 12;
    const bool float_compatible = descriptor.type == 2 || descriptor.type == 10;
    const bool color_compatible = descriptor.type == 5;
    const bool angle_compatible = descriptor.type == 3;
    const bool point_compatible = descriptor.type == 6;
    const bool point3d_compatible = descriptor.type == 18;
    const bool arbitrary_compatible = descriptor.type == 11;
    if ((assignment.kind == parameters::RequestedKind::Integer && !integer_compatible) ||
        (assignment.kind == parameters::RequestedKind::Float && !float_compatible) ||
        (assignment.kind == parameters::RequestedKind::Color && !color_compatible) ||
        (assignment.kind == parameters::RequestedKind::Angle && !angle_compatible) ||
        (assignment.kind == parameters::RequestedKind::Point && !point_compatible) ||
        (assignment.kind == parameters::RequestedKind::Point3D && !point3d_compatible) ||
        (assignment.kind == parameters::RequestedKind::ArbitraryText && !arbitrary_compatible))
      return reject("kind_mismatch", descriptor.type);
    if (assignment.has_float_color &&
        (assignment.kind != parameters::RequestedKind::Color ||
         std::any_of(assignment.float_color.begin(), assignment.float_color.end(),
                     [](float value) { return !std::isfinite(value); })))
      return reject("invalid_float_color", descriptor.type);
    const double value = assignment.use_declared_default
        ? descriptor.default_value : assignment.value;
    if (assignment.use_declared_default &&
        (assignment.kind != RequestedKind::Integer && assignment.kind != RequestedKind::Float))
      return reject("default_kind_mismatch", descriptor.type);
    if (assignment.kind == RequestedKind::Integer || assignment.kind == RequestedKind::Float) {
      if (!std::isfinite(value) ||
          (assignment.kind == RequestedKind::Integer &&
           (std::trunc(value) != value || value < INT32_MIN || value > INT32_MAX)) ||
          (descriptor.type == 4 && value != 0.0 && value != 1.0) ||
          (descriptor.type == 2 && (value * 65536.0 < INT32_MIN || value * 65536.0 > INT32_MAX)))
        return reject("numeric_representation", descriptor.type);
    }
    if (assignment.use_declared_default && has_default_range(descriptor) &&
        !valid_default_range(descriptor))
      return reject("numeric_range", descriptor.type);
    if (descriptor.type == 12 && (value < 0 ||
         value > static_cast<double>(hooks().active_mask_count()) ||
         std::trunc(value) != value))
      return reject("mask_range", descriptor.type);
    if ((assignment.kind == parameters::RequestedKind::Integer || assignment.kind == parameters::RequestedKind::Float) && descriptor.type != 12 && (!descriptor.has_numeric ||
         (!assignment.use_declared_default && (value < descriptor.valid_min || value > descriptor.valid_max))))
      return reject("numeric_range", descriptor.type);
  }
  return true;
}

void initialize_parameter_definitions(
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions,
    int32_t layer_width, int32_t layer_height) {
  // A POINT/POINT_3D default is a percentage of the layer size (SDK
  // PF_PointDef `x_dephault` /* percentage */); the value the plug-in reads
  // (`x_value`) is in pixels. Convert here so an effect that derives geometry
  // from a corner default - Corner Pin's 100% corner, Bezier Warp's control
  // points - sees the layer's actual extent rather than the raw percentage
  // read as pixels (issue #326 chain: Corner Pin returned a 100x100 FRAME_SETUP
  // extent for a 256x144 layer). Zero dims keep the raw percentage for the
  // point-less audio/UI paths.
  const auto point_pixels = [&](double percent, int32_t extent) {
    return layer_width > 0 && layer_height > 0
        ? percent / 100.0 * extent : percent;
  };
  const auto to_fixed = [](double value) {
    return static_cast<int32_t>(
        std::round(std::clamp(value, -32768.0, 32767.0) * 65536.0));
  };
  auto staged_records = runtime().records;
  for (std::size_t i = 0; i < staged_records.size(); ++i) {
    auto& record = staged_records[i];
    const double default_value = effective_default(record);
    definitions[i + 1] = record.raw;
    aexcompat::memory_fill::parameter(definitions[i + 1].data(), record.type);
    if (record.type == 0)
      std::memset(definitions[i + 1].data() + 56, 0, 40);
    else if (record.type == 1 || record.type == 7) {
      const auto encoded = static_cast<int32_t>(default_value);
      write<int32_t>(definitions[i + 1], abi_contract::PARAM_U_OFFSET, encoded);
      record.has_current = true;
      record.current_value = encoded;
    }
    else if (record.type == 4) {
      write<int32_t>(definitions[i + 1], abi_contract::PARAM_U_OFFSET, record.default_value != 0 ? 1 : 0);
      record.has_current = true;
      record.current_value = record.default_value != 0 ? 1.0 : 0.0;
    }
    else if (record.type == 2) {
      // Fixed sliders use the full signed 16.16 range, including the fractional
      // interval above 32767. Geometry's narrower clamp must not discard it.
      const auto encoded = static_cast<int32_t>(std::round(std::clamp(
          default_value * 65536.0, static_cast<double>(INT32_MIN),
          static_cast<double>(INT32_MAX))));
      write<int32_t>(definitions[i + 1], abi_contract::PARAM_U_OFFSET, encoded);
      record.has_current = true;
      record.current_value = encoded / 65536.0;
    }
    else if (record.type == 10) {
      write<double>(definitions[i + 1], abi_contract::PARAM_U_OFFSET, default_value);
      record.has_current = true;
      record.current_value = default_value;
    }
    else if (record.type == 5) {
      // PARAMS_SETUP may initialize value independently from dephault. A fresh
      // instance must materialize the declaration before sparse edits apply.
      record.current_color = record.default_color;
      std::memcpy(definitions[i + 1].data() + 56, record.default_color.data(),
                  record.default_color.size());
      for (std::size_t channel = 0; channel < 4; ++channel)
        record.current_float_color[channel] =
            record.current_color[channel] / 255.0f;
    }
    else if (record.type == 3) {
      const auto encoded = to_fixed(record.default_components[0]);
      write<int32_t>(definitions[i + 1], abi_contract::PARAM_U_OFFSET, encoded);
      record.current_components[0] = encoded / 65536.0;
    }
    else if (record.type == 6) {
      // Fixed (16.16) pixels: clamp to the same +/-32768 pixel range the point
      // override path parses (worker_l2_payload_parsers) so a pathological
      // percentage scaled by the extent cannot overflow the int32 value.
      const auto encoded_x = to_fixed(
          point_pixels(record.default_components[0], layer_width));
      const auto encoded_y = to_fixed(
          point_pixels(record.default_components[1], layer_height));
      write<int32_t>(definitions[i + 1], abi_contract::PARAM_U_OFFSET, encoded_x);
      write<int32_t>(definitions[i + 1], abi_contract::PARAM_U_OFFSET + abi_offsets::kPointValueY, encoded_y);
      record.current_components[0] = encoded_x / 65536.0;
      record.current_components[1] = encoded_y / 65536.0;
    } else if (record.type == 18) {
      // x is % of width, y and z are both % of _height_ (SDK PF_Point3DDef).
      const std::array<int32_t, 3> extents{layer_width, layer_height, layer_height};
      for (int component = 0; component < 3; ++component) {
        record.current_components[component] = point_pixels(record.default_components[component], extents[component]);
        write<double>(definitions[i + 1], abi_contract::PARAM_U_OFFSET + component * sizeof(double),
            record.current_components[component]);
      }
    }
    else if (record.type == 12) {
      const int32_t index = static_cast<int32_t>(record.default_value);
      int32_t mask_id = 0;
      if (index > 0) hooks().active_mask_id(static_cast<std::size_t>(index - 1), &mask_id);
      write<int32_t>(definitions[i + 1], abi_contract::PARAM_U_OFFSET, mask_id);
    }
  }
  runtime().records = std::move(staged_records);
}

bool apply_requested_assignments(
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions,
    const parameters::RequestedAssignments& requested,
    int32_t layer_width, int32_t layer_height) {
  if (!validate_requested_assignments(requested)) return false;
  auto staged_definitions = definitions;
  auto staged_records = runtime().records;
  // Point assignments carry the same percentage-of-layer unit as their
  // defaults (the multifilter sources them from discovery, which reads the
  // SDK `x_dephault` percentage); convert to the pixel `x_value` the plug-in
  // reads, matching initialize_parameter_definitions. Zero dims keep the raw
  // value for the point-less audio/UI/L2 paths (issue #1061 chain).
  const auto point_pixels = [&](double percent, int32_t extent) {
    return layer_width > 0 && layer_height > 0 ? percent / 100.0 * extent : percent;
  };
  const auto to_fixed = [](double pixels) {
    return static_cast<int32_t>(std::round(std::clamp(pixels, -32768.0, 32767.0) * 65536.0));
  };
  for (const auto& assignment : requested) {
    const auto slot = static_cast<std::size_t>(assignment.index);
    const auto type = staged_records[slot - 1].type;
    auto& record = staged_records[slot - 1];
    const double value = assignment.use_declared_default ? effective_default(record) : assignment.value;
    if (type == 11 && assignment.kind == parameters::RequestedKind::ArbitraryText) continue;
    if (type == 1 || type == 4 || type == 7) {
      const auto encoded = static_cast<int32_t>(value);
      write<int32_t>(staged_definitions[slot], abi_contract::PARAM_U_OFFSET, encoded);
      record.has_current = true;
      record.current_value = encoded;
    }
    else if (type == 12) {
      const int32_t index = static_cast<int32_t>(value);
      int32_t mask_id = 0;
      if (index > 0 && !hooks().active_mask_id(static_cast<std::size_t>(index - 1), &mask_id)) return false;
      write<int32_t>(staged_definitions[slot], abi_contract::PARAM_U_OFFSET, mask_id);
    }
    else if (type == 2) {
      const double fixed = value * 65536.0;
      if (!std::isfinite(fixed) || fixed < INT32_MIN || fixed > INT32_MAX) return false;
      const auto encoded = static_cast<int32_t>(std::round(fixed));
      write<int32_t>(staged_definitions[slot], abi_contract::PARAM_U_OFFSET, encoded);
      record.has_current = true;
      record.current_value = encoded / 65536.0;
    } else if (type == 10) {
      write<double>(staged_definitions[slot], abi_contract::PARAM_U_OFFSET, value);
      record.has_current = true;
      record.current_value = value;
    }
    else if (type == 5) {
      std::memcpy(staged_definitions[slot].data() + 56, assignment.color.data(), assignment.color.size());
      record.current_color = assignment.color;
      for (std::size_t channel = 0; channel < 4; ++channel)
        record.current_float_color[channel] = assignment.has_float_color
              ? assignment.float_color[channel] : assignment.color[channel] / 255.0f;
    }
    else if (type == 3) {
      const auto encoded = to_fixed(assignment.components[0]);
      write<int32_t>(staged_definitions[slot], abi_contract::PARAM_U_OFFSET, encoded);
      record.current_components[0] = encoded / 65536.0;
    }
    else if (type == 6) {
      const auto encoded_x = to_fixed(
          point_pixels(assignment.components[0], layer_width));
      const auto encoded_y = to_fixed(
          point_pixels(assignment.components[1], layer_height));
      write<int32_t>(staged_definitions[slot], abi_contract::PARAM_U_OFFSET, encoded_x);
      write<int32_t>(staged_definitions[slot], abi_contract::PARAM_U_OFFSET + abi_offsets::kPointValueY,
          encoded_y);
      record.current_components[0] = encoded_x / 65536.0;
      record.current_components[1] = encoded_y / 65536.0;
    } else if (type == 18) {
      const std::array<int32_t, 3> extents{layer_width, layer_height, layer_height};
      for (int component = 0; component < 3; ++component) {
        record.current_components[component] = point_pixels(assignment.components[component], extents[component]);
        write<double>(staged_definitions[slot], abi_contract::PARAM_U_OFFSET + component * sizeof(double),
            record.current_components[component]);
      }
    }
    else
      return false;
  }
  definitions = std::move(staged_definitions);
  runtime().records = std::move(staged_records);
  return true;
}

double requested_value(const parameters::RequestedAssignments& requested, const wchar_t* id) {
  const auto found = std::find_if(requested.begin(), requested.end(), [id](const auto& assignment) {
    return assignment.id == id;
  });
  return found == requested.end() || found->kind == parameters::RequestedKind::Color ? 0.0 : found->value;
}

std::string requested_parameters_json(const parameters::RequestedAssignments& requested) {
  std::ostringstream output;
  output << "[";
  for (std::size_t i = 0; i < requested.size(); ++i) {
    if (i != 0) output << ",";
    const auto& assignment = requested[i];
    std::string id;
    id.reserve(assignment.id.size());
    for (const wchar_t character : assignment.id) id.push_back(static_cast<char>(character));
    output << "{\"id\":\"" << id << "\",\"slot\":" << assignment.index
           << ",\"kind\":\""
           << (assignment.kind == parameters::RequestedKind::Integer ? "integer" :
               assignment.kind == parameters::RequestedKind::Float ? "float" :
               assignment.kind == parameters::RequestedKind::Color ? "color" :
               assignment.kind == parameters::RequestedKind::Angle ? "angle" :
               assignment.kind == parameters::RequestedKind::Point ? "point" :
               assignment.kind == parameters::RequestedKind::Point3D ? "point3d" : "arbitrary_text")
           << "\",\"value\":";
    if (assignment.use_declared_default)
      output << "null";
    else if (assignment.kind == parameters::RequestedKind::Integer)
      output << static_cast<int32_t>(assignment.value);
    else if (assignment.kind == parameters::RequestedKind::Float)
      output << std::setprecision(17) << assignment.value;
    else if (assignment.kind == parameters::RequestedKind::Color)
      output << "{\"alpha\":" << static_cast<unsigned>(assignment.color[0])
             << ",\"red\":" << static_cast<unsigned>(assignment.color[1])
             << ",\"green\":" << static_cast<unsigned>(assignment.color[2])
             << ",\"blue\":" << static_cast<unsigned>(assignment.color[3]) << "}";
    else if (assignment.kind == parameters::RequestedKind::ArbitraryText)
      output << "{\"bytes\":" << assignment.text.size() << "}";
    else {
      const int count = assignment.kind == parameters::RequestedKind::Point3D ? 3 :
          (assignment.kind == parameters::RequestedKind::Point ? 2 : 1);
      output << "[";
      for (int component = 0; component < count; ++component) {
        if (component) output << ",";
        output << std::setprecision(17) << assignment.components[component];
      }
      output << "]";
    }
    if (assignment.use_declared_default) output << ",\"use_declared_default\":true";
    if (assignment.has_float_color) {
      output << ",\"float_argb_bits\":[";
      for (std::size_t channel = 0; channel < 4; ++channel) {
        uint32_t bits{};
        std::memcpy(&bits, &assignment.float_color[channel], sizeof(bits));
        if (channel) output << ",";
        output << bits;
      }
      output << "]";
    }
    output << "}";
  }
  output << "]";
  return output.str();
}


}  // namespace aexcompat::worker_runtime::parameter_execution

// The PF add_param discovery callback and the options-button-name callback
// moved from worker_main (issue #170); parameter records and UI state
// already live in worker_runtime::parameters::state().
namespace aexcompat::l2_detail {
extern "C" int32_t __cdecl set_options_button_name(void*, const char*);
namespace {
constexpr std::size_t kMaxParams =
    aexcompat::worker_runtime::parameters::kMaxParameterCount;
constexpr std::size_t kParamSize =
    aexcompat::worker_runtime::parameters::kDefinitionSize;
constexpr std::size_t kParamType = abi_contract::PARAM_PARAM_TYPE_OFFSET;
constexpr std::size_t kParamName = abi_contract::PARAM_NAME_OFFSET;
constexpr std::size_t kParamNameSize = 32;
constexpr std::size_t kParamFlags = abi_contract::PARAM_FLAGS_OFFSET;
using aexcompat::worker_runtime::parameters::ParamRecord;
auto& g_param_state = aexcompat::worker_runtime::parameters::state();
auto& g_params = g_param_state.records;
auto& g_options_button_name = g_param_state.ui.options_button_name;
auto& g_options_button_name_calls = g_param_state.ui.options_button_name_calls;
template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}
}  // namespace

int32_t __cdecl add_param(void*, int32_t index, void* definition) {
  if (!definition || g_params.size() >= kMaxParams) return 4;
  std::array<std::byte, kParamSize> bytes{};
  std::memcpy(bytes.data(), definition, bytes.size());
  const char* name = reinterpret_cast<const char*>(bytes.data() + kParamName);
  const auto length = strnlen_s(name, kParamNameSize);
  if (extended_diag_enabled()) {
    std::cerr << "extended_diag:add_param index=" << index
              << " type=" << read<int32_t>(bytes, kParamType) << " name=\""
              << std::string(name, length) << "\"";
    // A layer parameter carries its `PF_LayerDefault` in the `dephault` field
    // that ends the `PF_LayerDef` sitting in the union (SDK AE_Effect.h:
    // MYSELF = -1, NONE = 0). Which of the two a plug-in declared decides
    // whether an unconnected slot is transparent or is the effect's own input,
    // and the trace had no way to show it (issue #1285).
    if (read<int32_t>(bytes, kParamType) == 0)
      std::cerr << " layer_dephault=" << read<int32_t>(bytes, abi_contract::PARAM_U_OFFSET + abi_offsets::kLayerDefault);
    std::cerr << "\n" << std::flush;
  }
  const int32_t host_index = index < 0 ? static_cast<int32_t>(g_params.size() + 1) : index;
  if (host_index <= 0 || host_index > static_cast<int32_t>(kMaxParams) ||
      std::any_of(g_params.begin(), g_params.end(),
                  [host_index](const auto& param) { return param.index == host_index; })) return 4;
  ParamRecord record{host_index, read<int32_t>(bytes, abi_contract::PARAM_UU_OFFSET), read<int32_t>(bytes, kParamType),
                     read<uint32_t>(bytes, kParamFlags), std::string(name, length)};
  constexpr std::size_t u = abi_contract::PARAM_U_OFFSET;
  if (record.type == 1) {
    record.has_numeric = true;
    record.valid_min = read<int32_t>(bytes, u + abi_contract::SLIDER_VALID_MIN_OFFSET);
    record.valid_max = read<int32_t>(bytes, u + abi_contract::SLIDER_VALID_MAX_OFFSET);
    record.slider_min = read<int32_t>(bytes, u + abi_contract::SLIDER_SLIDER_MIN_OFFSET);
    record.slider_max = read<int32_t>(bytes, u + abi_contract::SLIDER_SLIDER_MAX_OFFSET);
    record.default_value = read<int32_t>(bytes, u + abi_contract::SLIDER_DEFAULT_OFFSET);
  } else if (record.type == 2) {
    record.has_numeric = true;
    record.has_current = true;
    record.current_value = read<int32_t>(bytes, u) / 65536.0;
    record.valid_min = read<int32_t>(bytes, u + abi_contract::SLIDER_VALID_MIN_OFFSET) / 65536.0;
    record.valid_max = read<int32_t>(bytes, u + abi_contract::SLIDER_VALID_MAX_OFFSET) / 65536.0;
    record.slider_min = read<int32_t>(bytes, u + abi_contract::SLIDER_SLIDER_MIN_OFFSET) / 65536.0;
    record.slider_max = read<int32_t>(bytes, u + abi_contract::SLIDER_SLIDER_MAX_OFFSET) / 65536.0;
    record.default_value = read<int32_t>(bytes, u + abi_contract::SLIDER_DEFAULT_OFFSET) / 65536.0;
    record.precision = read<int16_t>(bytes, u + abi_offsets::kFixedSliderPrecision);
  } else if (record.type == 7) {
    record.has_numeric = true;
    record.valid_min = 1;
    record.valid_max = read<int16_t>(bytes, u + abi_contract::POPUP_NUM_CHOICES_OFFSET);
    record.slider_min = record.valid_min;
    record.slider_max = record.valid_max;
    // A PF popup value is 1-based (SDK PF_PopupDef). Reshape declares its
    // Elasticity (9 choices) and Interpolation Method (3 choices) popups with
    // dephault = value = 0; AE 2026 reads both back as 1 through ExtendScript
    // with no user edit and renders the effect (issue #1253,
    // docs/MASKLESS_PATH_EFFECTS_OBSERVATION_2026-08-17.md; that RENDER
    // receives 1 is inferred from that render passing through). Handing the
    // plug-in the raw 0 made its RENDER map "Interpolation Method - 1" to a
    // mode its grid generator rejects, and FLO_DoDistortion answered
    // PF_Err_OUT_OF_MEMORY. Lift a non-positive declared default to the first
    // choice; a default above num_choices is not an observed case and is left
    // as declared.
    const int16_t declared_default = read<int16_t>(bytes, u + abi_contract::POPUP_DEFAULT_OFFSET);
    record.default_value = declared_default < 1 ? 1 : declared_default;
    const char* choices = read<const char*>(bytes, u + abi_contract::POPUP_NAMES_OFFSET);
    if (choices) record.choices.assign(choices, strnlen_s(choices, 4096));
  } else if (record.type == 4) {
    record.has_numeric = true;
    record.has_current = true;
    record.valid_min = 0;
    record.valid_max = 1;
    record.slider_min = 0;
    record.slider_max = 1;
    record.default_value = read<uint8_t>(bytes, u + abi_contract::CHECKBOX_DEFAULT_OFFSET) ? 1 : 0;
    record.current_value = read<int32_t>(bytes, u) != 0 ? 1 : 0;
    const char* label = read<const char*>(bytes, u + abi_contract::CHECKBOX_LABEL_OFFSET);
    if (label) record.label.assign(label, strnlen_s(label, 4096));
  } else if (record.type == 0) {
    record.layer_default = read<int32_t>(bytes, u + abi_offsets::kLayerDefault);
  } else if (record.type == 12) {
    record.has_numeric = true;
    record.valid_min = 0;
    record.valid_max = 1024;
    record.slider_min = 0;
    record.slider_max = 1024;
    record.default_value = read<int32_t>(bytes, u + abi_offsets::kPathDefault);
  } else if (record.type == 10) {
    record.has_numeric = true;
    record.valid_min = read<float>(bytes, u + abi_contract::FLOAT_SLIDER_VALID_MIN_OFFSET);
    record.valid_max = read<float>(bytes, u + abi_contract::FLOAT_SLIDER_VALID_MAX_OFFSET);
    record.slider_min = read<float>(bytes, u + abi_contract::FLOAT_SLIDER_SLIDER_MIN_OFFSET);
    record.slider_max = read<float>(bytes, u + abi_contract::FLOAT_SLIDER_SLIDER_MAX_OFFSET);
    record.default_value = read<float>(bytes, u + abi_contract::FLOAT_SLIDER_DEFAULT_OFFSET);
    record.precision = read<int16_t>(bytes, u + abi_contract::FLOAT_SLIDER_PRECISION_OFFSET);
  } else if (record.type == 5) {
    record.has_color = true;
    std::memcpy(record.current_color.data(), bytes.data() + u, record.current_color.size());
    std::memcpy(record.default_color.data(), bytes.data() + u + 4, record.default_color.size());
    for (std::size_t channel = 0; channel < 4; ++channel) {
      record.current_float_color[channel] = record.current_color[channel] / 255.0f;
      record.default_float_color[channel] = record.default_color[channel] / 255.0f;
    }
  } else if (record.type == 3) {
    record.component_count = 1;
    record.current_components[0] = read<int32_t>(bytes, u) / 65536.0;
    record.default_components[0] = read<int32_t>(bytes, u + abi_offsets::kAngleDefault) / 65536.0;
  } else if (record.type == 6) {
    record.component_count = 2;
    record.current_components[0] = read<int32_t>(bytes, u) / 65536.0;
    record.current_components[1] = read<int32_t>(bytes, u + abi_offsets::kPointValueY) / 65536.0;
    record.default_components[0] = read<int32_t>(bytes, u + abi_offsets::kPointDefaultX) / 65536.0;
    record.default_components[1] = read<int32_t>(bytes, u + abi_offsets::kPointDefaultY) / 65536.0;
  } else if (record.type == 18) {
    record.component_count = 3;
    for (int component = 0; component < 3; ++component) {
      record.current_components[component] = read<double>(bytes, u + abi_offsets::kPoint3DValueX + component * sizeof(double));
      record.default_components[component] = read<double>(bytes, u + abi_offsets::kPoint3DDefaultX + component * sizeof(double));
    }
  } else if (record.type == 15) {
    const char* label = read<const char*>(bytes, u + abi_offsets::kButtonLabel);
    if (label) record.label.assign(label, strnlen_s(label, 4096));
  }
  record.raw = bytes;
  g_params.push_back(std::move(record));
  return 0;
}

int32_t __cdecl set_options_button_name(void* effect_ref, const char* name) {
  if (!effect_ref || !name) return 4;
  const std::size_t length = strnlen_s(name, 256);
  if (length == 256) return 4;
  g_options_button_name.assign(name, length);
  ++g_options_button_name_calls;
  return 0;
}

}  // namespace aexcompat::l2_detail
