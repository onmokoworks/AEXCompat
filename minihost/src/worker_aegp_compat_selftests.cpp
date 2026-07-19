#include "worker_aegp_compat_selftests.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_scene_runtime.hpp"
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
bool verify_aegp_resizer_3d_chain() { return g_hooks.resizer_3d && g_hooks.resizer_3d(); }
bool verify_aegp_apply_effect() { return g_hooks.apply_effect && g_hooks.apply_effect(); }
bool verify_aegp_effect_stack() { return g_hooks.effect_stack && g_hooks.effect_stack(); }
bool verify_aegp_projector_levels() { return g_hooks.projector_levels && g_hooks.projector_levels(); }

}  // namespace aexcompat::l2_detail
