#include "worker_parameter_execution.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>
#include <iomanip>
#include <sstream>
#include <vector>

namespace aexcompat::worker_runtime::parameter_execution {
using parameters::RequestedAssignments;
using parameters::RequestedKind;
namespace {
constexpr int32_t kArbitraryCallback = 22;
constexpr std::size_t kInCurrentTime = 224;
Hooks g_hooks{};
Hooks& hooks() { return g_hooks; }
parameters::State& runtime() { return parameters::state(); }
template <typename T, std::size_t N> T read(const std::array<std::byte, N>& b, std::size_t o) { T v{}; std::memcpy(&v, b.data()+o, sizeof(v)); return v; }
template <typename T, std::size_t N> void write(std::array<std::byte, N>& b, std::size_t o, const T& v) { std::memcpy(b.data()+o, &v, sizeof(v)); }
}  // namespace

bool configure_hooks(const Hooks& value) noexcept { if (!value.invoke_entry || !value.handle_is_live || !value.active_mask_count || !value.active_mask_id) return false; g_hooks=value; return true; }

bool initialize_arbitrary_values(EffectEntry entry,
    std::array<std::byte, kInputSize>& input,
    std::array<std::byte, kOutputSize>& output,
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions) {
  for (std::size_t i = 0; i < runtime().records.size(); ++i) {
    if (runtime().records[i].type != 11) continue;
    const std::size_t u = 56;
    const int16_t id = read<int16_t>(definitions[i + 1], u);
    void* source = read<void*>(definitions[i + 1], u + 8);
    void* refcon = read<void*>(definitions[i + 1], u + 24);
    void* destination = nullptr;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, 0, 2);
    write<int16_t>(extra, 4, id);
    write<void*>(extra, 8, refcon);
    write<void*>(extra, 16, source);
    write<void*>(extra, 24, &destination);
    const int32_t error = source
        ? entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr, extra.data())
        : 4;
    if (error != 0 || !destination || destination == source) {
      ++runtime().arbitrary.print_failures;
      return false;
    }
    write<void*>(definitions[i + 1], u + 16, destination);
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
    constexpr std::size_t u = 56;
    auto& definition = definitions[static_cast<std::size_t>(assignment.index)];
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + 24);
    void* previous = read<void*>(definition, u + 16);
    void* scanned = nullptr;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, 0, 10);
    write<int16_t>(extra, 4, id);
    write<void*>(extra, 8, refcon);
    write<const char*>(extra, 16, assignment.text.data());
    write<uint32_t>(extra, 24, static_cast<uint32_t>(assignment.text.size()));
    write<void*>(extra, 32, &scanned);
    uint32_t exception_code = 0;
    const bool scan_error = hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(),
        nullptr, nullptr, extra.data(), &exception_code) != 0 || exception_code != 0 ||
        !hooks().handle_is_live(scanned) || scanned == previous;
    auto dispose_scanned = [&]() {
      if (!hooks().handle_is_live(scanned) || scanned == previous) return true;
      std::array<std::byte, 48> scanned_dispose_extra{};
      write<int32_t>(scanned_dispose_extra, 0, 1);
      write<int16_t>(scanned_dispose_extra, 4, id);
      write<void*>(scanned_dispose_extra, 8, refcon);
      write<void*>(scanned_dispose_extra, 16, scanned);
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
    write<int32_t>(dispose_extra, 0, 1);
    write<int16_t>(dispose_extra, 4, id);
    write<void*>(dispose_extra, 8, refcon);
    write<void*>(dispose_extra, 16, previous);
    if (entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data()) != 0) {
      dispose_scanned();
      ++runtime().arbitrary.invalid_operations;
      return false;
    }
    ++runtime().arbitrary.dispose_calls;
    ++runtime().arbitrary.scan_calls;
    write<void*>(definition, u + 16, scanned);
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
    const std::size_t u = 56;
    void* value = read<void*>(definitions[i + 1], u + 16);
    if (!value) continue;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, 0, 1);
    write<int16_t>(extra, 4, read<int16_t>(definitions[i + 1], u));
    write<void*>(extra, 8, read<void*>(definitions[i + 1], u + 24));
    write<void*>(extra, 16, value);
    const int32_t error = entry(kArbitraryCallback, input.data(), output.data(),
                                nullptr, nullptr, extra.data());
    write<void*>(definitions[i + 1], u + 16, nullptr);
    if (error == 0) ++runtime().arbitrary.dispose_calls;
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
    const std::size_t u = 56;
    void* value = read<void*>(param.raw, u + 8);
    if (!value) continue;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, 0, 1);
    write<int16_t>(extra, 4, read<int16_t>(param.raw, u));
    write<void*>(extra, 8, read<void*>(param.raw, u + 24));
    write<void*>(extra, 16, value);
    const int32_t error = entry(kArbitraryCallback, input.data(), output.data(),
                                nullptr, nullptr, extra.data());
    write<void*>(param.raw, u + 8, nullptr);
    if (error == 0) ++runtime().arbitrary.dispose_calls;
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
    constexpr std::size_t u = 56;
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + 24);
    const auto dispose = [&](void* value) {
      if (!hooks().handle_is_live(value)) return false;
      std::array<std::byte, 48> extra{};
      write<int32_t>(extra, 0, 1); write<int16_t>(extra, 4, id);
      write<void*>(extra, 8, refcon); write<void*>(extra, 16, value);
      uint32_t exception_code = 0;
      const bool ok = hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(),
          nullptr, nullptr, extra.data(), &exception_code) == 0 && exception_code == 0;
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
      write<int32_t>(extra, 0, 5); write<int16_t>(extra, 4, id);
      write<void*>(extra, 8, refcon);
      write<uint32_t>(extra, 16, static_cast<uint32_t>(key.arbitrary.size()));
      write<const unsigned char*>(extra, 24, key.arbitrary.data());
      write<void*>(extra, 32, &value);
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
        write<int32_t>(new_extra, 0, 0); write<int16_t>(new_extra, 4, id);
        write<void*>(new_extra, 8, refcon); write<void*>(new_extra, 16, &preallocated);
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
        write<int32_t>(extra, 0, 6); write<int16_t>(extra, 4, id);
        write<void*>(extra, 8, refcon); write<void*>(extra, 16, owned[selected]);
        write<void*>(extra, 24, owned[right]); write<double>(extra, 32, amount);
        write<void*>(extra, 40, &replacement);
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
    void* previous = read<void*>(definition, u + 16);
    if (!dispose(previous)) {
      if (std::find(owned.begin(), owned.end(), replacement) == owned.end()) dispose(replacement);
      cleanup();
      return false;
    }
    write<void*>(definition, u + 16, replacement);
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
    const std::size_t u = 56;
    void* value = read<void*>(param.raw, u + 8);
    void* refcon = read<void*>(param.raw, u + 24);
    uint32_t print_size = 0;
    std::array<std::byte, 48> size_extra{};
    write<int32_t>(size_extra, 0, 8);
    write<int16_t>(size_extra, 4, read<int16_t>(param.raw, u));
    write<void*>(size_extra, 8, refcon);
    write<void*>(size_extra, 16, value);
    write<void*>(size_extra, 24, &print_size);
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
    write<int32_t>(print_extra, 0, 9);
    write<int16_t>(print_extra, 4, read<int16_t>(param.raw, u));
    write<void*>(print_extra, 8, refcon);
    write<int32_t>(print_extra, 16, 0);
    write<void*>(print_extra, 24, value);
    write<uint32_t>(print_extra, 32, print_size);
    write<void*>(print_extra, 40, buffer.data());
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
    constexpr std::size_t u = 56;
    auto& definition = definitions[i + 1];
    const int16_t id = read<int16_t>(definition, u);
    void* source = read<void*>(definition, u + 16);
    void* refcon = read<void*>(definition, u + 24);
    void* scanned = nullptr;
    std::array<std::byte, 48> scan_extra{};
    write<int32_t>(scan_extra, 0, 10);
    write<int16_t>(scan_extra, 4, id);
    write<void*>(scan_extra, 8, refcon);
    write<const char*>(scan_extra, 16, runtime().records[i].arbitrary_summary.c_str());
    write<uint32_t>(scan_extra, 24,
        static_cast<uint32_t>(runtime().records[i].arbitrary_summary.size()));
    write<void*>(scan_extra, 32, &scanned);
    uint32_t exception_code = 0;
    const bool created = hooks().invoke_entry(entry, kArbitraryCallback, input.data(),
        output.data(), nullptr, nullptr, scan_extra.data(), &exception_code) == 0 &&
        exception_code == 0 && hooks().handle_is_live(scanned) && scanned != source;
    if (!created) {
      if (hooks().handle_is_live(scanned)) {
        std::array<std::byte, 48> dispose_extra{};
        write<int32_t>(dispose_extra, 0, 1);
        write<int16_t>(dispose_extra, 4, id);
        write<void*>(dispose_extra, 8, refcon);
        write<void*>(dispose_extra, 16, scanned);
        entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data());
        ++runtime().arbitrary.dispose_calls;
      }
      ++runtime().arbitrary.scan_failures;
      continue;
    }
    int32_t comparison = 3;
    std::array<std::byte, 48> compare_extra{};
    write<int32_t>(compare_extra, 0, 7);
    write<int16_t>(compare_extra, 4, id);
    write<void*>(compare_extra, 8, refcon);
    write<void*>(compare_extra, 16, source);
    write<void*>(compare_extra, 24, scanned);
    write<void*>(compare_extra, 32, &comparison);
    exception_code = 0;
    const bool equal = hooks().invoke_entry(entry, kArbitraryCallback, input.data(),
        output.data(), nullptr, nullptr, compare_extra.data(), &exception_code) == 0 &&
        exception_code == 0 && comparison == 0;
    std::array<std::byte, 48> dispose_extra{};
    write<int32_t>(dispose_extra, 0, 1);
    write<int16_t>(dispose_extra, 4, id);
    write<void*>(dispose_extra, 8, refcon);
    write<void*>(dispose_extra, 16, scanned);
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
  const int32_t total_time = read<int32_t>(input, 232);
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
    const std::size_t u = 56;
    auto& definition = definitions[i + 1];
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + 24);
    void* source = read<void*>(definition, u + 16);
    if (!hooks().handle_is_live(source)) {
      ++runtime().arbitrary.interpolation_failures;
      return false;
    }
    const auto dispose = [&](void* value) {
      if (!hooks().handle_is_live(value)) return false;
      std::array<std::byte, 48> extra{};
      write<int32_t>(extra, 0, 1);
      write<int16_t>(extra, 4, id);
      write<void*>(extra, 8, refcon);
      write<void*>(extra, 16, value);
      const bool ok = entry(kArbitraryCallback, input.data(), output.data(), nullptr,
                            nullptr, extra.data()) == 0;
      if (ok) ++runtime().arbitrary.dispose_calls;
      return ok;
    };
    void* created = nullptr;
    std::array<std::byte, 48> new_extra{};
    write<int32_t>(new_extra, 0, 0);
    write<int16_t>(new_extra, 4, id);
    write<void*>(new_extra, 8, refcon);
    write<void*>(new_extra, 16, &created);
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
    write<int32_t>(interp_extra, 0, 6);
    write<int16_t>(interp_extra, 4, id);
    write<void*>(interp_extra, 8, refcon);
    write<void*>(interp_extra, 16, source);
    write<void*>(interp_extra, 24, source);
    write<double>(interp_extra, 32, amount);
    write<void*>(interp_extra, 40, &interpolated);
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
    write<void*>(definition, u + 16, interpolated);
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
    const std::size_t u = 56;
    auto& definition = definitions[i + 1];
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + 24);
    void* source = read<void*>(definition, u + 16);
    uint32_t exception_code = 0;
    uint32_t flat_size = 0;
    std::array<std::byte, 48> size_extra{};
    write<int32_t>(size_extra, 0, 3);
    write<int16_t>(size_extra, 4, id);
    write<void*>(size_extra, 8, refcon);
    write<void*>(size_extra, 16, source);
    write<void*>(size_extra, 24, &flat_size);
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
      write<int32_t>(extra, 0, 4);
      write<int16_t>(extra, 4, id);
      write<void*>(extra, 8, refcon);
      write<void*>(extra, 16, value);
      write<uint32_t>(extra, 24, flat_size);
      write<void*>(extra, 32, buffer);
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
    write<int32_t>(unflatten_extra, 0, 5);
    write<int16_t>(unflatten_extra, 4, id);
    write<void*>(unflatten_extra, 8, refcon);
    write<uint32_t>(unflatten_extra, 16, flat_size);
    write<void*>(unflatten_extra, 24, original_flat.data() + kGuardBytes);
    write<void*>(unflatten_extra, 32, &restored);
    exception_code = 0;
    const int32_t unflatten_error = hooks().invoke_entry(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
        nullptr, unflatten_extra.data(), &exception_code) != 0 || exception_code != 0 ||
        !hooks().handle_is_live(restored) || restored == source;
    if (unflatten_error) {
      if (hooks().handle_is_live(restored)) {
        std::array<std::byte, 48> dispose_extra{};
        write<int32_t>(dispose_extra, 0, 1);
        write<int16_t>(dispose_extra, 4, id);
        write<void*>(dispose_extra, 8, refcon);
        write<void*>(dispose_extra, 16, restored);
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
      write<int32_t>(dispose_extra, 0, 1);
      write<int16_t>(dispose_extra, 4, id);
      write<void*>(dispose_extra, 8, refcon);
      write<void*>(dispose_extra, 16, restored);
      entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr, dispose_extra.data());
      ++runtime().arbitrary.dispose_calls;
      ++runtime().arbitrary.roundtrip_failures;
      continue;
    }
    std::array<std::byte, 48> dispose_extra{};
    write<int32_t>(dispose_extra, 0, 1);
    write<int16_t>(dispose_extra, 4, id);
    write<void*>(dispose_extra, 8, refcon);
    write<void*>(dispose_extra, 16, source);
    if (entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data()) != 0) {
      ++runtime().arbitrary.invalid_operations;
      return false;
    }
    ++runtime().arbitrary.dispose_calls;
    write<void*>(definition, u + 16, restored);
    ++runtime().arbitrary.roundtrip_calls;
  }
  return true;
}



