#include "worker_aegp_compat_selftests.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_aegp_external_render_runtime.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_pf_helper_runtime.hpp"
#include "worker_pf_state_runtime.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>
#include <limits>
#include <string>

namespace aexcompat::l2_detail {
namespace { AegpCompatSelftestHooks g_hooks; }

namespace {
int32_t compat_acquire_suite(const char* name, int32_t version, const void** suite) {
  return g_hooks.acquire_suite(name, version, suite);
}
int32_t compat_release_suite(const char* name, int32_t version) {
  return g_hooks.release_suite(name, version);
}
bool compat_suite_leases_balanced() { return g_hooks.suite_leases_balanced(); }
}

#define g_aegp_layers scene_runtime_state().layers
#define g_aegp_comp_idle_roundtrip_mode (*g_hooks.comp_idle_roundtrip_mode)
#define g_active_ui_param_count (*g_hooks.active_ui_param_count)
#define g_aegp_stream_acquires scene_runtime_state().stream_acquires
#define g_aegp_stream_disposes scene_runtime_state().stream_disposes
#define g_aegp_stream_value_acquires scene_runtime_state().stream_value_acquires
#define g_aegp_stream_value_disposes scene_runtime_state().stream_value_disposes
#define g_aegp_active_camera_layer_index scene_runtime_state().active_camera_layer_index
#define aegp_get_new_effect_stream_by_index_v2 g_hooks.get_new_effect_stream_v2
#define aegp_get_stream_name_v2 g_hooks.get_stream_name_v2
#define aegp_get_stream_type_v2 g_hooks.get_stream_type_v2
#define aegp_get_new_stream_value_v2 g_hooks.get_new_stream_value_v2
#define aegp_set_stream_value_v2 g_hooks.set_stream_value_v2
#define aegp_dispose_stream_value_v2 g_hooks.dispose_stream_value_v2
#define aegp_dispose_stream_v2 g_hooks.dispose_stream_v2

void configure_aegp_compat_selftests(AegpCompatSelftestHooks hooks) { g_hooks = hooks; }

bool verify_legacy_effect_compat_suites() {
  if (!g_hooks.acquire_suite || !g_hooks.release_suite || !g_hooks.get_comp_bg_color ||
      !g_hooks.convert_effect_time || !g_hooks.comp || !g_hooks.effect) return false;
  const void* comp_suite = nullptr;
  const void* interface_suite = nullptr;
  const void* helper_suite = nullptr;
  bool ok = aexcompat::pf_helper::selftest() &&
      g_hooks.acquire_suite("AEGP Comp Suite", 21, &comp_suite) == 0 &&
      comp_suite == g_hooks.comp_suite &&
      g_hooks.acquire_suite("AEGP PF Interface Suite", 1, &interface_suite) == 0 &&
      interface_suite == g_hooks.interface_suite &&
      g_hooks.acquire_suite("AE Plugin Helper Suite", 1, &helper_suite) == 0 &&
      helper_suite == g_hooks.helper_suite;
  AegpCompatColor color{-1.0, -2.0, -3.0, -4.0};
  const AegpCompatColor color_sentinel = color;
  ok = ok && g_hooks.get_comp_bg_color(g_hooks.comp, &color) == 0 &&
      color.alpha == 1.0 && color.red == 0.0 && color.green == 0.0 && color.blue == 0.0;
  color = color_sentinel;
  ok = ok && g_hooks.get_comp_bg_color(nullptr, &color) != 0 &&
      std::memcmp(&color, &color_sentinel, sizeof(color)) == 0 &&
      g_hooks.get_comp_bg_color(g_hooks.comp_item, &color) != 0 &&
      std::memcmp(&color, &color_sentinel, sizeof(color)) == 0 &&
      g_hooks.get_comp_bg_color(g_hooks.comp, nullptr) != 0;
  suite_abi::AegpTime time{77, 99};
  const auto sentinel = time;
  ok = ok && g_hooks.convert_effect_time(g_hooks.effect, -17, 24000, &time) == 0 &&
      time.value == -17 && time.scale == 24000;
  time = sentinel;
  ok = ok && g_hooks.convert_effect_time(nullptr, 1, 30, &time) != 0 &&
      time.value == sentinel.value && time.scale == sentinel.scale &&
      g_hooks.convert_effect_time(g_hooks.effect, 1, 0, &time) != 0 &&
      time.value == sentinel.value && time.scale == sentinel.scale &&
      g_hooks.convert_effect_time(g_hooks.effect,
          (std::numeric_limits<int32_t>::min)(),
          (std::numeric_limits<uint32_t>::max)(), &time) == 0 &&
      time.value == (std::numeric_limits<int32_t>::min)() &&
      time.scale == (std::numeric_limits<uint32_t>::max)() &&
      g_hooks.convert_effect_time(g_hooks.effect, 0, 1, nullptr) != 0;
  aexcompat::pf_helper::set_effect_tool_for_test(14);
  int32_t tool = -1;
  ok = ok && aexcompat::pf_helper::get_current_tool(&tool) == 0 && tool == 0 &&
      aexcompat::pf_helper::get_current_tool(nullptr) == 516 &&
      aexcompat::pf_helper::effect_tool_for_test() == 14;
  aexcompat::pf_helper::reset();
  ok = g_hooks.release_suite("AE Plugin Helper Suite", 1) == 0 && ok;
  ok = g_hooks.release_suite("AEGP PF Interface Suite", 1) == 0 && ok;
  ok = g_hooks.release_suite("AEGP Comp Suite", 21) == 0 && ok;
  return ok;
}

bool verify_aegp_layer_source_item() {
  if (!g_hooks.get_layer_source_item || !g_hooks.get_item_type ||
      !g_hooks.comp_item || !g_hooks.item_suite ||
      !g_hooks.layer_source_item_calls || !g_hooks.item_type_calls) return false;
  const uint32_t calls_before = *g_hooks.layer_source_item_calls;
  void* item = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  bool ok = g_hooks.get_layer_source_item(g_hooks.pf_layer, &item) == 0 &&
            item == g_hooks.comp_item;
  for (auto& layer : g_aegp_layers) {
    item = nullptr;
    ok = ok && g_hooks.get_layer_source_item(&layer, &item) == 0 &&
         item == g_hooks.comp_item;
  }
  scene_runtime::AegpSceneObject foreign{0x464f5245};
  item = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && g_hooks.get_layer_source_item(&foreign, &item) == 4 &&
       item == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
       g_hooks.get_layer_source_item(nullptr, &item) == 4 &&
       item == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
       g_hooks.get_layer_source_item(g_hooks.pf_layer, nullptr) == 4 &&
       *g_hooks.layer_source_item_calls == calls_before + 4;
  const uint32_t item_type_calls_before = *g_hooks.item_type_calls;
  const bool saved_comp_idle_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_comp_idle_roundtrip_mode = true;
  const void* acquired{};
  ok = ok && compat_acquire_suite("AEGP Item Suite", 14, &acquired) == 0 &&
       acquired == g_hooks.item_suite;
  int16_t item_type = -1;
  ok = ok && g_hooks.get_item_type(g_hooks.comp_item, &item_type) == 0 &&
       item_type == 2;
  scene_runtime::AegpSceneObject foreign_item{0x464f5249};
  item_type = 0x1234;
  ok = ok && g_hooks.get_item_type(&foreign_item, &item_type) == 4 &&
       item_type == 0x1234 && g_hooks.get_item_type(nullptr, &item_type) == 4 &&
       item_type == 0x1234 && g_hooks.get_item_type(g_hooks.comp_item, nullptr) == 4 &&
       *g_hooks.item_type_calls == item_type_calls_before + 1;
  g_aegp_comp_idle_roundtrip_mode = saved_comp_idle_mode;
  return ok;
}

bool verify_camera_case(bool smart_case) {
  if (!g_hooks.get_camera || !g_hooks.set_camera_index || !g_hooks.camera_index ||
      !g_hooks.layer_at || !g_hooks.layer_index) return false;
  const int32_t saved_index = g_hooks.camera_index();
  const bool saved_live = pf_state_runtime::effect_is_live();
  const auto saved_in = g_aegp_layer_in_points;
  const auto saved_duration = g_aegp_layer_durations;
  pf_state_runtime::reset_effect_lifetime(true);
  g_hooks.set_camera_index(-1);
  const suite_abi::AegpTime active{smart_case ? 45 : 15, 30};
  void* camera = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  bool ok = g_hooks.get_camera(g_hooks.effect, &active, &camera) == 0 && !camera;
  g_hooks.set_camera_index(2);
  g_aegp_layer_in_points[2] = {smart_case ? 30 : 10, 30};
  g_aegp_layer_durations[2] = {60, 30};
  camera = nullptr;
  ok = ok && g_hooks.get_camera(g_hooks.effect, &active, &camera) == 0 &&
      camera == g_hooks.layer_at(2) && g_hooks.layer_index(camera) == 2;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  camera = unchanged;
  suite_abi::AegpTime invalid{active.value, 0};
  suite_abi::AegpTime before{smart_case ? 29 : 9, 30};
  suite_abi::AegpTime after{smart_case ? 90 : 70, 30};
  uint32_t foreign = 0x464f5247;
  ok = ok && g_hooks.get_camera(nullptr, &active, &camera) != 0 && camera == unchanged &&
      g_hooks.get_camera(&foreign, &active, &camera) != 0 && camera == unchanged &&
      g_hooks.get_camera(g_hooks.effect, nullptr, &camera) != 0 && camera == unchanged &&
      g_hooks.get_camera(g_hooks.effect, &invalid, &camera) != 0 && camera == unchanged;
  camera = unchanged;
  ok = ok && g_hooks.get_camera(g_hooks.effect, &before, &camera) == 0 && !camera;
  camera = unchanged;
  ok = ok && g_hooks.get_camera(g_hooks.effect, &after, &camera) == 0 && !camera &&
      g_hooks.get_camera(g_hooks.effect, &active, nullptr) != 0;
  camera = unchanged;
  pf_state_runtime::reset_effect_lifetime(false);
  ok = ok && g_hooks.get_camera(g_hooks.effect, &active, &camera) != 0 && camera == unchanged;
  g_hooks.set_camera_index(saved_index);
  g_aegp_layer_in_points = saved_in;
  g_aegp_layer_durations = saved_duration;
  pf_state_runtime::reset_effect_lifetime(saved_live);
  return ok;
}

bool verify_matrix_case(bool smart_case) {
  if (!g_hooks.get_camera_matrix || !g_hooks.get_dimensions ||
      !g_hooks.set_dimensions) return false;
  const bool saved_live = pf_state_runtime::effect_is_live();
  int32_t saved_width = 0, saved_height = 0;
  g_hooks.get_dimensions(&saved_width, &saved_height);
  pf_state_runtime::reset_effect_lifetime(true);
  g_hooks.set_dimensions(smart_case ? 1920 : 640, smart_case ? 1080 : 480);
  const suite_abi::AegpTime time{smart_case ? 45 : 15, 30};
  AegpMatrix4 matrix{};
  double distance = -1.0;
  int16_t width = -1, height = -1;
  const int16_t expected_width = smart_case ? 1920 : 640;
  const int16_t expected_height = smart_case ? 1080 : 480;
  bool ok = g_hooks.get_camera_matrix(g_hooks.effect, &time, &matrix, &distance,
      &width, &height) == 0 && distance == expected_width &&
      width == expected_width && height == expected_height;
  for (std::size_t row = 0; row < 4; ++row)
    for (std::size_t column = 0; column < 4; ++column)
      ok = ok && matrix.mat[row][column] == (row == column ? 1.0 : 0.0);
  AegpMatrix4 sentinel{};
  std::memset(&sentinel, 0x5a, sizeof(sentinel));
  matrix = sentinel; distance = -2.0; width = -2; height = -2;
  suite_abi::AegpTime invalid{time.value, 0};
  ok = ok && g_hooks.get_camera_matrix(nullptr, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0 &&
      g_hooks.get_camera_matrix(g_hooks.effect, &invalid, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0;
  pf_state_runtime::reset_effect_lifetime(false);
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0;
  g_hooks.set_dimensions(saved_width, saved_height);
  pf_state_runtime::reset_effect_lifetime(saved_live);
  return ok;
}

bool verify_aegp_get_effect_camera() {
  if (!g_hooks.acquire_suite || !g_hooks.release_suite) return false;
  const void* suite = nullptr;
  bool ok = g_hooks.acquire_suite("AEGP PF Interface Suite", 1, &suite) == 0 &&
      suite == g_hooks.interface_suite && verify_camera_case(false) &&
      verify_camera_case(true) && verify_matrix_case(false) && verify_matrix_case(true);
  ok = g_hooks.release_suite("AEGP PF Interface Suite", 1) == 0 && ok;
  return ok && (!g_hooks.suite_leases_balanced || g_hooks.suite_leases_balanced());
}

#define acquire_suite compat_acquire_suite
#define release_suite compat_release_suite
#define suite_leases_balanced compat_suite_leases_balanced
bool verify_aegp_apply_effect() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const bool saved_live = g_aegp_effect_live;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const uint32_t timestamp_before =
      aexcompat::aegp_external_render_runtime::project_generation();
  g_aegp_effect_instances = {};
  g_aegp_effect_instances[0] = {
      &g_aegp_layers[0], kAegpInstalledEffects[0].key, 0, 1, 1, true};
  g_aegp_effect_leases = {};
  g_aegp_effect_live = false;
  g_aegp_comp_idle_roundtrip_mode = true;

  const void* suite2 = nullptr;
  const void* suite3 = nullptr;
  const void* suite4 = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 2, &suite2) == 0 &&
      acquire_suite("AEGP Effect Suite", 3, &suite3) == 0 &&
      acquire_suite("AEGP Effect Suite", 4, &suite4) == 0;
  if (suite2 && suite3 && suite4) {
    ok = ok && static_cast<void* const*>(const_cast<void*>(suite2))[9] ==
                   reinterpret_cast<void*>(&aegp_apply_effect) &&
        static_cast<void* const*>(const_cast<void*>(suite3))[9] ==
                   reinterpret_cast<void*>(&aegp_apply_effect) &&
        static_cast<void* const*>(const_cast<void*>(suite4))[9] ==
                   reinterpret_cast<void*>(&aegp_apply_effect);
  }

  int32_t layer0_count = -1;
  int32_t layer1_count = -1;
  void* applied = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_layer_num_effects(&g_aegp_layers[0], &layer0_count) == 0 &&
      layer0_count == 1 && aegp_get_layer_num_effects(&g_aegp_layers[1], &layer1_count) == 0 &&
      layer1_count == 0 && aegp_apply_effect(7, &g_aegp_layers[1],
          kAegpInstalledEffects[0].key, &applied) == 0 && applied &&
      applied != reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_layer_num_effects(&g_aegp_layers[1], &layer1_count) == 0 &&
      layer1_count == 1;

  void* stream = nullptr;
  ok = ok && aegp_get_new_effect_stream_by_index(7, applied, 1, &stream) == 0 && stream &&
      g_aegp_transform_stream.layer == &g_aegp_layers[1] &&
      g_aegp_transform_stream.effect_instance_index == 1 &&
      g_aegp_transform_stream.effect_instance_generation ==
          g_aegp_effect_instances[1].generation &&
      aegp_dispose_stream(stream) == 0;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  const auto scene_before_failures = g_aegp_effect_instances;
  const uint32_t timestamp_after_apply =
      aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && aegp_get_new_effect_stream_by_index(8, applied, 1, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      aegp_apply_effect(7, nullptr, kAegpInstalledEffects[0].key, &unchanged) == 4 &&
      aegp_apply_effect(7, &g_aegp_layers[1], 9999, &unchanged) == 4 &&
      aegp_apply_effect(7, &g_aegp_layers[1], kAegpInstalledEffects[0].key, nullptr) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      std::memcmp(g_aegp_effect_instances.data(), scene_before_failures.data(),
                  sizeof(g_aegp_effect_instances)) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() == timestamp_after_apply;

  std::array<void*, kAegpEffectInstanceCapacity - 2> more{};
  for (std::size_t i = 0; ok && i < more.size(); ++i)
    ok = aegp_apply_effect(7, &g_aegp_layers[2], kAegpInstalledEffects[0].key,
                          &more[i]) == 0;
  unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc));
  const auto full_scene = g_aegp_effect_instances;
  ok = ok && aegp_apply_effect(7, &g_aegp_layers[2], kAegpInstalledEffects[0].key,
                              &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc)) &&
      std::memcmp(g_aegp_effect_instances.data(), full_scene.data(),
                  sizeof(g_aegp_effect_instances)) == 0;

  const void* stale = applied;
  int32_t key = -1;
  ok = ok && aegp_dispose_effect(applied) == 0 &&
      aegp_get_installed_key_from_layer_effect(const_cast<void*>(stale), &key) == 4 &&
      aegp_dispose_effect(const_cast<void*>(stale)) == 4 &&
      aegp_dispose_effect(g_hooks.effect) == 4;
  void* reacquired = nullptr;
  ok = ok && aegp_get_layer_effect_by_index(7, &g_aegp_layers[1], 0, &reacquired) == 0 &&
      reacquired != stale && aegp_get_installed_key_from_layer_effect(reacquired, &key) == 0 &&
      key == kAegpInstalledEffects[0].key && aegp_dispose_effect(reacquired) == 0;
  for (void* lease : more) if (lease) ok = aegp_dispose_effect(lease) == 0 && ok;

  ok = release_suite("AEGP Effect Suite", 4) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 3) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 2) == 0 && ok;
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_effect_live = saved_live;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  return ok && timestamp_before != timestamp_after_apply && suite_leases_balanced();
}
bool verify_aegp_effect_stack() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const auto saved_streams = g_aegp_legacy_effect_streams;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const std::size_t saved_param_count = g_active_ui_param_count;
  g_aegp_effect_instances = {};
  g_aegp_effect_instances[0] = {
      &g_aegp_layers[0], kAegpInstalledEffects[0].key, 0, 1, 1, true};
  g_aegp_effect_leases = {};
  g_aegp_legacy_effect_streams = {};
  g_aegp_comp_idle_roundtrip_mode = true;
  g_active_ui_param_count = 5;

  const void* suite2 = nullptr;
  const void* suite3 = nullptr;
  const void* suite4 = nullptr;
  const void* stream_suite2 = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 2, &suite2) == 0 &&
      acquire_suite("AEGP Effect Suite", 3, &suite3) == 0 &&
      acquire_suite("AEGP Effect Suite", 4, &suite4) == 0 &&
      acquire_suite("AEGP Stream Suite", 7, &stream_suite2) == 0;
  for (const void* suite : {suite2, suite3, suite4}) {
    if (!suite) { ok = false; continue; }
    const auto* slots = static_cast<void* const*>(const_cast<void*>(suite));
    ok = ok && slots[5] == reinterpret_cast<void*>(&aegp_set_effect_flags) &&
        slots[6] == reinterpret_cast<void*>(&aegp_reorder_effect) &&
        slots[10] == reinterpret_cast<void*>(&aegp_delete_layer_effect) &&
        slots[16] == reinterpret_cast<void*>(&aegp_duplicate_effect);
  }

  void* first = nullptr;
  void* second = nullptr;
  void* duplicate = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_apply_effect(7, &g_aegp_layers[1],
                 kAegpInstalledEffects[0].key, &first) == 0 &&
      aegp_apply_effect(7, &g_aegp_layers[1],
                 kAegpInstalledEffects[0].key, &second) == 0 &&
      aegp_duplicate_effect(first, &duplicate) == 0 && duplicate &&
      duplicate != reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  std::size_t first_index = 0;
  std::size_t second_index = 0;
  std::size_t duplicate_index = 0;
  ok = ok && resolve_effect_instance(first, 7, &first_index) &&
      resolve_effect_instance(second, 7, &second_index) &&
      resolve_effect_instance(duplicate, 7, &duplicate_index) &&
      g_aegp_effect_instances[first_index].stack_order == 0 &&
      g_aegp_effect_instances[duplicate_index].stack_order == 1 &&
      g_aegp_effect_instances[second_index].stack_order == 2;

  uint32_t flags = 0;
  ok = ok && aegp_set_effect_flags(duplicate, 3, 2) == 0 &&
      aegp_get_effect_flags(duplicate, &flags) == 0 && flags == 2 &&
      aegp_reorder_effect(duplicate, 2) == 0 &&
      g_aegp_effect_instances[second_index].stack_order == 1 &&
      g_aegp_effect_instances[duplicate_index].stack_order == 2;

  void* stream = nullptr;
  AegpTime time{0, 30};
  AegpStreamValue value{};
  char name[64]{};
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, duplicate, 1, &stream) == 0 &&
      aegp_get_stream_name_v2(stream, 1, name) == 0 &&
      std::strcmp(name, "Amount") == 0 &&
      aegp_get_new_stream_value_v2(8, stream, 1, &time, 0, &value) == 4 &&
      aegp_get_new_stream_value_v2(7, stream, 1, &time, 0, &value) == 0;
  double changed = 62.745098;
  std::memcpy(value.value.data(), &changed, sizeof(changed));
  ok = ok && aegp_set_stream_value_v2(7, stream, &value) == 0 &&
      aegp_dispose_stream_value_v2(&value) == 0 &&
      std::abs(g_aegp_effect_instances[duplicate_index].parameter_values[0][0] -
               changed) < 0.000001 &&
      aegp_delete_layer_effect(duplicate) == 0;
  int32_t type = -1;
  int32_t count = -1;
  ok = ok && aegp_get_stream_type_v2(stream, &type) == 4 &&
      aegp_get_layer_num_effects(&g_aegp_layers[1], &count) == 0 && count == 2 &&
      aegp_dispose_stream_v2(stream) == 0 &&
      aegp_get_effect_flags(duplicate, &flags) == 4 &&
      aegp_dispose_effect(duplicate) == 0;

  const auto snapshot = g_aegp_effect_instances;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  ok = ok && aegp_reorder_effect(first, 2) == 4 &&
      aegp_set_effect_flags(first, 1, 2) == 4 &&
      aegp_duplicate_effect(nullptr, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      std::memcmp(snapshot.data(), g_aegp_effect_instances.data(), sizeof(snapshot)) == 0;

  ok = aegp_dispose_effect(first) == 0 && ok;
  ok = aegp_dispose_effect(second) == 0 && ok;
  ok = release_suite("AEGP Stream Suite", 7) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 4) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 3) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 2) == 0 && ok;
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_legacy_effect_streams = saved_streams;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  g_active_ui_param_count = saved_param_count;
  return ok && suite_leases_balanced();
}
bool verify_aegp_projector_levels() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const auto saved_streams = g_aegp_legacy_effect_streams;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const uint32_t stream_acquires_before = g_aegp_stream_acquires;
  const uint32_t stream_disposes_before = g_aegp_stream_disposes;
  const uint32_t value_acquires_before = g_aegp_stream_value_acquires;
  const uint32_t value_disposes_before = g_aegp_stream_value_disposes;
  g_aegp_effect_instances = {};
  g_aegp_effect_leases = {};
  g_aegp_legacy_effect_streams = {};
  g_aegp_comp_idle_roundtrip_mode = true;

  bool ok = true;
  int32_t count = -1;
  int32_t key = kAegpInstalledEffectKeyNone;
  int32_t easy_levels_key = kAegpInstalledEffectKeyNone;
  std::array<char, kAegpMaxEffectCategoryNameSize> match_name{};
  ok = aegp_get_num_installed_effects(&count) == 0 && count == 3;
  for (int32_t index = 0; ok && index < count; ++index) {
    ok = aegp_get_next_installed_effect(key, &key) == 0 && key != 0 &&
         aegp_get_effect_match_name(key, match_name.data()) == 0;
    if (ok && std::strcmp(match_name.data(), "ADBE Easy Levels") == 0)
      easy_levels_key = key;
  }
  int32_t end_key = -1;
  ok = ok && easy_levels_key != 0 &&
       aegp_get_next_installed_effect(key, &end_key) == 0 && end_key == 0;

  void* effect = nullptr;
  ok = ok && aegp_apply_effect(7, &g_aegp_layers[1], easy_levels_key, &effect) == 0;
  ok = ok && aegp_get_effect_num_param_streams_v2(effect, &count) == 0 && count == 7;
  void* input = nullptr;
  void* black = nullptr;
  void* white = nullptr;
  void* black_duplicate = nullptr;
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 0, &input) == 0 &&
       aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &black) == 0 &&
       aegp_get_new_effect_stream_by_index_v2(7, effect, 5, &white) == 0 &&
       aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &black_duplicate) == 0 &&
       input != black && black != white && black != black_duplicate;

  char name[64]{};
  int32_t type = -1;
  AegpTime time{0, 30};
  AegpStreamValue input_value{}, black_value{}, white_value{}, duplicate_value{};
  ok = ok && aegp_get_stream_name_v2(input, 1, name) == 0 &&
       std::strcmp(name, "Input") == 0 && aegp_get_stream_type_v2(input, &type) == 0 &&
       type == 9 && aegp_get_new_stream_value_v2(7, input, 1, &time, 1, &input_value) == 0;
  void* input_layer = nullptr;
  std::memcpy(&input_layer, input_value.value.data(), sizeof(input_layer));
  ok = ok && input_layer == &g_aegp_layers[1] &&
       aegp_set_stream_value_v2(7, input, &input_value) == 4;

  ok = ok && aegp_get_stream_name_v2(black, 1, name) == 0 &&
       std::strcmp(name, "Input Black") == 0 && aegp_get_stream_type_v2(black, &type) == 0 &&
       type == 5 && aegp_get_new_stream_value_v2(7, black, 1, &time, 1, &black_value) == 0 &&
       aegp_get_stream_name_v2(white, 1, name) == 0 &&
       std::strcmp(name, "Input White") == 0 && aegp_get_stream_type_v2(white, &type) == 0 &&
       type == 5 && aegp_get_new_stream_value_v2(7, white, 1, &time, 1, &white_value) == 0 &&
       aegp_get_new_stream_value_v2(7, black_duplicate, 1, &time, 1, &duplicate_value) == 0;
  double black_default = -1.0, white_default = -1.0, duplicate_default = -1.0;
  std::memcpy(&black_default, black_value.value.data(), sizeof(double));
  std::memcpy(&white_default, white_value.value.data(), sizeof(double));
  std::memcpy(&duplicate_default, duplicate_value.value.data(), sizeof(double));
  const double black_changed = .62745098;
  const double white_changed = .92156862745;
  std::memcpy(black_value.value.data(), &black_changed, sizeof(double));
  std::memcpy(white_value.value.data(), &white_changed, sizeof(double));
  ok = ok && black_default == 0.0 && white_default == 1.0 && duplicate_default == 0.0 &&
       aegp_set_stream_value_v2(8, black, &black_value) == 4 &&
       aegp_set_stream_value_v2(7, black, &black_value) == 0 &&
       aegp_set_stream_value_v2(7, white, &white_value) == 0;

  const AegpStreamValue stale_black_value = black_value;
  ok = ok && aegp_dispose_stream_value_v2(&duplicate_value) == 0 &&
       aegp_dispose_stream_value_v2(&white_value) == 0 &&
       aegp_dispose_stream_value_v2(&black_value) == 0;
  AegpStreamValue black_readback{}, white_readback{};
  ok = ok && aegp_get_new_stream_value_v2(7, black_duplicate, 1, &time, 1,
                                           &black_readback) == 0 &&
       aegp_get_new_stream_value_v2(7, white, 1, &time, 1, &white_readback) == 0;
  double black_actual = 0.0, white_actual = 0.0;
  std::memcpy(&black_actual, black_readback.value.data(), sizeof(double));
  std::memcpy(&white_actual, white_readback.value.data(), sizeof(double));
  ok = ok && std::abs(black_actual - black_changed) < 1e-12 &&
       std::abs(white_actual - white_changed) < 1e-12 &&
       aegp_dispose_stream_value_v2(&white_readback) == 0 &&
       aegp_dispose_stream_value_v2(&black_readback) == 0 &&
       aegp_dispose_stream_value_v2(&input_value) == 0;
  AegpStreamValue current_black_value{};
  AegpStreamValue stale_copy = stale_black_value;
  ok = ok && aegp_get_new_stream_value_v2(7, black, 1, &time, 1,
                                           &current_black_value) == 0 &&
       aegp_dispose_stream_value_v2(&stale_copy) == 4 &&
       stale_copy.stream == black &&
       aegp_dispose_stream_value_v2(&current_black_value) == 0;

  std::array<void*, kAegpLegacyEffectStreamCapacity - 4> capacity_streams{};
  for (std::size_t index = 0; ok && index < capacity_streams.size(); ++index)
    ok = aegp_get_new_effect_stream_by_index_v2(7, effect, 4,
                                                &capacity_streams[index]) == 0;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &unchanged) == 4 &&
       unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  for (auto iterator = capacity_streams.rbegin(); iterator != capacity_streams.rend(); ++iterator)
    if (*iterator) ok = aegp_dispose_stream_v2(*iterator) == 0 && ok;
  ok = aegp_dispose_stream_v2(black_duplicate) == 0 && ok;
  ok = aegp_dispose_stream_v2(white) == 0 && ok;
  ok = aegp_dispose_stream_v2(black) == 0 && ok;
  ok = aegp_dispose_stream_v2(input) == 0 && ok;

  void* reused_stream = nullptr;
  type = 0x12345678;
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &reused_stream) == 0 &&
       reused_stream != black && aegp_get_stream_type_v2(black, &type) == 4 &&
       type == 0x12345678 && aegp_get_stream_type_v2(reused_stream, &type) == 0 &&
       type == 5 && aegp_dispose_stream_v2(reused_stream) == 0;

  void* stale_stream = nullptr;
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &stale_stream) == 0 &&
       aegp_delete_layer_effect(effect) == 0;
  type = 0x12345678;
  ok = ok && aegp_get_stream_type_v2(stale_stream, &type) == 4 &&
       type == 0x12345678 && aegp_dispose_stream_v2(stale_stream) == 0 &&
       aegp_dispose_effect(effect) == 0;

  ok = ok && g_aegp_stream_acquires - stream_acquires_before ==
                 g_aegp_stream_disposes - stream_disposes_before &&
       g_aegp_stream_value_acquires - value_acquires_before ==
                 g_aegp_stream_value_disposes - value_disposes_before;
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_legacy_effect_streams = saved_streams;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  return ok;
}
bool verify_aegp_resizer_3d_chain() {
  const void* layer_suite = nullptr;
  const void* stream_suite = nullptr;
  const void* comp_suite = nullptr;
  const void* item_suite = nullptr;
  const int32_t saved_camera_index = g_aegp_active_camera_layer_index;
  int32_t saved_width = 0, saved_height = 0;
  g_hooks.get_dimensions(&saved_width, &saved_height);
  const auto saved_in_points = g_aegp_layer_in_points;
  const auto saved_durations = g_aegp_layer_durations;
  const auto saved_transforms = g_aegp_layer_transforms;
  g_aegp_active_camera_layer_index = 2;
  g_aegp_layer_in_points[2] = {0, 30};
  g_aegp_layer_durations[2] = {300, 30};
  g_hooks.set_dimensions(1920, 1080);
  const AegpTime time{45, 30};

  bool ok = acquire_suite("AEGP Layer Suite", 14, &layer_suite) == 0 &&
      layer_suite == g_aegp_layer_suite8.data() &&
      g_aegp_layer_suite8[38] == reinterpret_cast<void*>(&aegp_get_layer_to_world_xform) &&
      acquire_suite("AEGP Stream Suite", 7, &stream_suite) == 0 &&
      stream_suite == g_aegp_stream_suite2.data() &&
      g_aegp_stream_suite2[16] == reinterpret_cast<void*>(&aegp_get_layer_stream_value_v2) &&
      acquire_suite("AEGP Comp Suite", 9, &comp_suite) == 0 &&
      comp_suite == g_aegp_comp_suite4.data() &&
      g_aegp_comp_suite4[1] == reinterpret_cast<void*>(&aegp_get_item_from_comp) &&
      acquire_suite("AEGP Item Suite", 10, &item_suite) == 0 &&
      item_suite == &g_aegp_legacy_item_suite6 &&
      reinterpret_cast<void**>(&g_aegp_legacy_item_suite6)[16] ==
          reinterpret_cast<void*>(&aegp_get_item_dimensions);
  AegpMatrix4 matrix{};
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) == 0;
  for (std::size_t row = 0; row < 4; ++row) {
    for (std::size_t column = 0; column < 4; ++column)
      ok = ok && matrix.mat[row][column] == (row == column ? 1.0 : 0.0);
  }
  g_aegp_layer_transforms[2] = {};
  g_aegp_layer_transforms[2].anchor = {{1.0, 2.0, 3.0}};
  g_aegp_layer_transforms[2].position = {{10.0, 20.0, 30.0}};
  g_aegp_layer_transforms[2].scale = {{200.0, 50.0, 100.0}};
  g_aegp_layer_transforms[2].rotation_degrees = {{0.0, 0.0, 90.0}};
  g_aegp_layer_transforms[2].is_3d = true;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) == 0;
  const auto near = [](double actual, double expected) {
    return std::abs(actual - expected) < 1.0e-9;
  };
  const double expected[4][4] = {
      {0.0, -0.5, 0.0, 11.0},
      {2.0, 0.0, 0.0, 18.0},
      {0.0, 0.0, 1.0, 27.0},
      {0.0, 0.0, 0.0, 1.0}};
  for (std::size_t row = 0; row < 4; ++row)
    for (std::size_t column = 0; column < 4; ++column)
      ok = ok && near(matrix.mat[row][column], expected[row][column]);
  AegpMatrix4 matrix_sentinel{};
  std::memset(&matrix_sentinel, 0x5a, sizeof(matrix_sentinel));
  g_aegp_layer_transforms[2].scale[0] = 0.0;
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  g_aegp_layer_transforms[2].scale[0] = 200.0;
  g_aegp_layer_transforms[2].rotation_degrees[1] =
      std::numeric_limits<double>::quiet_NaN();
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  int foreign_layer = 0;
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&foreign_layer, &time, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  g_aegp_layer_transforms[2] = saved_transforms[2];
  AegpLegacyStreamVal zoom{-1.0};
  int32_t type = -1;
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 11, 1,
      &time, 0, &zoom, &type) == 0 && zoom.one_d == 1920.0 && type == 5;
  void* item = nullptr;
  int32_t width = -1;
  int32_t height = -1;
  ok = ok && aegp_get_item_from_comp(g_hooks.comp, &item) == 0 &&
      item == g_hooks.comp_item &&
      aegp_get_item_dimensions(item, &width, &height) == 0 &&
      width == 1920 && height == 1080;

  AegpLegacyStreamVal sentinel{-2.0};
  type = -2;
  AegpTime invalid{45, 0};
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[1], 11, 1,
      &time, 0, &sentinel, &type) != 0 && sentinel.one_d == -2.0 && type == -2 &&
      aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 10, 1,
      &time, 0, &sentinel, &type) != 0 && sentinel.one_d == -2.0 && type == -2;
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &invalid, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  void* item_sentinel = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  width = -2;
  height = -3;
  ok = ok && aegp_get_item_from_comp(g_hooks.pf_layer, &item_sentinel) != 0 &&
      item_sentinel == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
      aegp_get_item_dimensions(g_hooks.pf_layer, &width, &height) != 0 &&
      width == -2 && height == -3;

  ok = release_suite("AEGP Item Suite", 10) == 0 && ok;
  ok = release_suite("AEGP Comp Suite", 9) == 0 && ok;
  ok = release_suite("AEGP Stream Suite", 7) == 0 && ok;
  ok = release_suite("AEGP Layer Suite", 14) == 0 && ok;
  g_aegp_active_camera_layer_index = saved_camera_index;
  g_hooks.set_dimensions(saved_width, saved_height);
  g_aegp_layer_in_points = saved_in_points;
  g_aegp_layer_durations = saved_durations;
  g_aegp_layer_transforms = saved_transforms;
  return ok && suite_leases_balanced();
}

