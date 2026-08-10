#include "worker_parameter_runtime.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>

namespace aexcompat::worker_runtime::parameters {
namespace {
State g_state;
}

State& state() noexcept { return g_state; }

const parameter_animation::ParameterTimeline* timeline(int32_t slot) noexcept {
  const auto& timelines = g_state.timelines;
  const auto found = std::find_if(timelines.begin(), timelines.end(),
      [slot](const auto& value) { return value.slot == slot; });
  return found == timelines.end() ? nullptr : &*found;
}

bool copy_definition_at_time(int32_t slot, int32_t time, uint32_t scale,
                             const Definition& hosted, Definition& result) {
  if (scale == 0 || slot < 0 ||
      static_cast<std::size_t>(slot) > g_state.records.size())
    return false;
  result = hosted;
  // Slot 0 is the input layer. It is hosted as a per-frame world definition,
  // not as a parameter record/timeline, and therefore remains a direct copy.
  if (slot == 0)
    return true;
  const auto* source = timeline(slot);
  if (!source)
    return true;
  if (source->keys.empty() ||
      source->keys.front().kind ==
          parameter_animation::AnimationValueKind::Arbitrary)
    return false;

  const auto rational_less = [](int32_t left, uint32_t left_scale,
                                int32_t right, uint32_t right_scale) {
    return static_cast<int64_t>(left) * right_scale <
           static_cast<int64_t>(right) * left_scale;
  };
  parameter_animation::ParameterAnimationKey value = source->keys.back();
  if (!rational_less(source->keys.front().time, source->keys.front().scale,
                     time, scale)) {
    value = source->keys.front();
  } else {
    for (std::size_t i = 1; i < source->keys.size(); ++i) {
      const auto& right = source->keys[i];
      if (!rational_less(time, scale, right.time, right.scale))
        continue;
      const auto& left = source->keys[i - 1];
      value = left;
      if (!left.hold && left.kind == right.kind &&
          left.component_count == right.component_count) {
        const long double now = static_cast<long double>(time) / scale;
        const long double a = static_cast<long double>(left.time) / left.scale;
        const long double b = static_cast<long double>(right.time) / right.scale;
        const double fraction = static_cast<double>((now - a) / (b - a));
        if (value.kind == parameter_animation::AnimationValueKind::Scalar) {
          value.scalar += (right.scalar - value.scalar) * fraction;
        } else if (value.kind == parameter_animation::AnimationValueKind::Color) {
          for (std::size_t channel = 0; channel < value.color.size(); ++channel)
            value.color[channel] = static_cast<unsigned char>(std::clamp(
                std::lround(value.color[channel] +
                            (right.color[channel] - value.color[channel]) *
                                fraction),
                0l, 255l));
        } else {
          for (int component = 0; component < value.component_count; ++component)
            value.components[component] +=
                (right.components[component] - value.components[component]) *
                fraction;
        }
      }
      break;
    }
  }

  const auto& param = g_state.records[static_cast<std::size_t>(slot - 1)];
  const auto write = [&](std::size_t offset, const auto& encoded) {
    std::memcpy(result.data() + offset, &encoded, sizeof(encoded));
  };
  if (value.kind == parameter_animation::AnimationValueKind::Scalar) {
    if (param.type == 1 || param.type == 4 || param.type == 7) {
      if (!std::isfinite(value.scalar) || std::floor(value.scalar) != value.scalar ||
          value.scalar < std::numeric_limits<int32_t>::min() ||
          value.scalar > std::numeric_limits<int32_t>::max())
        return false;
      write(56, static_cast<int32_t>(value.scalar));
    } else if (param.type == 2) {
      const double encoded = value.scalar * 65536.0;
      if (encoded < std::numeric_limits<int32_t>::min() ||
          encoded > std::numeric_limits<int32_t>::max())
        return false;
      write(56, static_cast<int32_t>(std::round(encoded)));
    } else if (param.type == 10) {
      write(56, value.scalar);
    } else {
      return false;
    }
  } else if (value.kind == parameter_animation::AnimationValueKind::Color) {
    if (param.type != 5)
      return false;
    std::memcpy(result.data() + 56, value.color.data(), value.color.size());
  } else {
    const int count = param.type == 3 ? 1 : (param.type == 6 ? 2 : (param.type == 18 ? 3 : 0));
    if (count == 0 || value.component_count != count)
      return false;
    for (int component = 0; component < count; ++component) {
      if (param.type == 18) {
        write(56 + component * 8, value.components[component]);
      } else {
        const double encoded = value.components[component] * 65536.0;
        if (encoded < std::numeric_limits<int32_t>::min() ||
            encoded > std::numeric_limits<int32_t>::max())
          return false;
        write(56 + component * 4,
              static_cast<int32_t>(std::round(encoded)));
      }
    }
  }
  return true;
}

}  // namespace aexcompat::worker_runtime::parameters
