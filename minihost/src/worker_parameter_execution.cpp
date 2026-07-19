#include "worker_parameter_execution.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>
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

bool configure_hooks(const Hooks& value) noexcept { if (!value.invoke_entry || !value.handle_is_live) return false; g_hooks=value; return true; }

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



}  // namespace aexcompat::worker_runtime::parameter_execution