#define g_aegp_effect scene_runtime_state().effect
#define aegp_get_effect_param_union_by_index_v3 g_hooks.get_effect_param_union_v3

bool verify_aegp_effect_param_union_suite4() {
  constexpr std::size_t kParamSize = worker_runtime::parameters::kDefinitionSize;
  const bool saved_live = g_aegp_effect_live;
  g_aegp_effect_live = true;
  std::array<std::byte, kParamSize - 56> value{};
  std::array<std::byte, kParamSize - 56> sentinel{};
  sentinel.fill(std::byte{0x5a});
  int32_t type = -1;
  bool ok = aegp_get_effect_param_union_by_index_v3(
                1, &g_aegp_effect, 0, &type, value.data()) == 0 &&
            type == 0 &&
            std::all_of(value.begin(), value.end(),
                        [](std::byte byte) { return byte == std::byte{0}; });
  value = sentinel;
  type = -1;
  ok = ok && aegp_get_effect_param_union_by_index_v3(
                 1, &g_aegp_effect, 1, &type, value.data()) == 0 &&
       type == 1;
  for (const int32_t index : {-1, 5}) {
    value = sentinel;
    type = 0x12345678;
    ok = ok && aegp_get_effect_param_union_by_index_v3(
                   1, &g_aegp_effect, index, &type, value.data()) == 4 &&
         type == 0x12345678 && value == sentinel;
  }
  value = sentinel;
  type = 0x12345678;
  ok = ok && aegp_get_effect_param_union_by_index_v3(
                 1, &g_aegp_effect, 0, nullptr, value.data()) == 4 &&
       value == sentinel &&
       aegp_get_effect_param_union_by_index_v3(
           1, &g_aegp_effect, 0, &type, nullptr) == 4 &&
       type == 0x12345678;
  g_aegp_effect_live = false;
  ok = ok && aegp_get_effect_param_union_by_index_v3(
                 1, &g_aegp_effect, 0, &type, value.data()) == 4;
  g_aegp_effect_live = saved_live;
  return ok;
}

