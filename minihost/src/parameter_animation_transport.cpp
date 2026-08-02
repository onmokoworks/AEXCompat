#include "parameter_animation_transport.hpp"

#include "strict_json.hpp"

#include <algorithm>
#include <cmath>
#include <fstream>
#include <iterator>
#include <set>
#include <string>
#include <utility>
#include <variant>

namespace aexcompat::parameter_animation {
namespace {

using strict_json::JsonValue;
using strict_json::StrictJsonParser;
using strict_json::json_exact_keys;
using strict_json::json_i32;
using strict_json::json_member;
using strict_json::json_number;
using strict_json::json_string;

constexpr std::size_t kMaxParams = 1024;

}  // namespace

bool rational_less(int32_t left_value, uint32_t left_scale,
                   int32_t right_value, uint32_t right_scale) {
  return static_cast<int64_t>(left_value) * right_scale <
         static_cast<int64_t>(right_value) * left_scale;
}

bool load_parameter_animation(const std::filesystem::path& path,
                              std::vector<ParameterTimeline>& result) {
  std::error_code error;
  const auto absolute = std::filesystem::absolute(path, error);
  const auto canonical = std::filesystem::canonical(path, error);
  // The broker uses <repository>/target as worker CWD so repository sources
  // are not ambient relative inputs. The transport boundary remains
  // <repository>/target/image-transport.
  const auto owned = std::filesystem::canonical(
      std::filesystem::current_path() / "image-transport", error);
  if (error || !path.is_absolute() || absolute.lexically_normal() != canonical ||
      canonical.parent_path() != owned)
    return false;
  const auto size = std::filesystem::file_size(canonical, error);
  if (error || size == 0 || size > 1024 * 1024) return false;
  std::ifstream input(canonical, std::ios::binary);
  std::string text((std::istreambuf_iterator<char>(input)), {});
  JsonValue root;
  if (input.bad() || !StrictJsonParser(std::move(text)).parse(root) ||
      !std::holds_alternative<JsonValue::Object>(root.value))
    return false;
  const auto& object = std::get<JsonValue::Object>(root.value);
  int32_t version{};
  if (!json_exact_keys(object, {"schema_version", "parameters"}) ||
      !json_i32(object, "schema_version", version) || version != 1)
    return false;
  const auto* parameters_value = json_member(object, "parameters");
  if (!parameters_value ||
      !std::holds_alternative<JsonValue::Array>(parameters_value->value))
    return false;
  std::set<int32_t> slots;
  std::size_t total = 0;
  std::size_t arbitrary_total = 0;
  std::vector<ParameterTimeline> parsed;
  for (const auto& item : std::get<JsonValue::Array>(parameters_value->value)) {
    if (!std::holds_alternative<JsonValue::Object>(item.value)) return false;
    const auto& parameter = std::get<JsonValue::Object>(item.value);
    ParameterTimeline timeline;
    int32_t slot{};
    if (!json_exact_keys(parameter, {"slot", "keys"}) ||
        !json_i32(parameter, "slot", slot) || slot <= 0 ||
        slot > static_cast<int32_t>(kMaxParams) || !slots.insert(slot).second)
      return false;
    timeline.slot = slot;
    const auto* keys_value = json_member(parameter, "keys");
    if (!keys_value || !std::holds_alternative<JsonValue::Array>(keys_value->value))
      return false;
    const auto& keys = std::get<JsonValue::Array>(keys_value->value);
    if (keys.empty() || keys.size() > 256 || total > 4096 - keys.size())
      return false;
    total += keys.size();
    for (const auto& item_key : keys) {
      if (!std::holds_alternative<JsonValue::Object>(item_key.value)) return false;
      const auto& key_object = std::get<JsonValue::Object>(item_key.value);
      if (!json_exact_keys(key_object, {"time", "interpolation", "value"}))
        return false;
      AnimationKey key;
      std::string interpolation;
      const auto* time_value = json_member(key_object, "time");
      const auto* animation_value = json_member(key_object, "value");
      if (!time_value || !animation_value ||
          !std::holds_alternative<JsonValue::Object>(time_value->value) ||
          !std::holds_alternative<JsonValue::Object>(animation_value->value) ||
          !json_string(key_object, "interpolation", interpolation) ||
          (interpolation != "hold" && interpolation != "linear"))
        return false;
      key.hold = interpolation == "hold";
      const auto& time = std::get<JsonValue::Object>(time_value->value);
      int32_t scale{};
      if (!json_exact_keys(time, {"value", "scale"}) ||
          !json_i32(time, "value", key.time) ||
          !json_i32(time, "scale", scale) || scale <= 0)
        return false;
      key.scale = static_cast<uint32_t>(scale);
      if (!timeline.keys.empty() &&
          !rational_less(timeline.keys.back().time, timeline.keys.back().scale,
                         key.time, key.scale))
        return false;
      const auto& value_object =
          std::get<JsonValue::Object>(animation_value->value);
      std::string type;
      if (!json_string(value_object, "type", type) ||
          !json_exact_keys(value_object, {"type", "value"}))
        return false;
      const auto* value = json_member(value_object, "value");
      if (type == "scalar") {
        key.kind = AnimationValueKind::Scalar;
        if (!json_number(value_object, "value", key.scalar)) return false;
      } else if (type == "color") {
        key.kind = AnimationValueKind::Color;
        if (!value || !std::holds_alternative<JsonValue::Array>(value->value) ||
            std::get<JsonValue::Array>(value->value).size() != 4)
          return false;
        for (std::size_t index = 0; index < 4; ++index) {
          const auto& component = std::get<JsonValue::Array>(value->value)[index];
          if (!std::holds_alternative<int64_t>(component.value) ||
              std::get<int64_t>(component.value) < 0 ||
              std::get<int64_t>(component.value) > 255)
            return false;
          key.color[index] =
              static_cast<unsigned char>(std::get<int64_t>(component.value));
        }
      } else if (type == "components") {
        key.kind = AnimationValueKind::Components;
        if (!value || !std::holds_alternative<JsonValue::Array>(value->value))
          return false;
        const auto& components = std::get<JsonValue::Array>(value->value);
        if (components.empty() || components.size() > 3) return false;
        key.component_count = static_cast<int32_t>(components.size());
        for (std::size_t index = 0; index < components.size(); ++index) {
          if (std::holds_alternative<double>(components[index].value))
            key.components[index] = std::get<double>(components[index].value);
          else if (std::holds_alternative<int64_t>(components[index].value))
            key.components[index] =
                static_cast<double>(std::get<int64_t>(components[index].value));
          else
            return false;
          if (!std::isfinite(key.components[index])) return false;
        }
      } else if (type == "arbitrary") {
        key.kind = AnimationValueKind::Arbitrary;
        if (!value || !std::holds_alternative<JsonValue::Array>(value->value))
          return false;
        const auto& bytes = std::get<JsonValue::Array>(value->value);
        if (bytes.empty() || bytes.size() > 64 * 1024 ||
            arbitrary_total > 1024 * 1024 - bytes.size())
          return false;
        arbitrary_total += bytes.size();
        key.arbitrary.reserve(bytes.size());
        for (const auto& byte : bytes) {
          if (!std::holds_alternative<int64_t>(byte.value) ||
              std::get<int64_t>(byte.value) < 0 ||
              std::get<int64_t>(byte.value) > 255)
            return false;
          key.arbitrary.push_back(
              static_cast<unsigned char>(std::get<int64_t>(byte.value)));
        }
      } else {
        return false;
      }
      timeline.keys.push_back(std::move(key));
    }
    const bool has_arbitrary =
        std::any_of(timeline.keys.begin(), timeline.keys.end(), [](const auto& key) {
          return key.kind == AnimationValueKind::Arbitrary;
        });
    if (has_arbitrary &&
        std::any_of(timeline.keys.begin(), timeline.keys.end(), [](const auto& key) {
          return key.kind != AnimationValueKind::Arbitrary;
        }))
      return false;
    parsed.push_back(std::move(timeline));
  }
  result = std::move(parsed);
  return true;
}

}  // namespace aexcompat::parameter_animation
