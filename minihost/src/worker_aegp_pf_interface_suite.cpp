#include "worker_aegp_pf_interface_suite.hpp"

#include "render_subsystem.h"
#include "worker_aegp_scene.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_smart_runtime.hpp"

#include <cstdint>
#include <limits>

namespace aexcompat::l2_detail {

using aexcompat::pf_state_runtime::effect_is_live;

// Worker-entry owned effect/layer identity objects stay in l2_main with the
// callback ABI that hands them out; the callbacks here read them cross-TU.
extern OpaqueHostObject g_effect;
extern OpaqueHostObject g_layer;

namespace {
auto& smart_state_ref() { return aexcompat::worker_runtime::smart::state(); }
auto& g_render_context_state = aexcompat::render::render_context_state();
auto& g_full_resolution_width = g_render_context_state.full_resolution_width;
auto& g_full_resolution_height = g_render_context_state.full_resolution_height;
auto& g_aegp_layers = scene_runtime_state().layers;
int32_t& g_aegp_active_camera_layer_index =
    scene_runtime_state().active_camera_layer_index;
auto smart_state() -> decltype(smart_state_ref()) { return smart_state_ref(); }
}  // namespace

int32_t __cdecl get_effect_layer(void* effect, void** layer) {
  if (effect != &g_effect || !layer) return 4;
  *layer = &g_layer;
  return 0;
}

int32_t __cdecl convert_effect_to_comp_time(
    void* effect, int32_t what_time, uint32_t time_scale, AegpTime* comp_time) {
  if (effect != &g_effect || time_scale == 0 || !comp_time) return 4;
  const int64_t checked_value = static_cast<int64_t>(what_time);
  const uint64_t checked_scale = static_cast<uint64_t>(time_scale);
  if (checked_value < (std::numeric_limits<int32_t>::min)() ||
      checked_value > (std::numeric_limits<int32_t>::max)() ||
      checked_scale > (std::numeric_limits<uint32_t>::max)())
    return 4;
  const AegpTime converted{static_cast<int32_t>(checked_value),
                           static_cast<uint32_t>(checked_scale)};
  *comp_time = converted;
  return 0;
}

bool valid_comp_time(const AegpTime& time) {
  if (time.scale == 0) return false;
  constexpr int64_t kCompDurationValue = 300;
  constexpr uint32_t kCompDurationScale = 30;
  const int64_t scaled_time = static_cast<int64_t>(time.value) * kCompDurationScale;
  const int64_t scaled_duration = kCompDurationValue * static_cast<int64_t>(time.scale);
  return scaled_time >= 0 && scaled_time < scaled_duration;
}

bool layer_active_at_time(std::size_t index, const AegpTime& time) {
  if (index >= g_aegp_layer_in_points.size() || index >= g_aegp_layer_durations.size())
    return false;
  const auto& in_point = g_aegp_layer_in_points[index];
  const auto& duration = g_aegp_layer_durations[index];
  if (in_point.scale == 0 || duration.scale == 0 || duration.value <= 0) return false;
  const long double seconds =
      static_cast<long double>(time.value) / static_cast<long double>(time.scale);
  const long double in_seconds =
      static_cast<long double>(in_point.value) / static_cast<long double>(in_point.scale);
  const long double duration_seconds =
      static_cast<long double>(duration.value) / static_cast<long double>(duration.scale);
  return seconds >= in_seconds && seconds < in_seconds + duration_seconds;
}

int32_t __cdecl get_effect_camera(
    void* effect, const AegpTime* comp_time, void** camera_layer) {
  if (effect != &g_effect || !effect_is_live() || !comp_time || !camera_layer ||
      !valid_comp_time(*comp_time)) return 4;
  void* result = nullptr;
  if (g_aegp_active_camera_layer_index >= 0) {
    const auto index = static_cast<std::size_t>(g_aegp_active_camera_layer_index);
    if (index >= g_aegp_layers.size()) return 4;
    if (layer_active_at_time(index, *comp_time)) result = &g_aegp_layers[index];
  }
  *camera_layer = result;
  return 0;
}

int32_t __cdecl get_effect_camera_matrix(void* effect, const AegpTime* comp_time,
    AegpMatrix4* camera_matrix, double* distance_to_image_plane,
    int16_t* image_plane_width, int16_t* image_plane_height) {
  if (effect != &g_effect || !effect_is_live() || !comp_time ||
      !camera_matrix || !distance_to_image_plane || !image_plane_width ||
      !image_plane_height || !valid_comp_time(*comp_time)) return 4;
  const int32_t width = g_full_resolution_width > 0
      ? g_full_resolution_width : smart_state().width;
  const int32_t height = g_full_resolution_height > 0
      ? g_full_resolution_height : smart_state().height;
  if (width <= 0 || height <= 0 || width > INT16_MAX || height > INT16_MAX)
    return 4;

  // The headless scene uses an unrotated default camera and a deterministic
  // image-plane distance until project camera transforms are modeled.
  AegpMatrix4 result{};
  for (std::size_t index = 0; index < 4; ++index) result.mat[index][index] = 1.0;
  *camera_matrix = result;
  *distance_to_image_plane = static_cast<double>(width);
  *image_plane_width = static_cast<int16_t>(width);
  *image_plane_height = static_cast<int16_t>(height);
  return 0;
}



PfInterfaceSuite g_pf_interface_suite{&get_effect_layer, &get_new_effect_for_effect,
    &convert_effect_to_comp_time, &get_effect_camera,
    &get_effect_camera_matrix};

}  // namespace aexcompat::l2_detail
