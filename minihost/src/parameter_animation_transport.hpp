#pragma once

#include <array>
#include <cstdint>
#include <filesystem>
#include <vector>

namespace aexcompat::parameter_animation {

enum class AnimationValueKind { Scalar, Color, Components, Arbitrary };

struct ParameterAnimationKey {
  int32_t time{};
  uint32_t scale{};
  bool hold{};
  AnimationValueKind kind{};
  double scalar{};
  std::array<unsigned char, 4> color{};
  std::array<double, 3> components{};
  int32_t component_count{};
  std::vector<unsigned char> arbitrary;
};

using AnimationKey = ParameterAnimationKey;

struct ParameterTimeline {
  int32_t slot{};
  std::vector<ParameterAnimationKey> keys;
};

bool rational_less(int32_t left_value, uint32_t left_scale,
                   int32_t right_value, uint32_t right_scale);
bool load_parameter_animation(const std::filesystem::path& path,
                              std::vector<ParameterTimeline>& result);

}  // namespace aexcompat::parameter_animation
