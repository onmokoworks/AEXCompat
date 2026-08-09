#include "worker_pf_param_suites.hpp"

#include "worker_mask_runtime_internal.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_pf_suites_internal.hpp"
#include "parameter_animation_transport.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <mutex>

namespace aexcompat::l2_detail {

using aexcompat::parameter_animation::ParameterAnimationKey;
using aexcompat::parameter_animation::ParameterTimeline;
using aexcompat::parameter_animation::rational_less;
using aexcompat::worker_runtime::parameters::ParamRecord;

// Worker-entry owned effect identity and the shared animation helpers stay in
// l2_main with the lifecycle apply path; the callbacks here read them
// cross-TU.
extern OpaqueHostObject g_effect;
const ParameterTimeline* parameter_timeline(int32_t slot);
bool valid_param_utils_index(int32_t index, bool allow_groups = false);
ParameterAnimationKey evaluate_animation(const ParameterTimeline& timeline,
                                         int32_t time, uint32_t scale);
bool write_animation_value(
    std::array<std::byte, worker_runtime::parameters::kDefinitionSize>& definition,
    const ParamRecord& param, const ParameterAnimationKey& key);

// Private protocol constants mirrored from worker_main's frozen ParamDef
// layout; parameter/UI/keyframe state reads through its Phase D owner.
namespace {
constexpr std::size_t kParamSize = aexcompat::worker_runtime::parameters::kDefinitionSize;
constexpr std::size_t kParamType = 12;
constexpr std::size_t kParamUiFlags = 4;
constexpr std::size_t kParamName = 16;
constexpr std::size_t kParamNameSize = 32;
constexpr std::size_t kParamFlags = 48;
constexpr int32_t kPfBadCallbackParam = 516;
auto& g_parameter_runtime = aexcompat::worker_runtime::parameters::state();
auto& g_params = g_parameter_runtime.records;
auto& g_update_params_ui_active = g_parameter_runtime.ui.update_active;
auto& g_user_changed_param_active = g_parameter_runtime.ui.user_changed_active;
auto& g_active_ui_params = g_parameter_runtime.ui.active_params;
auto& g_active_ui_param_count = g_parameter_runtime.ui.active_param_count;
auto& g_update_param_ui_calls = g_parameter_runtime.ui.update_calls;
auto& g_parameter_timelines = g_parameter_runtime.timelines;
auto& g_keyframe_checkout_ledger = g_parameter_runtime.keyframe_checkout_ledger;
auto& g_keyframe_checkout_mutex = g_parameter_runtime.keyframe_checkout_mutex;
}  // namespace

int32_t __cdecl floating_point_from_point(void*, const void* definition, void* output) {
  if (!definition || !output) return 4;
  const auto* bytes = static_cast<const std::byte*>(definition);
  auto* values = static_cast<double*>(output);
  int32_t x{}, y{};
  std::memcpy(&x, bytes + 56, sizeof(x));
  std::memcpy(&y, bytes + 60, sizeof(y));
  values[0] = x / 65536.0;
  values[1] = y / 65536.0;
  return 0;
}

int32_t __cdecl floating_point_from_angle(void*, const void* definition, double* output) {
  if (!definition || !output) return 4;
  int32_t value{};
  std::memcpy(&value, static_cast<const std::byte*>(definition) + 56, sizeof(value));
  *output = value / 65536.0;
  return 0;
}

int32_t __cdecl floating_point_from_color(void* effect_ref, const void* definition,
                                          PfColorParamPixelFloat* output) {
  if (effect_ref != &g_effect || !definition || !output) return kPfBadCallbackParam;
  const auto* bytes = static_cast<const std::byte*>(definition);
  int32_t disk_id{}, type{};
  std::memcpy(&disk_id, bytes, sizeof(disk_id));
  std::memcpy(&type, bytes + kParamType, sizeof(type));
  if (type != 5) return kPfUnrecognizedParamType;
  std::array<unsigned char, 4> value{};
  std::memcpy(value.data(), bytes + 56, value.size());
  // Resolve which colour parameter this is. The disk id (PF_ParamDef.uu.id,
  // offset 0) is unreliable: it is host-assigned in AE, but many effects leave
  // it 0 for every parameter (Beam sets it 0 on its points and colours alike),
  // so a lookup by id collides with the first zero-id parameter - a POINT for
  // Beam - and answers PF_Err_UNRECOGNIZED_PARAM_TYPE for a valid colour
  // (issue #1060). Match on the colour value among the type-5 parameters
  // instead: the checked-out value is either a parameter's current or default
  // colour, which identifies it without depending on the id.
  const auto by_id = std::find_if(g_params.begin(), g_params.end(),
      [disk_id](const ParamRecord& p) { return p.disk_id == disk_id; });
  const std::array<float, 4>* resolved = nullptr;
  const auto match = [&](const ParamRecord& p) -> const std::array<float, 4>* {
    if (p.type != 5 || !p.has_color) return nullptr;
    if (value == p.current_color) return &p.current_float_color;
    if (value == p.default_color) return &p.default_float_color;
    return nullptr;
  };
  if (by_id != g_params.end()) resolved = match(*by_id);
  if (!resolved)
    for (const auto& p : g_params) if ((resolved = match(p))) break;
  if (!resolved) return kPfBadCallbackParam;

  const PfColorParamPixelFloat result{(*resolved)[0], (*resolved)[1],
                                      (*resolved)[2], (*resolved)[3]};
  std::memcpy(output, &result, sizeof(result));
  return 0;
}

int32_t __cdecl update_param_ui(void* effect_ref, int32_t index, const void* definition) {
  if (effect_ref != &g_effect || (!g_update_params_ui_active && !g_user_changed_param_active) || !definition || index <= 0 ||
      static_cast<std::size_t>(index) >= g_active_ui_param_count || !g_active_ui_params ||
      !g_active_ui_params[index]) return kPfBadCallbackParam;
  const auto* source = static_cast<const std::byte*>(definition);
  auto* target = static_cast<std::byte*>(g_active_ui_params[index]);
  int32_t source_type{}, target_type{};
  std::memcpy(&source_type, source + kParamType, sizeof(source_type));
  std::memcpy(&target_type, target + kParamType, sizeof(target_type));
  if (source_type != target_type) return kPfUnrecognizedParamType;

  constexpr uint32_t kMutableUiFlags = (1u << 4) | (1u << 5) | (1u << 9);
  constexpr uint32_t kCollapseTwirly = 1u << 5;
  uint32_t source_ui{}, target_ui{}, source_flags{}, target_flags{};
  std::memcpy(&source_ui, source + kParamUiFlags, sizeof(source_ui));
  std::memcpy(&target_ui, target + kParamUiFlags, sizeof(target_ui));
  std::memcpy(&source_flags, source + kParamFlags, sizeof(source_flags));
  std::memcpy(&target_flags, target + kParamFlags, sizeof(target_flags));
  target_ui = (target_ui & ~kMutableUiFlags) | (source_ui & kMutableUiFlags);
  target_flags = (target_flags & ~kCollapseTwirly) | (source_flags & kCollapseTwirly);
  std::memcpy(target + kParamUiFlags, &target_ui, sizeof(target_ui));
  std::memcpy(target + 8, source + 8, 2 * sizeof(int16_t));
  std::memcpy(target + kParamName, source + kParamName, kParamNameSize);
  target[kParamName + kParamNameSize - 1] = std::byte{0};
  std::memcpy(target + kParamFlags, &target_flags, sizeof(target_flags));

  constexpr std::size_t u = 56;
  if (target_type == 1 || target_type == 2) {
    std::memcpy(target + u + 76, source + u + 76, 2 * sizeof(int32_t));
    if (target_type == 2) std::memcpy(target + u + 88, source + u + 88, 8);
  } else if (target_type == 10) {
    std::memcpy(target + u + 56, source + u + 56, 2 * sizeof(float));
    std::memcpy(target + u + 68, source + u + 68, 8);
  }
  ++g_update_param_ui_calls;
  return 0;
}

int32_t __cdecl is_identical_param_checkout(void *effect_ref, int32_t index,
                                            int32_t time1, int32_t step1,
                                            uint32_t scale1, int32_t time2,
                                            int32_t step2, uint32_t scale2,
                                            uint8_t *identical) {
  if (effect_ref != &g_effect || !identical ||
      !valid_param_utils_index(index) || scale1 == 0 || scale2 == 0 ||
      step1 < 0 || step2 < 0)
    return kPfBadCallbackParam;
  const auto *timeline = parameter_timeline(index);
  if (!timeline) {
    *identical = 1;
    return 0;
  }
  std::array<std::byte, kParamSize> first = g_params[index - 1].raw,
                                    second = first;
  if (!write_animation_value(first, g_params[index - 1],
                             evaluate_animation(*timeline, time1, scale1)) ||
      !write_animation_value(second, g_params[index - 1],
                             evaluate_animation(*timeline, time2, scale2)))
    return kPfBadCallbackParam;
  *identical =
      std::memcmp(first.data() + 56, second.data() + 56, kParamSize - 56) == 0
          ? 1
          : 0;
  return 0;
}

int32_t __cdecl find_param_keyframe_time(void *effect_ref, int32_t index,
                                         int32_t time, uint32_t scale,
                                         int32_t direction, uint8_t *found,
                                         int32_t *key_index, int32_t *key_time,
                                         uint32_t *key_scale) {
  if (effect_ref != &g_effect || !found || !valid_param_utils_index(index) ||
      scale == 0 ||
      (direction != 0 && direction != 1 && direction != 0x1000 &&
       direction != 0x1001) ||
      ((!key_time) != (!key_scale)))
    return kPfBadCallbackParam;
  const auto *timeline = parameter_timeline(index);
  *found = 0;
  if (timeline) {
    const bool greater = direction == 0 || direction == 0x1000;
    const bool inclusive = direction == 0x1000 || direction == 0x1001;
    const auto matches = [&](const ParameterAnimationKey &key) {
      const bool key_less = rational_less(key.time, key.scale, time, scale);
      const bool time_less = rational_less(time, scale, key.time, key.scale);
      const bool equal = !key_less && !time_less;
      return greater ? (time_less || (inclusive && equal))
                     : (key_less || (inclusive && equal));
    };
    for (std::size_t offset = 0; offset < timeline->keys.size(); ++offset) {
      const std::size_t i = greater ? offset : timeline->keys.size() - 1 - offset;
      const auto &key = timeline->keys[i];
      if (matches(key)) {
        *found = 1;
        if (key_index)
          *key_index = static_cast<int32_t>(i);
        if (key_time) {
          *key_time = key.time;
          *key_scale = key.scale;
        }
        return 0;
      }
    }
  }
  if (key_index)
    *key_index = -1;
  if (key_time) {
    *key_time = 0;
    *key_scale = scale;
  }
  return 0;
}

int32_t __cdecl get_param_keyframe_count(void *effect_ref, int32_t index,
                                         int32_t *count) {
  if (effect_ref != &g_effect || !count || !valid_param_utils_index(index))
    return kPfBadCallbackParam;
  *count = -1;
  const auto *timeline = parameter_timeline(index);
  if (timeline)
    *count = static_cast<int32_t>(timeline->keys.size());
  return 0;
}

int32_t __cdecl checkout_param_keyframe(void *effect_ref, int32_t index,
                                        int32_t key_index, int32_t *key_time,
                                        uint32_t *key_scale, void *definition) {
  if (effect_ref != &g_effect || !valid_param_utils_index(index) ||
      key_index < 0 || ((!key_time) != (!key_scale)) ||
      (!definition && !key_time))
    return kPfBadCallbackParam;
  const auto *timeline = parameter_timeline(index);
  if (!timeline || static_cast<std::size_t>(key_index) >= timeline->keys.size())
    return kPfInvalidIndex;
  const auto &key = timeline->keys[key_index];
  if (key_time) {
    *key_time = key.time;
    *key_scale = key.scale;
  }
  if (definition) {
    auto bytes = g_params[index - 1].raw;
    if (!write_animation_value(bytes, g_params[index - 1], key))
      return kPfBadCallbackParam;
    std::memcpy(definition, bytes.data(), bytes.size());
    std::lock_guard<std::mutex> lock(g_keyframe_checkout_mutex);
    if (!g_keyframe_checkout_ledger.emplace(definition, bytes).second)
      return kPfBadCallbackParam;
  }
  return 0;
}

int32_t __cdecl checkin_param_keyframe(void *effect_ref, void *definition) {
  if (effect_ref != &g_effect || !definition)
    return kPfBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_keyframe_checkout_mutex);
  const auto found = g_keyframe_checkout_ledger.find(definition);
  if (found == g_keyframe_checkout_ledger.end())
    return kPfInvalidIndex;
  g_keyframe_checkout_ledger.erase(found);
  return 0;
}

