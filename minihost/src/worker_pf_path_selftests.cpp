#include "worker_pf_path_selftests.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_pf_path_runtime.hpp"
#include "worker_world_registry.hpp"
#include "worker_world_safety.hpp"

#include <cstddef>
#include <cstdint>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <initializer_list>
#include <limits>
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

  // Absent-path contract (issue #1253): PF_CheckoutPath of PF_PathID_NONE (0)
  // or of an id that names no path answers PF_Err_NONE with a NULL path (the
  // SDK's "can return NULL ptr if path doesn't exist"; Scribble aborts its
  // render on any other answer), and PF_CheckinPath of that NULL for the same
  // absent id answers PF_Err_NONE. Nothing that names host state loosens: a
  // NULL checkin for an id that does resolve, a NULL checkin marked changed,
  // a null out pointer, a non-positive step and a zero scale stay rejected.
  {
    const auto before = aexcompat::pf_path_runtime::snapshot();
    void* absent = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5a5a));
    if (aexcompat::pf_path_runtime::checkout_path(effect_ref(), 0, 0, 1, 1, &absent) != 0 ||
        absent != nullptr ||
        aexcompat::pf_path_runtime::checkin_path(effect_ref(), 0, 0, nullptr) != 0 ||
        (absent = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5a5a)), false) ||
        aexcompat::pf_path_runtime::checkout_path(effect_ref(), 99, 0, 1, 1, &absent) != 0 ||
        absent != nullptr ||
        aexcompat::pf_path_runtime::checkin_path(effect_ref(), 99, 0, nullptr) != 0 ||
        aexcompat::pf_path_runtime::checkin_path(effect_ref(), 1, 0, nullptr) == 0 ||
        aexcompat::pf_path_runtime::checkin_path(effect_ref(), 99, 1, nullptr) == 0 ||
        aexcompat::pf_path_runtime::checkin_path(nullptr, 99, 0, nullptr) == 0 ||
        aexcompat::pf_path_runtime::checkout_path(effect_ref(), 0, 0, 1, 1, nullptr) == 0 ||
        aexcompat::pf_path_runtime::checkout_path(effect_ref(), 0, 0, 0, 1, &absent) == 0 ||
        aexcompat::pf_path_runtime::checkout_path(effect_ref(), 0, 0, 1, 0, &absent) == 0 ||
        aexcompat::pf_path_runtime::checkout_path(nullptr, 0, 0, 1, 1, &absent) == 0)
      return false;
    const auto after = aexcompat::pf_path_runtime::snapshot();
    if (after.absent_checkouts != before.absent_checkouts + 2 ||
        after.absent_checkins != before.absent_checkins + 2 ||
        after.checkout_calls != before.checkout_calls ||
        after.checkin_calls != before.checkin_calls ||
        after.invalid_operations != before.invalid_operations + 7 ||
        !aexcompat::pf_path_runtime::lifetimes_balanced())
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

