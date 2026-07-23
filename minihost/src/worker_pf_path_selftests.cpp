#include "worker_pf_path_selftests.hpp"
#include "worker_pf_path_runtime.hpp"
#include "worker_world_registry.hpp"
#include "worker_world_safety.hpp"

#include <cstddef>
#include <cmath>
#include <cstring>
#include <utility>
#include <vector>

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
  if (!install_scene({{20, false, {{0, 0, 0, 0, 0, 0},
                                  {4, 0, 0, 0, 0, 0},
                                  {4, 4, 0, 0, 0, 0},
                                  {0, 0, 0, 0, 0, 0}}}}))
    return false;
  void* render_path = nullptr;
  if (aexcompat::pf_path_runtime::checkout_path(effect_ref(), 20, 0, 1, 1,
                                                  &render_path) != 0)
    return false;
  const auto checkin_render_path = [&] {
    return aexcompat::pf_path_runtime::checkin_path(effect_ref(), 20, 0,
                                                     render_path) == 0;
  };
  const auto render_format = [&](int32_t pixel_format, int32_t pixel_bytes) {
    constexpr int32_t width = 8;
    constexpr int32_t height = 8;
    const int32_t rowbytes = width * pixel_bytes + pixel_bytes;
    std::vector<std::byte> pixels(static_cast<std::size_t>(rowbytes) * height,
                                  std::byte{0xcd});
    const auto set_channel = [&](int32_t x, int32_t y) {
      auto* pixel = pixels.data() + static_cast<std::size_t>(y) * rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      if (pixel_format == aexcompat::world_registry::kPixelFormatArgb32) {
        pixel[0] = std::byte{0xff};
      } else if (pixel_format == aexcompat::world_registry::kPixelFormatArgb64) {
        const uint16_t value = 0xffff;
        std::memcpy(pixel, &value, sizeof(value));
      } else {
        const float value = 1.0f;
        std::memcpy(pixel, &value, sizeof(value));
      }
    };
    set_channel(1, 1);
    aexcompat::world_safety::LocalEffectWorld world{};
    world.world_flags = 2 | (pixel_format == aexcompat::world_registry::kPixelFormatArgb32 ? 0 : 1);
    world.data = pixels.data();
    world.rowbytes = rowbytes;
    world.width = width;
    world.height = height;
    world.extent_hint = {0, 0, width, height};
    aexcompat::world_safety::DispatchWorldFormatScope scope;
    if (!scope.register_world(&world, pixel_format)) return false;
    aexcompat::pf_path_runtime::LegacyRect bounds{0, 0, width, height};
    if (aexcompat::pf_path_runtime::mask_world_with_path(
            effect_ref(), &render_path, 0, 0, 0, .5, 0, &world, &bounds) != 0)
      return false;
    const auto* pixel = pixels.data() + rowbytes + pixel_bytes;
    if (pixel_format == aexcompat::world_registry::kPixelFormatArgb32) {
      if (pixel[0] != std::byte{0x80}) return false;
    } else if (pixel_format == aexcompat::world_registry::kPixelFormatArgb64) {
      uint16_t value{};
      std::memcpy(&value, pixel, sizeof(value));
      if (value != 32768) return false;
    } else {
      float value{};
      std::memcpy(&value, pixel, sizeof(value));
      if (std::abs(value - .5f) > 1e-6f) return false;
    }
    const auto* padding = pixels.data() + rowbytes - pixel_bytes;
    for (int32_t index = 0; index < pixel_bytes; ++index)
      if (padding[index] != std::byte{0xcd}) return false;
    return true;
  };
  if (!render_format(aexcompat::world_registry::kPixelFormatArgb32, 4) ||
      !render_format(aexcompat::world_registry::kPixelFormatArgb64, 8) ||
      !render_format(aexcompat::world_registry::kPixelFormatArgb128, 16)) {
    checkin_render_path();
    return false;
  }
  {
    constexpr int32_t width = 8;
    constexpr int32_t height = 8;
    std::vector<std::byte> pixels(width * height * 4, std::byte{0xcd});
    aexcompat::world_safety::LocalEffectWorld world{};
    world.world_flags = 2;
    world.data = pixels.data();
    world.rowbytes = width * 4 - 1;
    world.width = width;
    world.height = height;
    aexcompat::world_safety::DispatchWorldFormatScope scope;
    if (!scope.register_world(&world, aexcompat::world_registry::kPixelFormatArgb32) ||
        aexcompat::pf_path_runtime::mask_world_with_path(
            effect_ref(), &render_path, 0, 0, 0, .5, 0, &world, nullptr) == 0) {
      checkin_render_path();
      return false;
    }
  }
  if (!checkin_render_path()) return false;
  return aexcompat::pf_path_runtime::lifetimes_balanced();
}

}  // namespace aexcompat::l2_detail