bool validate_requested_assignments(const parameters::RequestedAssignments& requested) {
  for (const auto& assignment : requested) {
    if (assignment.index < 1 || static_cast<std::size_t>(assignment.index) > runtime().records.size()) return false;
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
        (assignment.kind == parameters::RequestedKind::ArbitraryText && !arbitrary_compatible) ||
        (descriptor.type == 12 && (assignment.value < 0 ||
         assignment.value > static_cast<double>(hooks().active_mask_count()) ||
         std::trunc(assignment.value) != assignment.value)) ||
        ((assignment.kind == parameters::RequestedKind::Integer || assignment.kind == parameters::RequestedKind::Float) && descriptor.type != 12 && (!descriptor.has_numeric ||
         assignment.value < descriptor.valid_min || assignment.value > descriptor.valid_max))) return false;
  }
  return true;
}

void initialize_parameter_definitions(
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions) {
  for (std::size_t i = 0; i < runtime().records.size(); ++i) {
    definitions[i + 1] = runtime().records[i].raw;
    if (runtime().records[i].type == 0)
      std::memset(definitions[i + 1].data() + 56, 0, 40);
    else if (runtime().records[i].type == 1 || runtime().records[i].type == 7)
      write<int32_t>(definitions[i + 1], 56, static_cast<int32_t>(runtime().records[i].default_value));
    else if (runtime().records[i].type == 4)
      write<int32_t>(definitions[i + 1], 56, runtime().records[i].default_value != 0 ? 1 : 0);
    else if (runtime().records[i].type == 2)
      write<int32_t>(definitions[i + 1], 56,
          static_cast<int32_t>(std::round(runtime().records[i].default_value * 65536.0)));
    else if (runtime().records[i].type == 10)
      write<double>(definitions[i + 1], 56, runtime().records[i].default_value);
    else if (runtime().records[i].type == 3)
      write<int32_t>(definitions[i + 1], 56, static_cast<int32_t>(std::round(runtime().records[i].default_components[0] * 65536.0)));
    else if (runtime().records[i].type == 6) {
      write<int32_t>(definitions[i + 1], 56, static_cast<int32_t>(std::round(runtime().records[i].default_components[0] * 65536.0)));
      write<int32_t>(definitions[i + 1], 60, static_cast<int32_t>(std::round(runtime().records[i].default_components[1] * 65536.0)));
    } else if (runtime().records[i].type == 18)
      for (int component = 0; component < 3; ++component)
        write<double>(definitions[i + 1], 56 + component * 8, runtime().records[i].default_components[component]);
    else if (runtime().records[i].type == 12) {
      const int32_t index = static_cast<int32_t>(runtime().records[i].default_value);
      int32_t mask_id = 0;
      if (index > 0) hooks().active_mask_id(static_cast<std::size_t>(index - 1), &mask_id);
      write<int32_t>(definitions[i + 1], 56, mask_id);
    }
  }
}

