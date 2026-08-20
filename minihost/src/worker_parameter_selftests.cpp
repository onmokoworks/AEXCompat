#include "worker_parameter_selftests.hpp"
#include "worker_parameter_limits.hpp"
#include "worker_invocation_orchestration.hpp"
#include "worker_classic_render_entry.hpp"
#include "worker_smart_setup.hpp"
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>
namespace aexcompat::l2_detail {
int32_t __cdecl add_param(void*, int32_t, void*);
}
namespace aexcompat::parameter_selftests {
namespace { Hooks g_hooks; }
void configure(Hooks hooks) { g_hooks = hooks; }
auto& parameter_state() { return worker_runtime::parameters::state(); }
using worker_runtime::parameters::ParamRecord;
using parameter_animation::AnimationKey;
using parameter_animation::AnimationValueKind;
using parameter_animation::ParameterTimeline;
using pf_state_runtime::reset_effect_lifetime;
using pf_state_runtime::get_current_param_state;
using pf_state_runtime::are_param_states_identical;
using pf_state_runtime::corrupt_state_owner_for_test;
using pf_state_runtime::fill_registry_to_capacity_for_test;
using pf_state_runtime::live_state_count;
constexpr std::size_t kParamType=12, kParamUiFlags=4, kParamName=16, kParamFlags=48;
constexpr std::size_t kParamSize = worker_runtime::parameters::kDefinitionSize;
constexpr int32_t kPfBadCallbackParam=516, kPfInvalidIndex=513;
template<class T> T read(const Definition& b,std::size_t o){T v{};std::memcpy(&v,b.data()+o,sizeof(v));return v;}
template<class T> void write(Definition& b,std::size_t o,const T& v){std::memcpy(b.data()+o,&v,sizeof(v));}
#define acquire_suite g_hooks.acquire_suite
#define release_suite g_hooks.release_suite
#define update_param_ui g_hooks.update_param_ui
#define is_identical_param_checkout g_hooks.is_identical_checkout
#define find_param_keyframe_time g_hooks.find_keyframe_time
#define get_param_keyframe_count g_hooks.get_keyframe_count
#define checkout_param_keyframe g_hooks.checkout_keyframe
#define checkin_param_keyframe g_hooks.checkin_keyframe
#define param_key_index_to_time g_hooks.key_index_to_time
#define apply_parameter_animation g_hooks.apply_animation
bool verify_pf_param_utils_suite3() {
  const auto saved_params = parameter_state().records;
  parameter_state().records.clear();
  ParamRecord param{};
  param.index = 1;
  param.disk_id = 7001;
  param.type = 1;
  write<int32_t>(param.raw, 0, param.disk_id);
  write<int32_t>(param.raw, kParamType, param.type);
  write<int32_t>(param.raw, 56, 42);
  parameter_state().records.push_back(param);
  reset_effect_lifetime(true);

  auto active = param.raw;
  auto local = active;
  write<uint32_t>(local, kParamUiFlags, (1u << 5));
  write<int16_t>(local, 8, 123);
  write<int16_t>(local, 10, 45);
  write<uint32_t>(local, kParamFlags, (1u << 5));
  std::memcpy(local.data() + kParamName, "Updated", 8);
  void* active_params[2]{nullptr, active.data()};
  parameter_state().ui.active_params = active_params;
  parameter_state().ui.active_param_count = 2;
  parameter_state().ui.update_active = true;

  const void* acquired{};
  PfState first{}, second{}, changed{};
  PfTime start{12, 24}, duration{1, 24};
  uint8_t same = 0, identical = 0, found = 1;
  int32_t count = 0, key_index = 9, key_time = 9;
  uint32_t key_scale = 0;
  bool ok = acquire_suite("PF Param Utils Suite", 3, &acquired) == 0 &&
      acquired == g_hooks.param_utils_suite3 &&
      std::all_of(reinterpret_cast<void* const*>(g_hooks.param_utils_suite3),
                  reinterpret_cast<void* const*>(g_hooks.param_utils_suite3) + 9,
                  [](const void* slot) { return slot != nullptr; }) &&
      update_param_ui(g_hooks.effect, 1, local.data()) == 0 &&
      read<uint32_t>(active, kParamUiFlags) == (1u << 5) &&
      read<int16_t>(active, 8) == 123 && read<int16_t>(active, 10) == 45 &&
      read<uint32_t>(active, kParamFlags) == (1u << 5) &&
      std::strcmp(reinterpret_cast<const char*>(active.data() + kParamName), "Updated") == 0 &&
      get_current_param_state(g_hooks.effect, 1, &start, &duration, &first) == 0 &&
      get_current_param_state(g_hooks.effect, 1, &start, &duration, &second) == 0 &&
      are_param_states_identical(g_hooks.effect, &first, &second, &same) == 0 && same == 1;
  const void* acquired_v1{};
  PfState obsolete_state{};
  uint8_t obsolete_changed = 0;
  ok = ok && acquire_suite("PF Param Utils Suite", 2, &acquired_v1) == 0 &&
      acquired_v1 == g_hooks.param_utils_suite1 && acquired_v1 != acquired &&
      std::all_of(reinterpret_cast<void* const*>(g_hooks.param_utils_suite1),
                  reinterpret_cast<void* const*>(g_hooks.param_utils_suite1) + 10,
                  [](const void* slot) { return slot != nullptr; }) &&
      g_hooks.get_current_obsolete(g_hooks.effect, &obsolete_state) == 0 &&
      g_hooks.has_changed_obsolete(
          g_hooks.effect, &obsolete_state, 999, &obsolete_changed) == 0 &&
      obsolete_changed == 1;
  obsolete_changed = 0;
  ok = ok && g_hooks.inputs_changed_obsolete(
                 g_hooks.effect, &obsolete_state, &start, &duration, &obsolete_changed) == 0 &&
      obsolete_changed == 1;
  PfState foreign_state{{9, 8, 7, 6}};
  obsolete_changed = 0x5a;
  ok = ok && g_hooks.has_changed_obsolete(
                 g_hooks.effect, &foreign_state, 1, &obsolete_changed) == kPfBadCallbackParam &&
      obsolete_changed == 0x5a &&
      g_hooks.inputs_changed_obsolete(
          nullptr, &obsolete_state, nullptr, nullptr, &obsolete_changed) ==
          kPfBadCallbackParam && obsolete_changed == 0x5a &&
      release_suite("PF Param Utils Suite", 2) == 0;
  write<int32_t>(parameter_state().records[0].raw, 56, 43);
  ok = ok && get_current_param_state(g_hooks.effect, 1, &start, &duration, &changed) == 0 &&
      are_param_states_identical(g_hooks.effect, &first, &changed, &same) == 0 && same == 0 &&
      is_identical_param_checkout(g_hooks.effect, 1, 0, 1, 24, 10, 1, 24, &identical) == 0 &&
      identical == 1 &&
      find_param_keyframe_time(g_hooks.effect, 1, 0, 24, 0, &found, &key_index,
                               &key_time, &key_scale) == 0 &&
      found == 0 && key_index == -1 && key_time == 0 && key_scale == 24 &&
      get_param_keyframe_count(g_hooks.effect, 1, &count) == 0 && count == -1 &&
      checkout_param_keyframe(g_hooks.effect, 1, 0, nullptr, nullptr, parameter_state().records[0].raw.data()) ==
          kPfInvalidIndex &&
      checkin_param_keyframe(g_hooks.effect, parameter_state().records[0].raw.data()) == kPfInvalidIndex &&
      param_key_index_to_time(g_hooks.effect, 1, 0, &key_time, &key_scale) == kPfInvalidIndex;
  PfState sentinel{{1, 2, 3, 4}};
  changed = sentinel;
  ok = ok && get_current_param_state(nullptr, 1, nullptr, nullptr, &changed) ==
          kPfBadCallbackParam &&
      std::memcmp(&changed, &sentinel, sizeof(changed)) == 0 &&
      get_current_param_state(g_hooks.effect, 999, nullptr, nullptr, &changed) ==
          kPfBadCallbackParam &&
      are_param_states_identical(g_hooks.effect, nullptr, &first, &same) == kPfBadCallbackParam;

  PfState zero{}, random{}, bit_flip = first;
  BCryptGenRandom(nullptr, reinterpret_cast<PUCHAR>(&random), sizeof(random),
                  BCRYPT_USE_SYSTEM_PREFERRED_RNG);
  reinterpret_cast<unsigned char*>(&bit_flip)[7] ^= 0x40;
  same = 0x5a;
  ok = ok && are_param_states_identical(g_hooks.effect, &zero, &first, &same) ==
          kPfBadCallbackParam && same == 0x5a &&
      are_param_states_identical(g_hooks.effect, &random, &first, &same) ==
          kPfBadCallbackParam && same == 0x5a &&
      are_param_states_identical(g_hooks.effect, &bit_flip, &first, &same) ==
          kPfBadCallbackParam && same == 0x5a;
  ok = ok && corrupt_state_owner_for_test(first, g_hooks.layer);
  ok = ok && are_param_states_identical(g_hooks.effect, &first, &second, &same) ==
          kPfBadCallbackParam && same == 0x5a;
  reset_effect_lifetime(true);
  ok = ok && are_param_states_identical(g_hooks.effect, &first, &second, &same) ==
          kPfBadCallbackParam && same == 0x5a;
  fill_registry_to_capacity_for_test(g_hooks.effect, 1);
  changed = sentinel;
  ok = ok && get_current_param_state(g_hooks.effect, 1, nullptr, nullptr, &changed) ==
          kPfBadCallbackParam && std::memcmp(&changed, &sentinel, sizeof(changed)) == 0;
  reset_effect_lifetime(false);
  ok = ok && live_state_count() == 0 &&
      get_current_param_state(g_hooks.effect, 1, nullptr, nullptr, &changed) ==
          kPfBadCallbackParam && release_suite("PF Param Utils Suite", 3) == 0;
  parameter_state().ui.update_active = false;
  parameter_state().ui.active_params = nullptr;
  parameter_state().ui.active_param_count = 0;
  parameter_state().records = saved_params;
  return ok;
}

bool verify_parameter_animation_transport() {
  const auto saved_params = parameter_state().records;
  const auto saved_timelines = parameter_state().timelines;
  const int32_t saved_layer_width = parameter_state().animation_layer_width;
  const int32_t saved_layer_height = parameter_state().animation_layer_height;
  parameter_state().records.clear();
  parameter_state().timelines.clear();
  parameter_state().animation_layer_width = 256;
  parameter_state().animation_layer_height = 144;
  ParamRecord param{};
  param.index = 1;
  param.disk_id = 9001;
  param.type = 10;
  write<int32_t>(param.raw, 0, param.disk_id);
  write<int32_t>(param.raw, kParamType, param.type);
  parameter_state().records.push_back(param);
  ParameterTimeline timeline;
  timeline.slot = 1;
  AnimationKey a{};
  a.time = 0;
  a.scale = 24;
  a.kind = AnimationValueKind::Scalar;
  a.scalar = 10.0;
  AnimationKey b = a;
  b.time = 24;
  b.scalar = 20.0;
  AnimationKey c = b;
  c.time = 48;
  c.scalar = 40.0;
  b.hold = true;
  timeline.keys = {a, b, c};
  parameter_state().timelines.push_back(timeline);
  ParamRecord point{};
  point.index = 2;
  point.disk_id = 9002;
  point.type = 6;
  write<int32_t>(point.raw, 0, point.disk_id);
  write<int32_t>(point.raw, kParamType, point.type);
  parameter_state().records.push_back(point);
  ParameterTimeline point_timeline;
  point_timeline.slot = 2;
  AnimationKey point_key{};
  point_key.time = 0;
  point_key.scale = 24;
  point_key.kind = AnimationValueKind::Components;
  point_key.component_count = 2;
  point_key.components = {50.0, 25.0, 0.0};
  point_timeline.keys = {point_key};
  parameter_state().timelines.push_back(point_timeline);

  ParamRecord point3d{};
  point3d.index = 3;
  point3d.disk_id = 9003;
  point3d.type = 18;
  write<int32_t>(point3d.raw, 0, point3d.disk_id);
  write<int32_t>(point3d.raw, kParamType, point3d.type);
  parameter_state().records.push_back(point3d);
  ParameterTimeline point3d_timeline;
  point3d_timeline.slot = 3;
  AnimationKey point3d_key{};
  point3d_key.time = 0;
  point3d_key.scale = 24;
  point3d_key.kind = AnimationValueKind::Components;
  point3d_key.component_count = 3;
  point3d_key.components = {25.0, 50.0, 75.0};
  point3d_timeline.keys = {point3d_key};
  parameter_state().timelines.push_back(point3d_timeline);

  std::vector<std::array<std::byte, kParamSize>> definitions(4);
  definitions[1] = param.raw;
  definitions[2] = point.raw;
  definitions[3] = point3d.raw;
  uint8_t identical = 1, found = 0;
  int32_t count = 0, index = -1, time = 0;
  uint32_t scale = 0;
  bool ok = apply_parameter_animation(definitions, 12, 24) &&
            std::abs(read<double>(definitions[1], 56) - 15.0) < 1e-12 &&
            read<int32_t>(definitions[2], 56) == 128 * 65536 &&
            read<int32_t>(definitions[2], 60) == 36 * 65536 &&
            std::abs(read<double>(definitions[3], 56) - 64.0) < 1e-12 &&
            std::abs(read<double>(definitions[3], 64) - 72.0) < 1e-12 &&
            std::abs(read<double>(definitions[3], 72) - 108.0) < 1e-12 &&
            apply_parameter_animation(definitions, 36, 24) &&
            std::abs(read<double>(definitions[1], 56) - 20.0) < 1e-12 &&
            get_param_keyframe_count(g_hooks.effect, 1, &count) == 0 && count == 3 &&
            find_param_keyframe_time(g_hooks.effect, 1, 12, 24, 0, &found, &index,
                                     &time, &scale) == 0 &&
            found == 1 && index == 1 && time == 24 && scale == 24 &&
            param_key_index_to_time(g_hooks.effect, 1, 2, &time, &scale) == 0 &&
            time == 48 && scale == 24 &&
            is_identical_param_checkout(g_hooks.effect, 1, 24, 1, 24, 36, 1, 24,
                                        &identical) == 0 &&
            identical == 1 &&
            is_identical_param_checkout(g_hooks.effect, 1, 0, 1, 24, 12, 1, 24,
                                        &identical) == 0 &&
            identical == 0;
  parameter_state().animation_layer_width = 0;
  parameter_state().animation_layer_height = 0;
  ok = ok && aexcompat::l2_detail::verify_classic_animation_extent_wiring_for_test();
  parameter_state().animation_layer_width = 0;
  parameter_state().animation_layer_height = 0;
  ok = ok && worker_runtime::smart_setup::verify_animation_extent_wiring_for_test();
  Definition point_checkout{}, point3d_checkout{};
  ok = ok && checkout_param_keyframe(g_hooks.effect, 2, 0, nullptr, nullptr,
                                     &point_checkout) == 0 &&
       read<int32_t>(point_checkout, 56) == 128 * 65536 &&
       read<int32_t>(point_checkout, 60) == 36 * 65536 &&
       checkin_param_keyframe(g_hooks.effect, &point_checkout) == 0 &&
       checkout_param_keyframe(g_hooks.effect, 3, 0, nullptr, nullptr,
                               &point3d_checkout) == 0 &&
       std::abs(read<double>(point3d_checkout, 56) - 64.0) < 1e-12 &&
       std::abs(read<double>(point3d_checkout, 64) - 72.0) < 1e-12 &&
       std::abs(read<double>(point3d_checkout, 72) - 108.0) < 1e-12 &&
       checkin_param_keyframe(g_hooks.effect, &point3d_checkout) == 0;
  Definition temporal_point{}, temporal_point3d{};
  ok = ok && aexcompat::worker_runtime::parameters::copy_definition_at_time(
                 2, 0, 24, point.raw, temporal_point) &&
       read<int32_t>(temporal_point, 56) == 128 * 65536 &&
       read<int32_t>(temporal_point, 60) == 36 * 65536 &&
       aexcompat::worker_runtime::parameters::copy_definition_at_time(
           3, 0, 24, point3d.raw, temporal_point3d) &&
       std::abs(read<double>(temporal_point3d, 56) - 64.0) < 1e-12 &&
       std::abs(read<double>(temporal_point3d, 64) - 72.0) < 1e-12 &&
       std::abs(read<double>(temporal_point3d, 72) - 108.0) < 1e-12;
  parameter_state().animation_layer_width = 0;
  parameter_state().animation_layer_height = 0;
  ok = ok && apply_parameter_animation(definitions, 0, 24) &&
       read<int32_t>(definitions[2], 56) == 50 * 65536 &&
       read<int32_t>(definitions[2], 60) == 25 * 65536 &&
       std::abs(read<double>(definitions[3], 56) - 25.0) < 1e-12 &&
       std::abs(read<double>(definitions[3], 64) - 50.0) < 1e-12 &&
       std::abs(read<double>(definitions[3], 72) - 75.0) < 1e-12;
  parameter_state().records = saved_params;
  parameter_state().timelines = saved_timelines;
  parameter_state().animation_layer_width = saved_layer_width;
  parameter_state().animation_layer_height = saved_layer_height;
  parameter_state().keyframe_checkout_ledger.clear();
  return ok;
}

bool verify_parameter_registry_capacity() {
  auto& records = parameter_state().records;
  const auto saved_params = records;
  records.clear();
  Definition definition{};
  std::memcpy(definition.data() + kParamName, "capacity", 9);
  bool ok = true;
  for (std::size_t index = 1;
       index <= worker_runtime::parameters::kMaxParameterCount; ++index) {
    write<int32_t>(definition, 0, static_cast<int32_t>(index));
    ok = ok && aexcompat::l2_detail::add_param(
                   nullptr, static_cast<int32_t>(index), definition.data()) == 0;
  }
  ok = ok && records.size() ==
                 worker_runtime::parameters::kMaxParameterCount &&
       records[1264].index == 1265 &&
       aexcompat::l2_detail::add_param(nullptr, -1, definition.data()) != 0;

  wchar_t executable[] = L"worker";
  wchar_t mode[] = L"--user-changed";
  wchar_t plugin[] = L"plugin.aex";
  wchar_t dependency_root[] = L"dependencies";
  wchar_t accepted_slot[] = L"1265";
  wchar_t rejected_slot[] = L"4097";
  wchar_t* accepted_argv[]{executable, mode, plugin, dependency_root,
                           accepted_slot};
  wchar_t* rejected_argv[]{executable, mode, plugin, dependency_root,
                           rejected_slot};
  worker_runtime::invocation::InvocationState accepted{};
  worker_runtime::invocation::InvocationState rejected{};
  const worker_runtime::invocation::L2ModeHooks mode_hooks{
      nullptr, worker_runtime::parameters::kMaxParameterCount};
  ok = ok && worker_runtime::invocation::parse_l2_modes(
                 5, accepted_argv, accepted, mode_hooks) == 0 &&
       accepted.user_changed_param_slot == 1265 &&
       worker_runtime::invocation::parse_l2_modes(
           5, rejected_argv, rejected, mode_hooks) == 3;

  // Popup defaults are 1-based (issue #1253): a declared dephault of 0
  // (Reshape's Elasticity / Interpolation Method) registers as the first
  // choice, the way AE materializes it; a declared in-range default is kept.
  records.clear();
  Definition popup{};
  std::memcpy(popup.data() + kParamName, "popup", 6);
  write<int32_t>(popup, kParamType, 7);
  write<int16_t>(popup, 56 + 4, 3);
  write<int16_t>(popup, 56 + 6, 0);
  ok = ok && aexcompat::l2_detail::add_param(nullptr, -1, popup.data()) == 0;
  write<int16_t>(popup, 56 + 6, 2);
  ok = ok && aexcompat::l2_detail::add_param(nullptr, -1, popup.data()) == 0;
  ok = ok && records.size() == 2 && records[0].type == 7 &&
       records[0].default_value == 1.0 && records[0].valid_max == 3.0 &&
       records[1].default_value == 2.0;
  records = saved_params;
  return ok;
}
}  // namespace aexcompat::parameter_selftests