bool verify_aegp_installed_effect_catalog_suite4() {
  const bool saved_comp_idle_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_comp_idle_roundtrip_mode = true;
  const void* acquired = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 4, &acquired) == 0 &&
            acquired == g_aegp_effect_suite4.data();
  int32_t count = -1;
  ok = ok && aegp_get_num_installed_effects(&count) == 0 &&
       count == static_cast<int32_t>(kAegpInstalledEffects.size());
  count = 0x12345678;
  ok = ok && aegp_get_num_installed_effects(nullptr) == 4 && count == 0x12345678;

  int32_t key = -1;
  ok = ok && aegp_get_next_installed_effect(kAegpInstalledEffectKeyNone, &key) == 0 &&
       key == kAegpInstalledEffects[0].key;
  const int32_t installed_key = key;
  for (std::size_t index = 1; index < kAegpInstalledEffects.size(); ++index)
    ok = ok && aegp_get_next_installed_effect(key, &key) == 0 &&
         key == kAegpInstalledEffects[index].key;
  ok = ok && aegp_get_next_installed_effect(key, &key) == 0 &&
       key == kAegpInstalledEffectKeyNone;
  key = 0x12345678;
  ok = ok && aegp_get_next_installed_effect(9999, &key) == 4 &&
       key == 0x12345678 &&
       aegp_get_next_installed_effect(kAegpInstalledEffectKeyNone, nullptr) == 4;

  std::array<char, kAegpMaxEffectCategoryNameSize> name{};
  std::array<char, kAegpMaxEffectCategoryNameSize> match_name{};
  std::array<char, kAegpMaxEffectCategoryNameSize> category{};
  ok = ok && aegp_get_effect_name(installed_key, name.data()) == 0 &&
       std::strcmp(name.data(), kAegpInstalledEffects[0].name) == 0 &&
       aegp_get_effect_match_name(installed_key, match_name.data()) == 0 &&
       std::strcmp(match_name.data(), kAegpInstalledEffects[0].match_name) == 0 &&
       aegp_get_effect_category(installed_key, category.data()) == 0 &&
       std::strcmp(category.data(), kAegpInstalledEffects[0].category) == 0 &&
       category[std::strlen(kAegpInstalledEffects[0].category)] == '\0';
  category.fill('Z');
  ok = ok && aegp_get_effect_category(9999, category.data()) == 4 &&
       std::all_of(category.begin(), category.end(), [](char value) { return value == 'Z'; }) &&
       aegp_get_effect_category(installed_key, nullptr) == 4;
  g_aegp_comp_idle_roundtrip_mode = saved_comp_idle_mode;
  return ok;
}
}  // namespace aexcompat::l2_detail
