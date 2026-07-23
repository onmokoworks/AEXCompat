#include "worker_aegp_pf_interface_suite.hpp"

#include "render_subsystem.h"
#include "worker_aegp_scene.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_smart_runtime.hpp"

#include <cmath>
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

bool valid_spatial_ratio(const aexcompat::render::SpatialRatio& ratio) {
  constexpr int32_t kSpatialNumeratorLimit = 1000000;
  constexpr uint32_t kSpatialDenominatorLimit = 1000000;
  if (ratio.numerator <= 0 || ratio.numerator > kSpatialNumeratorLimit ||
      ratio.denominator == 0 || ratio.denominator > kSpatialDenominatorLimit)
    return false;
  const long double value = static_cast<long double>(ratio.numerator) /
      static_cast<long double>(ratio.denominator);
  return std::isfinite(value) && value > 0.0L && value <= 1000000.0L;
}

bool valid_camera_spatial_context() {
  return valid_spatial_ratio(g_render_context_state.downsample_x) &&
      valid_spatial_ratio(g_render_context_state.downsample_y) &&
      valid_spatial_ratio(g_render_context_state.pixel_aspect_ratio);
}
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

bool invert_affine_matrix(const AegpMatrix4& input, AegpMatrix4& output) {
  constexpr double kMatrixLimit = 1.0e12;
  constexpr double kDeterminantEpsilon = 1.0e-12;
  for (std::size_t row = 0; row < 4; ++row)
    for (std::size_t column = 0; column < 4; ++column)
      if (!std::isfinite(input.mat[row][column]) ||
          std::abs(input.mat[row][column]) > kMatrixLimit) return false;
  if (std::abs(input.mat[3][0]) > kDeterminantEpsilon ||
      std::abs(input.mat[3][1]) > kDeterminantEpsilon ||
      std::abs(input.mat[3][2]) > kDeterminantEpsilon ||
      std::abs(input.mat[3][3] - 1.0) > kDeterminantEpsilon) return false;

  const double a = input.mat[0][0], b = input.mat[0][1], c = input.mat[0][2];
  const double d = input.mat[1][0], e = input.mat[1][1], f = input.mat[1][2];
  const double g = input.mat[2][0], h = input.mat[2][1], i = input.mat[2][2];
  const double determinant = a * (e * i - f * h) -
      b * (d * i - f * g) + c * (d * h - e * g);
  if (!std::isfinite(determinant) || std::abs(determinant) <= kDeterminantEpsilon)
    return false;

  const double inverse_determinant = 1.0 / determinant;
  AegpMatrix4 result{};
  result.mat[0][0] = (e * i - f * h) * inverse_determinant;
  result.mat[0][1] = (c * h - b * i) * inverse_determinant;
  result.mat[0][2] = (b * f - c * e) * inverse_determinant;
  result.mat[1][0] = (f * g - d * i) * inverse_determinant;
  result.mat[1][1] = (a * i - c * g) * inverse_determinant;
  result.mat[1][2] = (c * d - a * f) * inverse_determinant;
  result.mat[2][0] = (d * h - e * g) * inverse_determinant;
  result.mat[2][1] = (b * g - a * h) * inverse_determinant;
  result.mat[2][2] = (a * e - b * d) * inverse_determinant;
  for (std::size_t row = 0; row < 3; ++row) {
    result.mat[row][3] = -(result.mat[row][0] * input.mat[0][3] +
        result.mat[row][1] * input.mat[1][3] +
        result.mat[row][2] * input.mat[2][3]);
  }
  result.mat[3][3] = 1.0;
  for (std::size_t row = 0; row < 4; ++row)
    for (std::size_t column = 0; column < 4; ++column)
      if (!std::isfinite(result.mat[row][column]) ||
          std::abs(result.mat[row][column]) > kMatrixLimit) return false;
  output = result;
  return true;
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
  if (!valid_camera_spatial_context()) return 4;
  const int32_t width = g_full_resolution_width > 0
      ? g_full_resolution_width : smart_state().width;
  const int32_t height = g_full_resolution_height > 0
      ? g_full_resolution_height : smart_state().height;
  if (width <= 0 || height <= 0 || width > INT16_MAX || height > INT16_MAX)
    return 4;

  AegpMatrix4 result{};
  for (std::size_t index = 0; index < 4; ++index) result.mat[index][index] = 1.0;
  void* camera_layer = nullptr;
  if (get_effect_camera(effect, comp_time, &camera_layer) != 0) return 4;
  if (camera_layer) {
    AegpMatrix4 world{};
    if (aegp_get_layer_to_world_xform(camera_layer, comp_time, &world) != 0 ||
        !invert_affine_matrix(world, result)) return 4;
  }
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