bool verify_pf_mask_composition(aexcompat::pf_path_runtime::HostHooks path_hooks,
    aexcompat::mask_runtime::HostContext mask_hooks) {
  aexcompat::mask_runtime::configure_host_context(mask_hooks);
  aexcompat::pf_path_runtime::configure(path_hooks);
  aexcompat::pf_path_runtime::reset();

  constexpr int32_t kWidth = 8;
  constexpr int32_t kHeight = 8;

  const auto rect = [](double left, double top, double right, double bottom) {
    const CurveVertex a{left, top, 0, 0, 0, 0};
    const CurveVertex b{right, top, 0, 0, 0, 0};
    const CurveVertex c{right, bottom, 0, 0, 0, 0};
    const CurveVertex d{left, bottom, 0, 0, 0, 0};
    return std::vector<CurveVertex>{a, b, c, d, a};
  };
  const auto in_rect = [](double left, double top, double right, double bottom) {
    return [=](int32_t x, int32_t y) {
      return x + .5 > left && x + .5 < right && y + .5 > top && y + .5 < bottom;
    };
  };
  const auto in_inner = in_rect(2, 2, 6, 6);
  const auto in_left = in_rect(0, 0, 6, 6);
  const auto in_right = in_rect(2, 2, 8, 8);

  // Self-test-only seeding helper: the production synthetic-scene hook keeps
  // HostMask defaults (ADD, not inverted, 100% opacity), so the style fields
  // the composition engine reads are set directly on the installed scene.
  struct Style { int32_t order; int32_t mode; bool invert; double opacity; };
  const auto install_styled = [](std::vector<CurveSnapshot> curves,
                                 const std::vector<Style>& styles) {
    if (!install_scene(std::move(curves)) || g_mask_scene.size() != styles.size())
      return false;
    for (size_t index = 0; index < styles.size(); ++index) {
      g_mask_scene[index].dynamic_order = styles[index].order;
      g_mask_scene[index].mode = styles[index].mode;
      g_mask_scene[index].invert = styles[index].invert;
      g_mask_scene[index].opacity = styles[index].opacity;
    }
    return true;
  };
  const auto checkout_paths = [](std::initializer_list<int32_t> ids,
                                 std::vector<void*>& handles) {
    handles.clear();
    for (const int32_t id : ids) {
      void* handle = nullptr;
      if (aexcompat::pf_path_runtime::checkout_path(effect_ref(), id, 0, 1, 1,
                                                    &handle) != 0)
        return false;
      handles.push_back(handle);
    }
    return true;
  };
  const auto checkin_paths = [](std::initializer_list<int32_t> ids,
                                const std::vector<void*>& handles) {
    size_t index = 0;
    for (const int32_t id : ids)
      if (aexcompat::pf_path_runtime::checkin_path(effect_ref(), id, 0,
                                                   handles[index++]) != 0)
        return false;
    return true;
  };
  const auto expect_argb8 = [&](const auto& expected) {
    std::vector<std::byte> pixels(kWidth * kHeight * 4, std::byte{0});
    for (int32_t p = 0; p < kWidth * kHeight; ++p) pixels[p * 4] = std::byte{0xff};
    aexcompat::world_safety::LocalEffectWorld world{};
    world.world_flags = 2;
    world.data = pixels.data();
    world.rowbytes = kWidth * 4;
    world.width = kWidth;
    world.height = kHeight;
    world.extent_hint = {0, 0, kWidth, kHeight};
    aexcompat::world_safety::DispatchWorldFormatScope scope;
    aexcompat::pf_path_runtime::LegacyRect bounds{0, 0, kWidth, kHeight};
    if (!scope.register_world(&world, aexcompat::world_registry::kPixelFormatArgb32) ||
        aexcompat::pf_path_runtime::mask_world_with_scene(
            effect_ref(), 0, 0, 0, &world, &bounds) != 0)
      return false;
    for (int32_t y = 0; y < kHeight; ++y)
      for (int32_t x = 0; x < kWidth; ++x)
        if (std::to_integer<int>(pixels[(y * kWidth + x) * 4]) != expected(x, y))
          return false;
    return true;
  };
  const auto expect_rejected_unchanged = [&] {
    std::vector<std::byte> pixels(kWidth * kHeight * 4, std::byte{0x5a});
    aexcompat::world_safety::LocalEffectWorld world{};
    world.world_flags = 2;
    world.data = pixels.data();
    world.rowbytes = kWidth * 4;
    world.width = kWidth;
    world.height = kHeight;
    world.extent_hint = {0, 0, kWidth, kHeight};
    aexcompat::world_safety::DispatchWorldFormatScope scope;
    aexcompat::pf_path_runtime::LegacyRect bounds{0, 0, kWidth, kHeight};
    if (!scope.register_world(&world, aexcompat::world_registry::kPixelFormatArgb32) ||
        aexcompat::pf_path_runtime::mask_world_with_scene(
            effect_ref(), 0, 0, 0, &world, &bounds) == 0)
      return false;
    return std::all_of(pixels.begin(), pixels.end(),
                       [](std::byte value) { return value == std::byte{0x5a}; });
  };
  const auto run_scene = [&](std::vector<CurveSnapshot> curves,
                             const std::vector<Style>& styles,
                             std::initializer_list<int32_t> ids,
                             const auto& expected) {
    std::vector<void*> handles;
    return install_styled(std::move(curves), styles) &&
        checkout_paths(ids, handles) && expect_argb8(expected) &&
        checkin_paths(ids, handles);
  };

  // Two overlapping ADD masks at 50% opacity combine with max: the overlap
  // must keep either mask's own coverage instead of accumulating.
  if (!run_scene({{1, false, rect(0, 0, 6, 6)}, {2, false, rect(2, 2, 8, 8)}},
                 {{0, 1, false, 50}, {1, 1, false, 50}}, {1, 2},
                 [&](int32_t x, int32_t y) {
                   return in_left(x, y) || in_right(x, y) ? 128 : 0;
                 }) ||
      // ADD then SUBTRACT punches a hole through the first mask.
      !run_scene({{1, false, rect(0, 0, 8, 8)}, {2, false, rect(2, 2, 6, 6)}},
                 {{0, 1, false, 100}, {1, 2, false, 100}}, {1, 2},
                 [&](int32_t x, int32_t y) { return in_inner(x, y) ? 0 : 255; }) ||
      // ADD then INTERSECT clips the result to the second mask.
      !run_scene({{1, false, rect(0, 0, 8, 8)}, {2, false, rect(2, 2, 6, 6)}},
                 {{0, 1, false, 100}, {1, 3, false, 100}}, {1, 2},
                 [&](int32_t x, int32_t y) { return in_inner(x, y) ? 255 : 0; }) ||
      // A SUBTRACT-only scene starts from the base=1 documented policy.
      !run_scene({{1, false, rect(2, 2, 6, 6)}}, {{0, 2, false, 100}}, {1},
                 [&](int32_t x, int32_t y) { return in_inner(x, y) ? 0 : 255; }) ||
      // An inverted ADD mask keeps everything except its own shape.
      !run_scene({{1, false, rect(2, 2, 6, 6)}}, {{0, 1, true, 100}}, {1},
                 [&](int32_t x, int32_t y) { return in_inner(x, y) ? 0 : 255; }) ||
      // Opacity scales the mask coverage before combination.
      !run_scene({{1, false, rect(2, 2, 6, 6)}}, {{0, 1, false, 50}}, {1},
                 [&](int32_t x, int32_t y) { return in_inner(x, y) ? 128 : 0; }) ||
      // NONE masks do nothing: no coverage contribution and no checkout
      // required, and a NONE-only scene leaves the base=1 layer intact.
      !run_scene({{1, false, rect(2, 2, 6, 6)}, {2, false, rect(0, 0, 8, 8)}},
                 {{0, 1, false, 100}, {1, 0, false, 100}}, {1},
                 [&](int32_t x, int32_t y) { return in_inner(x, y) ? 255 : 0; }) ||
      !run_scene({{1, false, rect(0, 0, 8, 8)}}, {{0, 0, false, 100}}, {},
                 [&](int32_t, int32_t) { return 255; }))
    return false;

  // Unobserved modes (LIGHTEN/DARKEN/DIFFERENCE/ACCUM) and out-of-range
  // values fail closed and leave the world unchanged.
  for (const int32_t mode : {4, 7, 9})
    if (!install_styled({{1, false, rect(2, 2, 6, 6)}}, {{0, mode, false, 100}}) ||
        !expect_rejected_unchanged())
      return false;
  std::vector<void*> handles;
  // An open path is not a mask and is rejected; the checkout still balances.
  if (!install_styled({{1, true, {{0, 0, 0, 0, 0, 0}, {8, 0, 0, 0, 0, 0},
                                  {8, 8, 0, 0, 0, 0}}}},
                      {{0, 1, false, 100}}) ||
      !checkout_paths({1}, handles) || !expect_rejected_unchanged() ||
      !checkin_paths({1}, handles))
    return false;
  // Vertex counts above the flatten limit are rejected.
  std::vector<CurveVertex> ring;
  for (int i = 0; i < 65; ++i) {
    const double angle = i * 6.283185307179586 / 65;
    ring.push_back({4 + 3 * std::cos(angle), 4 + 3 * std::sin(angle), 0, 0, 0, 0});
  }
  ring.push_back(ring.front());
  if (!install_styled({{1, false, std::move(ring)}}, {{0, 1, false, 100}}) ||
      !checkout_paths({1}, handles) || !expect_rejected_unchanged() ||
      !checkin_paths({1}, handles))
    return false;
  // A participating mask that was never checked out is rejected.
  if (!install_styled({{1, false, rect(2, 2, 6, 6)}}, {{0, 1, false, 100}}) ||
      !expect_rejected_unchanged())
    return false;

  // The composed coverage multiplies the first channel in the 16-bit and
  // 32-bit-float depths as well; a half-coverage full-frame ADD mask is the
  // depth oracle.
  const auto expect_half_depth = [&](int32_t pixel_format, int32_t pixel_bytes) {
    if (!install_styled({{1, false, rect(0, 0, 8, 8)}}, {{0, 1, false, 50}}) ||
        !checkout_paths({1}, handles))
      return false;
    const int32_t rowbytes = kWidth * pixel_bytes;
    std::vector<std::byte> pixels(static_cast<size_t>(rowbytes) * kHeight,
                                  std::byte{0});
    for (int32_t p = 0; p < kWidth * kHeight; ++p) {
      auto* pixel = pixels.data() + static_cast<size_t>(p) * pixel_bytes;
      if (pixel_format == aexcompat::world_registry::kPixelFormatArgb64) {
        const uint16_t value = 0xffff;
        std::memcpy(pixel, &value, sizeof(value));
      } else {
        const float value = 1.0f;
        std::memcpy(pixel, &value, sizeof(value));
      }
    }
    aexcompat::world_safety::LocalEffectWorld world{};
    world.world_flags = 2 | 1;
    world.data = pixels.data();
    world.rowbytes = rowbytes;
    world.width = kWidth;
    world.height = kHeight;
    world.extent_hint = {0, 0, kWidth, kHeight};
    aexcompat::world_safety::DispatchWorldFormatScope scope;
    aexcompat::pf_path_runtime::LegacyRect bounds{0, 0, kWidth, kHeight};
    if (!scope.register_world(&world, pixel_format) ||
        aexcompat::pf_path_runtime::mask_world_with_scene(
            effect_ref(), 0, 0, 0, &world, &bounds) != 0 ||
        !checkin_paths({1}, handles))
      return false;
    for (int32_t p = 0; p < kWidth * kHeight; ++p) {
      const auto* pixel = pixels.data() + static_cast<size_t>(p) * pixel_bytes;
      if (pixel_format == aexcompat::world_registry::kPixelFormatArgb64) {
        uint16_t value{};
        std::memcpy(&value, pixel, sizeof(value));
        if (value != 32768) return false;
      } else {
        float value{};
        std::memcpy(&value, pixel, sizeof(value));
        if (std::abs(value - .5f) > 1e-6f) return false;
      }
    }
    return true;
  };
  if (!expect_half_depth(aexcompat::world_registry::kPixelFormatArgb64, 8) ||
      !expect_half_depth(aexcompat::world_registry::kPixelFormatArgb128, 16))
    return false;
  // A non-finite 32F pixel anywhere in the area fails closed before writing.
  {
    if (!install_styled({{1, false, rect(0, 0, 8, 8)}}, {{0, 1, false, 100}}) ||
        !checkout_paths({1}, handles))
      return false;
    constexpr int32_t pixel_bytes = 16;
    std::vector<std::byte> pixels(kWidth * kHeight * pixel_bytes, std::byte{0});
    for (int32_t p = 0; p < kWidth * kHeight; ++p) {
      const float value = 1.0f;
      std::memcpy(pixels.data() + static_cast<size_t>(p) * pixel_bytes, &value,
                  sizeof(value));
    }
    const float not_a_number = std::numeric_limits<float>::quiet_NaN();
    std::memcpy(pixels.data(), &not_a_number, sizeof(not_a_number));
    const auto original = pixels;
    aexcompat::world_safety::LocalEffectWorld world{};
    world.world_flags = 2 | 1;
    world.data = pixels.data();
    world.rowbytes = kWidth * pixel_bytes;
    world.width = kWidth;
    world.height = kHeight;
    world.extent_hint = {0, 0, kWidth, kHeight};
    aexcompat::world_safety::DispatchWorldFormatScope scope;
    aexcompat::pf_path_runtime::LegacyRect bounds{0, 0, kWidth, kHeight};
    if (!scope.register_world(&world, aexcompat::world_registry::kPixelFormatArgb128) ||
        aexcompat::pf_path_runtime::mask_world_with_scene(
            effect_ref(), 0, 0, 0, &world, &bounds) == 0 ||
        !checkin_paths({1}, handles) || pixels != original)
      return false;
  }
  return aexcompat::pf_path_runtime::lifetimes_balanced();
}

}  // namespace aexcompat::l2_detail