int32_t __cdecl param_key_index_to_time(void* effect_ref, int32_t index, int32_t key_index,
                                         int32_t* key_time, uint32_t* key_scale) {
  if (effect_ref != &g_effect || !valid_param_utils_index(index) || key_index < 0 ||
      !key_time || !key_scale)
    return kPfBadCallbackParam;
  const auto* timeline = parameter_timeline(index);
  if (!timeline || static_cast<std::size_t>(key_index) >= timeline->keys.size())
    return kPfInvalidIndex;
  *key_time = timeline->keys[key_index].time;
  *key_scale = timeline->keys[key_index].scale;
  return 0;
}



PointParamSuite g_point_param_suite{&floating_point_from_point};
AngleParamSuite g_angle_param_suite{&floating_point_from_angle};
PfColorParamSuite1 g_color_param_suite1{&floating_point_from_color};
ParamUtilsSuite1 g_param_utils_suite1{&update_param_ui, &get_current_param_state_obsolete,
    &has_param_changed_obsolete, &have_inputs_changed_over_time_span_obsolete,
    &is_identical_param_checkout, &find_param_keyframe_time, &get_param_keyframe_count,
    &checkout_param_keyframe, &checkin_param_keyframe, &param_key_index_to_time};
ParamUtilsSuite3 g_param_utils_suite{&update_param_ui, &get_current_param_state,
    &are_param_states_identical, &is_identical_param_checkout, &find_param_keyframe_time,
    &get_param_keyframe_count, &checkout_param_keyframe, &checkin_param_keyframe,
    &param_key_index_to_time};

}  // namespace aexcompat::l2_detail
