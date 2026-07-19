#include "worker_pf_path_selftests.hpp"
#include "worker_pf_path_runtime.hpp"

#include <cmath>
#include <utility>

namespace aexcompat::l2_detail {

namespace {
using aexcompat::mask_runtime::CurveSnapshot;
using aexcompat::mask_runtime::CurveVertex;

bool install_scene(std::vector<CurveSnapshot> curves) {
  const auto hook = aexcompat::mask_runtime::host_context().install_synthetic_scene;
  return hook && hook(curves);
}
void* effect_ref() { return aexcompat::pf_path_runtime::host_hooks().effect_ref; }
}  // namespace

bool verify_pf_path_data_hardening(aexcompat::pf_path_runtime::HostHooks path_hooks,
    aexcompat::mask_runtime::HostContext mask_hooks) {
  aexcompat::mask_runtime::configure_host_context(mask_hooks);
  aexcompat::pf_path_runtime::configure(path_hooks);
  aexcompat::pf_path_runtime::reset();

  const CurveVertex vertex{1, 2, 0, 0, 0, 0};
  if (!install_scene({{1, true, {}}, {2, false, {}},
                      {3, true, {vertex}}, {4, false, {vertex, vertex}}}))
    return false;

  for (const int32_t id : {1, 2, 3, 4}) {
    void* path = nullptr;
    int32_t segments = -1;
    void* prep = nullptr;
    if (aexcompat::pf_path_runtime::checkout_path(effect_ref(), id, 0, 1, 1, &path) != 0 ||
        aexcompat::pf_path_runtime::path_num_segments(effect_ref(), path, &segments) != 0 || segments < 0 ||
        (id != 4 && segments != 0) || (id == 4 && segments != 1) ||
        (segments == 0 && aexcompat::pf_path_runtime::path_prepare_seg_length(
            nullptr, path, 0, 1, &prep) == 0) || prep != nullptr ||
        aexcompat::pf_path_runtime::checkin_path(effect_ref(), id, 0, path) != 0)
      return false;
  }

  if (!install_scene({{5, false, {{0, 0, 0, 0, 0, 0}, {4, 0, 0, 0, 0, 0},
                                  {4, 4, 0, 0, 0, 0}, {0, 0, 0, 0, 0, 0}}},
                      {6, true, {{0, 0, 0, 0, 1, 0}, {1, 0, -1, 0, 0, 0}}}}))
    return false;
  void* path = nullptr;
  void* foreign_path = nullptr;
  void* prep = nullptr;
  if (aexcompat::pf_path_runtime::checkout_path(effect_ref(), 5, 0, 1, 1, &path) != 0 ||
      aexcompat::pf_path_runtime::checkout_path(effect_ref(), 6, 0, 1, 1, &foreign_path) != 0 ||
      aexcompat::pf_path_runtime::path_prepare_seg_length(effect_ref(), path, 0, 4, &prep) != 0)
    return false;
  void* live_prep = prep;
  void* stale_prep = prep;
  double length = 0;
  if (aexcompat::pf_path_runtime::path_cleanup_seg_length(effect_ref(), path, 1, &prep) == 0 || prep != live_prep ||
      aexcompat::pf_path_runtime::path_cleanup_seg_length(effect_ref(), foreign_path, 0, &prep) == 0 || prep != live_prep ||
      aexcompat::pf_path_runtime::checkin_path(effect_ref(), 6, 0, foreign_path) != 0 ||
      aexcompat::pf_path_runtime::checkin_path(effect_ref(), 5, 0, path) != 0 ||
      aexcompat::pf_path_runtime::path_get_seg_length(effect_ref(), path, 0, &prep, &length) == 0 ||
      aexcompat::pf_path_runtime::path_cleanup_seg_length(effect_ref(), path, 0, &prep) != 0 || prep != nullptr ||
      aexcompat::pf_path_runtime::path_cleanup_seg_length(effect_ref(), path, 0, &stale_prep) == 0 ||
      stale_prep != live_prep)
    return false;

  const auto verify_curve = [](int32_t id, bool open, std::vector<CurveVertex> vertices,
                               double expected_length, double length_tolerance,
                               double expected_mid_x, double expected_mid_y,
                               double position_tolerance, bool expect_zero_derivative) {
    if (!open) vertices.push_back(vertices.front());
    if (!install_scene({{id, open, std::move(vertices)}})) return false;
    void* curve_path = nullptr;
    void* curve_prep = nullptr;
    if (aexcompat::pf_path_runtime::checkout_path(effect_ref(), id, 0, 1, 1, &curve_path) != 0 ||
        aexcompat::pf_path_runtime::path_prepare_seg_length(effect_ref(), curve_path, 0, 1, &curve_prep) != 0)
      return false;
    double curve_length = 0.0, x = 0.0, y = 0.0, dx = 0.0, dy = 0.0;
    const bool evaluated =
        aexcompat::pf_path_runtime::path_get_seg_length(effect_ref(), curve_path, 0, &curve_prep, &curve_length) == 0 &&
        std::abs(curve_length - expected_length) <= length_tolerance &&
        aexcompat::pf_path_runtime::path_eval_seg_length_deriv1(effect_ref(), curve_path, &curve_prep, 0,
            curve_length * 0.5, &x, &y, &dx, &dy) == 0 &&
        std::abs(x - expected_mid_x) <= position_tolerance &&
        std::abs(y - expected_mid_y) <= position_tolerance &&
        (expect_zero_derivative ? std::hypot(dx, dy) == 0.0
                                : std::abs(std::hypot(dx, dy) - 1.0) <= 1e-10) &&
        aexcompat::pf_path_runtime::path_eval_seg_length(effect_ref(), curve_path, &curve_prep, 0,
            -1.0, &x, &y) != 0 &&
        aexcompat::pf_path_runtime::path_eval_seg_length(effect_ref(), curve_path, &curve_prep, 0,
            curve_length + 1.0, &x, &y) != 0;
    const bool cleaned =
        aexcompat::pf_path_runtime::path_cleanup_seg_length(effect_ref(), curve_path, 0, &curve_prep) == 0 &&
        curve_prep == nullptr && aexcompat::pf_path_runtime::checkin_path(effect_ref(), id, 0, curve_path) == 0;
    return evaluated && cleaned;
  };

  constexpr double kappa = 0.5522847498307936;
  if (!verify_curve(10, true,
          {{0, 0, 0, 0, 0, 0}, {4, 0, 0, 0, 0, 0}},
          4.0, 1e-10, 2.0, 0.0, 1e-8, false) ||
      !verify_curve(11, true,
          {{1, 0, 0, 0, 0, kappa}, {0, 1, kappa, 0, 0, 0}},
          1.5707963267948966, 3e-4, 0.7071067811865476,
          0.7071067811865476, 3e-5, false) ||
      !verify_curve(12, true,
          {{0, 0, 0, 0, 3, 6}, {6, 0, -3, -6, 0, 0}},
          9.537946844777451, 2e-3, 3.0, 0.0, 2e-4, false) ||
      !verify_curve(13, true,
          {{2, 3, 0, 0, 0, 0}, {2, 3, 0, 0, 0, 0}},
          0.0, 0.0, 2.0, 3.0, 0.0, true) ||
      !verify_curve(14, false,
          {{0, 0, 0, 0, 0, 0}, {1, 0, 0, 0, 0, 0}},
          1.0, 1e-10, 0.5, 0.0, 1e-8, false))
    return false;
  return aexcompat::pf_path_runtime::lifetimes_balanced();
}

}  // namespace aexcompat::l2_detail