bool apply_requested_assignments(
    std::vector<std::array<std::byte, parameters::kDefinitionSize>>& definitions,
    const parameters::RequestedAssignments& requested) {
  if (!validate_requested_assignments(requested)) return false;
  for (const auto& assignment : requested) {
    const auto slot = static_cast<std::size_t>(assignment.index);
    const auto type = runtime().records[slot - 1].type;
    if (type == 11 && assignment.kind == parameters::RequestedKind::ArbitraryText) continue;
    if (type == 1 || type == 4 || type == 7)
      write<int32_t>(definitions[slot], 56, static_cast<int32_t>(assignment.value));
    else if (type == 12) {
      const int32_t index = static_cast<int32_t>(assignment.value);
      int32_t mask_id = 0;
      if (index > 0 && !hooks().active_mask_id(static_cast<std::size_t>(index - 1), &mask_id)) return false;
      write<int32_t>(definitions[slot], 56, mask_id);
    }
    else if (type == 2) {
      const double fixed = assignment.value * 65536.0;
      if (!std::isfinite(fixed) || fixed < INT32_MIN || fixed > INT32_MAX) return false;
      write<int32_t>(definitions[slot], 56, static_cast<int32_t>(std::round(fixed)));
    } else if (type == 10)
      write<double>(definitions[slot], 56, assignment.value);
    else if (type == 5) {
      std::memcpy(definitions[slot].data() + 56, assignment.color.data(), assignment.color.size());
      runtime().records[slot - 1].current_color = assignment.color;
      for (std::size_t channel = 0; channel < 4; ++channel)
        runtime().records[slot - 1].current_float_color[channel] = assignment.color[channel] / 255.0f;
    }
    else if (type == 3)
      write<int32_t>(definitions[slot], 56, static_cast<int32_t>(std::round(assignment.components[0] * 65536.0)));
    else if (type == 6) {
      write<int32_t>(definitions[slot], 56, static_cast<int32_t>(std::round(assignment.components[0] * 65536.0)));
      write<int32_t>(definitions[slot], 60, static_cast<int32_t>(std::round(assignment.components[1] * 65536.0)));
    } else if (type == 18)
      for (int component = 0; component < 3; ++component)
        write<double>(definitions[slot], 56 + component * 8, assignment.components[component]);
    else
      return false;
  }
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
    if (assignment.kind == parameters::RequestedKind::Integer)
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
constexpr std::size_t kMaxParams = 1024;
constexpr std::size_t kParamSize =
    aexcompat::worker_runtime::parameters::kDefinitionSize;
constexpr std::size_t kParamType = 12;
constexpr std::size_t kParamName = 16;
constexpr std::size_t kParamNameSize = 32;
constexpr std::size_t kParamFlags = 48;
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
  const int32_t host_index = index < 0 ? static_cast<int32_t>(g_params.size() + 1) : index;
  if (host_index <= 0 || host_index > static_cast<int32_t>(kMaxParams) ||
      std::any_of(g_params.begin(), g_params.end(),
                  [host_index](const auto& param) { return param.index == host_index; })) return 4;
  ParamRecord record{host_index, read<int32_t>(bytes, 0), read<int32_t>(bytes, kParamType),
                     read<uint32_t>(bytes, kParamFlags), std::string(name, length)};
  constexpr std::size_t u = 56;
  if (record.type == 1) {
    record.has_numeric = true;
    record.valid_min = read<int32_t>(bytes, u + 68);
    record.valid_max = read<int32_t>(bytes, u + 72);
    record.slider_min = read<int32_t>(bytes, u + 76);
    record.slider_max = read<int32_t>(bytes, u + 80);
    record.default_value = read<int32_t>(bytes, u + 84);
  } else if (record.type == 2) {
    record.has_numeric = true;
    record.has_current = true;
    record.current_value = read<int32_t>(bytes, u) / 65536.0;
    record.valid_min = read<int32_t>(bytes, u + 68) / 65536.0;
    record.valid_max = read<int32_t>(bytes, u + 72) / 65536.0;
    record.slider_min = read<int32_t>(bytes, u + 76) / 65536.0;
    record.slider_max = read<int32_t>(bytes, u + 80) / 65536.0;
    record.default_value = read<int32_t>(bytes, u + 84) / 65536.0;
    record.precision = read<int16_t>(bytes, u + 88);
  } else if (record.type == 7) {
    record.has_numeric = true;
    record.valid_min = 1;
    record.valid_max = read<int16_t>(bytes, u + 4);
    record.slider_min = record.valid_min;
    record.slider_max = record.valid_max;
    record.default_value = read<int16_t>(bytes, u + 6);
    const char* choices = read<const char*>(bytes, u + 8);
    if (choices) record.choices.assign(choices, strnlen_s(choices, 4096));
  } else if (record.type == 4) {
    record.has_numeric = true;
    record.has_current = true;
    record.valid_min = 0;
    record.valid_max = 1;
    record.slider_min = 0;
    record.slider_max = 1;
    record.default_value = read<uint8_t>(bytes, u + 4) ? 1 : 0;
    record.current_value = read<int32_t>(bytes, u) != 0 ? 1 : 0;
    const char* label = read<const char*>(bytes, u + 8);
    if (label) record.label.assign(label, strnlen_s(label, 4096));
  } else if (record.type == 0) {
    record.layer_default = read<int32_t>(bytes, u + 116);
  } else if (record.type == 12) {
    record.has_numeric = true;
    record.valid_min = 0;
    record.valid_max = 1024;
    record.slider_min = 0;
    record.slider_max = 1024;
    record.default_value = read<int32_t>(bytes, u + 8);
  } else if (record.type == 10) {
    record.has_numeric = true;
    record.valid_min = read<float>(bytes, u + 48);
    record.valid_max = read<float>(bytes, u + 52);
    record.slider_min = read<float>(bytes, u + 56);
    record.slider_max = read<float>(bytes, u + 60);
    record.default_value = read<float>(bytes, u + 64);
    record.precision = read<int16_t>(bytes, u + 68);
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
    record.default_components[0] = read<int32_t>(bytes, u + 4) / 65536.0;
  } else if (record.type == 6) {
    record.component_count = 2;
    record.current_components[0] = read<int32_t>(bytes, u) / 65536.0;
    record.current_components[1] = read<int32_t>(bytes, u + 4) / 65536.0;
    record.default_components[0] = read<int32_t>(bytes, u + 12) / 65536.0;
    record.default_components[1] = read<int32_t>(bytes, u + 16) / 65536.0;
  } else if (record.type == 18) {
    record.component_count = 3;
    for (int component = 0; component < 3; ++component) {
      record.current_components[component] = read<double>(bytes, u + component * 8);
      record.default_components[component] = read<double>(bytes, u + 24 + component * 8);
    }
  } else if (record.type == 15) {
    const char* label = read<const char*>(bytes, u + 8);
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
