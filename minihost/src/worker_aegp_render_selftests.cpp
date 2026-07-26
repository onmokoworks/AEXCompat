#include "worker_aegp_render_selftests.hpp"
#include "worker_aegp_render_options.hpp"
#include "worker_aegp_layer_render_runtime.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_aegp_staged_item_runtime.hpp"
#include "worker_aegp_item_render_runtime.hpp"
#include "worker_host_suite_catalog.hpp"
#include "worker_render_receipts.hpp"
#include "worker_world_registry.hpp"
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>
#include <thread>
#include <vector>

namespace aexcompat::l2_detail {
using AegpRenderOptionsValue = aexcompat::render_options::ItemValue;
using aexcompat::suite_abi::AegpRect;
using aexcompat::suite_abi::AegpTime;
using aexcompat::render_receipts::ReceiptDraft;
using aexcompat::render_receipts::ReceiptSnapshot;
using namespace aexcompat::render_options;
using namespace aexcompat::world_registry;
constexpr std::size_t kMaxRenderOptions = 32;
constexpr int32_t kSyntheticCompWidth = 17;
constexpr int32_t kSyntheticCompHeight = 9;
namespace {
auto& g_synthetic_receipt_test_mode =
    aexcompat::render_receipts::receipt_test_state().synthetic_test_mode;
auto& g_async_manager = aexcompat::render_receipts::receipt_test_state().async_manager;
}  // namespace
// Definitions of the probe fixtures declared in the header: this TU owns
// them because its render probes are the only writers.
std::array<uint8_t, 4> g_render_options_baseline8{};
std::array<uint8_t, 4> g_render_options_time8{};
std::array<uint8_t, 4> g_render_options_downsample8{};
std::array<uint8_t, 4> g_render_options_roi_outside8{};
std::array<uint8_t, 4> g_render_options_roi_inside8{};
std::array<uint8_t, 4> g_render_options_field_excluded8{};
std::array<uint8_t, 4> g_render_options_matte8{};
std::array<uint16_t, 4> g_render_options_argb16{};
std::array<float, 4> g_render_options_argb32f{};
bool render_options_lifetimes_balanced();
bool async_receipt_lifetimes_balanced();
void* aegp_comp_item_handle();
bool is_render_worker();
int32_t __cdecl checkout_item_frame_async(void*, uint32_t, void*, void**);
int32_t get_receipt_world(void*, void***);
int32_t checkin_frame(void*);
int32_t __cdecl render_get_region_reject(void*, void*);
using AegpRenderCancelV1 = int32_t(__cdecl*)(void*, uint8_t*);
int32_t __cdecl render_checkout_frame_reject(void*, AegpRenderCancelV1, void*, void**);
void clear_staged_item_worlds_for_test();
bool verify_item_render_cycle_contract(void*);
bool prepare_scene_staged_item(void*);

void write_staged_test_channel(std::byte* pixel, int32_t pixel_bytes,
                               int channel, float value) {
  value = std::clamp(value, 0.0f, 1.0f);
  if (pixel_bytes == 4)
    reinterpret_cast<uint8_t*>(pixel)[channel] =
        static_cast<uint8_t>(std::lround(value * 255.0f));
  else if (pixel_bytes == 8)
    reinterpret_cast<uint16_t*>(pixel)[channel] =
        static_cast<uint16_t>(std::lround(value * 32768.0f));
  else
    std::memcpy(pixel + channel * sizeof(float), &value, sizeof(value));
}

bool verify_aegp_render_options_suite1() {
  struct SyntheticReceiptScope {
    SyntheticReceiptScope() { g_synthetic_receipt_test_mode = true; }
    ~SyntheticReceiptScope() { g_synthetic_receipt_test_mode = false; }
  } synthetic_receipt_scope;
  if (!render_options_lifetimes_balanced() || !async_receipt_lifetimes_balanced()) return false;
  const auto render_probe = [](AegpRenderOptionsValue options, int32_t type,
                               int32_t sample_x, int32_t sample_y, void* output) {
    ReceiptDraft probe{};
    probe.render_options = options;
    probe.rendered_region = options.roi.right == 0
        ? AegpRect{0, 0, kSyntheticCompWidth, kSyntheticCompHeight} : options.roi;
    const int32_t width = (kSyntheticCompWidth + options.downsample_x - 1) /
        options.downsample_x;
    const int32_t height = (kSyntheticCompHeight + options.downsample_y - 1) /
        options.downsample_y;
    const int32_t pixel_bytes = type == 1 ? 4 : (type == 2 ? 8 : 16);
    probe.pixels.resize(static_cast<std::size_t>(width) * height * pixel_bytes);
    aexcompat::aegp_item_render_runtime::populate_synthetic_pixels(
        probe, type, width, height);
    std::memcpy(output, probe.pixels.data() +
        (static_cast<std::size_t>(sample_y) * width + sample_x) * pixel_bytes, pixel_bytes);
  };
  AegpRenderOptionsValue pixel_options{};
  pixel_options.time = {7, 30};
  render_probe(pixel_options, 1, 0, 0, g_render_options_baseline8.data());
  pixel_options.time = {1, 32};
  render_probe(pixel_options, 1, 0, 0, g_render_options_time8.data());
  pixel_options.time = {7, 30};
  pixel_options.downsample_x = 3; pixel_options.downsample_y = 2;
  render_probe(pixel_options, 1, 1, 1, g_render_options_downsample8.data());
  pixel_options = AegpRenderOptionsValue{}; pixel_options.time = {7, 30};
  pixel_options.roi = {1, 0, 17, 9};
  render_probe(pixel_options, 1, 0, 0, g_render_options_roi_outside8.data());
  render_probe(pixel_options, 1, 1, 0, g_render_options_roi_inside8.data());
  pixel_options.roi = {}; pixel_options.field = 1;
  render_probe(pixel_options, 1, 0, 1, g_render_options_field_excluded8.data());
  pixel_options.field = 0; pixel_options.matte = 1;
  render_probe(pixel_options, 1, 0, 0, g_render_options_matte8.data());
  pixel_options.matte = 0;
  render_probe(pixel_options, 2, 0, 0, g_render_options_argb16.data());
  render_probe(pixel_options, 3, 0, 0, g_render_options_argb32f.data());
  const std::array<uint8_t, 4> baseline{{123, 59, 177, 157}};
  const std::array<float, 4> expected_float{{123.0f / 255.0f, 59.0f / 255.0f,
                                             177.0f / 255.0f, 157.0f / 255.0f}};
  if (g_render_options_baseline8 != baseline ||
      g_render_options_time8 != std::array<uint8_t, 4>{{72, 8, 24, 56}} ||
      g_render_options_downsample8 != std::array<uint8_t, 4>{{170, 110, 235, 198}} ||
      g_render_options_roi_outside8 != std::array<uint8_t, 4>{} ||
      g_render_options_roi_inside8 != std::array<uint8_t, 4>{{134, 76, 177, 162}} ||
      g_render_options_field_excluded8 != std::array<uint8_t, 4>{} ||
      g_render_options_matte8 != std::array<uint8_t, 4>{{255, 59, 177, 157}} ||
      g_render_options_argb16 != std::array<uint16_t, 4>{{31611, 15163, 45489, 40349}} ||
      g_render_options_argb32f != expected_float) return false;
  void* options = nullptr;
  if (render_options_new_from_item(1, aegp_comp_item_handle(), &options) != 0 || !options) return false;
  AegpTime time{}, step{};
  int32_t field = -1, world_type = -1, matte = -1;
  int16_t dx = 0, dy = 0;
  int8_t channel_order = -1, render_quality = -1;
  uint8_t guide_layers = 2;
  AegpRect roi{-1, -1, -1, -1};
  const bool defaults = render_options_get_time(options, &time) == 0 &&
      time.value == 0 && time.scale == 1 &&
      render_options_get_time_step(options, &step) == 0 && step.value == 1 && step.scale == 30 &&
      render_options_get_field(options, &field) == 0 && field == 0 &&
      render_options_get_world_type(options, &world_type) == 0 && world_type == 1 &&
      render_options_get_downsample(options, &dx, &dy) == 0 && dx == 1 && dy == 1 &&
      render_options_get_roi(options, &roi) == 0 && roi.left == 0 && roi.top == 0 &&
      roi.right == 0 && roi.bottom == 0 &&
      render_options_get_matte(options, &matte) == 0 && matte == 0 &&
      render_options_get_channel_order(options, &channel_order) == 0 && channel_order == 0 &&
      render_options_get_guide_layers(options, &guide_layers) == 0 && guide_layers == 0 &&
      render_options_get_quality(options, &render_quality) == 0 && render_quality == 1;
  const AegpRect selected_roi{1, 2, 16, 8};
  if (!defaults || render_options_set_time(options, {-5, 24}) != 0 ||
      render_options_set_time_step(options, {2, 48}) != 0 ||
      render_options_set_field(options, 2) != 0 ||
      render_options_set_world_type(options, 2) != 0 ||
      render_options_set_downsample(options, 3, 2) != 0 ||
      render_options_set_roi(options, &selected_roi) != 0 ||
      render_options_set_matte(options, 1) != 0 ||
      render_options_set_channel_order(options, 1) != 0 ||
      render_options_set_guide_layers(options, 1) != 0 ||
      render_options_set_quality(options, 0) != 0) return false;
  if (render_options_get_time(options, &time) != 0 || time.value != -5 || time.scale != 24 ||
      render_options_get_time_step(options, &step) != 0 || step.value != 2 || step.scale != 48 ||
      render_options_get_field(options, &field) != 0 || field != 2 ||
      render_options_get_world_type(options, &world_type) != 0 || world_type != 2 ||
      render_options_get_downsample(options, &dx, &dy) != 0 || dx != 3 || dy != 2 ||
      render_options_get_roi(options, &roi) != 0 || roi.left != 1 || roi.bottom != 8 ||
      render_options_get_matte(options, &matte) != 0 || matte != 1 ||
      render_options_get_channel_order(options, &channel_order) != 0 || channel_order != 1 ||
      render_options_get_guide_layers(options, &guide_layers) != 0 || guide_layers != 1 ||
      render_options_get_quality(options, &render_quality) != 0 || render_quality != 0)
    return false;
  void* duplicate = nullptr;
  if (render_options_duplicate(1, options, &duplicate) != 0 || !duplicate ||
      render_options_set_time(duplicate, {7, 30}) != 0 ||
      render_options_get_time(options, &time) != 0 || time.value != -5 || time.scale != 24)
    return false;

  void* receipt = nullptr;
  if (checkout_item_frame_async(&g_async_manager, 1, options, &receipt) != 0 || !receipt ||
      render_options_dispose(options) != 0 || render_options_dispose(options) == 0) return false;
  void** world = nullptr;
  int32_t width = 0, height = 0, type = 0;
  AegpRect rendered{};
  ReceiptSnapshot receipt_snapshot{};
  const bool snapshot_ok =
      aexcompat::render_receipts::snapshot(receipt, receipt_snapshot) &&
      receipt_snapshot.has_render_options &&
      receipt_snapshot.render_options.time.value == -5 &&
      receipt_snapshot.render_options.time_step.value == 2 &&
      receipt_snapshot.render_options.field == 2 &&
      receipt_snapshot.render_options.matte == 1 &&
      receipt_snapshot.render_options.channel_order == 1 &&
      receipt_snapshot.render_options.render_guide_layers == 1 &&
      receipt_snapshot.render_options.render_quality == 0 &&
      receipt_snapshot.rendered_region.left == 1 &&
      receipt_snapshot.rendered_region.top == 1;
  if (!snapshot_ok || get_receipt_world(receipt, &world) != 0 ||
      aegp_world_get_type(world, &type) != 0 || type != 2 ||
      aegp_world_get_size(world, &width, &height) != 0 || width != 6 || height != 5 ||
      render_get_region_reject(receipt, &rendered) != 0 || rendered.right != 6 ||
      rendered.bottom != 4 ||
      checkin_frame(receipt) != 0) return false;

  for (int32_t depth = 1; depth <= 3; ++depth) {
    if (render_options_set_world_type(duplicate, depth) != 0 ||
        render_options_set_downsample(duplicate, static_cast<int16_t>(depth + 1), 4) != 0)
      return false;
    receipt = nullptr;
    if (render_checkout_frame_reject(duplicate, nullptr, nullptr, &receipt) != 0 || !receipt ||
        get_receipt_world(receipt, &world) != 0 || aegp_world_get_type(world, &type) != 0 ||
        type != depth || aegp_world_get_size(world, &width, &height) != 0 ||
        width != (kSyntheticCompWidth + depth) / (depth + 1) || height != 3 ||
        checkin_frame(receipt) != 0) return false;
  }
  if (render_options_set_matte(duplicate, 2) != 0) return false;
  receipt = reinterpret_cast<void*>(1);
  if (render_checkout_frame_reject(duplicate, nullptr, nullptr, &receipt) == 0 || receipt != nullptr ||
      render_options_set_time(duplicate, {1, 0}) == 0 ||
      render_options_set_time_step(duplicate, {0, 1}) == 0 ||
      render_options_set_field(duplicate, 3) == 0 ||
      render_options_set_world_type(duplicate, 0) == 0 ||
      render_options_set_downsample(duplicate, 0, 1) == 0 ||
      render_options_set_matte(duplicate, 3) == 0 ||
      render_options_set_channel_order(duplicate, 2) == 0 ||
      render_options_set_guide_layers(duplicate, 2) == 0 ||
      render_options_set_quality(duplicate, -1) == 0) return false;
  const AegpRect invalid_roi{2, 2, 1, 1};
  if (render_options_set_roi(duplicate, &invalid_roi) == 0) return false;
  if (render_options_dispose(duplicate) != 0 || render_options_get_time(duplicate, &time) == 0)
    return false;

  std::array<void*, kMaxRenderOptions> capacity_handles{};
  for (auto& handle : capacity_handles) {
    if (render_options_new_from_item(1, aegp_comp_item_handle(), &handle) != 0 || !handle)
      return false;
  }
  void* overflow = reinterpret_cast<void*>(1);
  if (render_options_new_from_item(1, aegp_comp_item_handle(), &overflow) == 0 ||
      overflow != nullptr) return false;
  const void* stale_capacity_handle = capacity_handles.front();
  for (void* handle : capacity_handles) {
    if (render_options_dispose(handle) != 0) return false;
  }
  void* replacement = nullptr;
  if (render_options_new_from_item(1, aegp_comp_item_handle(), &replacement) != 0 ||
      replacement == stale_capacity_handle || render_options_get_time(
          const_cast<void*>(stale_capacity_handle), &time) == 0 ||
      render_options_dispose(replacement) != 0) return false;

  void* invalid_output = reinterpret_cast<void*>(1);
  if (render_options_new_from_item(2, aegp_comp_item_handle(), &invalid_output) == 0 ||
      invalid_output != nullptr ||
      render_options_new_from_item(1, nullptr, &invalid_output) == 0 ||
      invalid_output != nullptr || render_options_duplicate(2, nullptr, &invalid_output) == 0 ||
      invalid_output != nullptr) return false;

  std::atomic<bool> concurrent_ok{true};
  std::array<std::thread, 3> threads;
  for (int index = 0; index < 3; ++index) {
    threads[index] = std::thread([index, &concurrent_ok] {
      void* local = nullptr;
      void* local_receipt = nullptr;
      void** local_world = nullptr;
      int32_t local_type = 0;
      if (render_options_new_from_item(1, aegp_comp_item_handle(), &local) != 0 ||
          render_options_set_world_type(local, index + 1) != 0 ||
          aexcompat::aegp_item_render_runtime::publish_receipt(
              local, &local_receipt) != 0 ||
          render_options_dispose(local) != 0 ||
          get_receipt_world(local_receipt, &local_world) != 0 ||
          aegp_world_get_type(local_world, &local_type) != 0 || local_type != index + 1 ||
          checkin_frame(local_receipt) != 0) concurrent_ok = false;
    });
  }
  for (auto& thread : threads) thread.join();
  return concurrent_ok && render_options_lifetimes_balanced() &&
      async_receipt_lifetimes_balanced();
}

bool verify_aegp_item_staged_worlds() {
  if (!render_options_lifetimes_balanced() || !async_receipt_lifetimes_balanced())
    return false;
  clear_staged_item_worlds_for_test();
  const AegpTime time{5, 24}, step{1, 24};
  constexpr int32_t width = 5, height = 4;
  auto fixture = [=](int32_t pixel_bytes) {
    std::vector<std::byte> pixels(width * height * pixel_bytes);
    for (int32_t y = 0; y < height; ++y) for (int32_t x = 0; x < width; ++x) {
      const std::array<float, 4> value{{(64 + x * 17 + y * 9) / 255.0f,
          (11 + x * 31) / 255.0f, (23 + y * 47) / 255.0f,
          (37 + x * 13 + y * 19) / 255.0f}};
      std::byte* pixel = pixels.data() + (static_cast<std::size_t>(y) * width + x) *
          pixel_bytes;
      for (int channel = 0; channel < 4; ++channel)
        write_staged_test_channel(pixel, pixel_bytes, channel, value[channel]);
    }
    return pixels;
  };
  const std::array<int32_t, 3> formats{{
      kPixelFormatArgb32, kPixelFormatArgb64, kPixelFormatArgb128}};
  if (!prepare_scene_staged_item(aegp_comp_item_handle()))
    return false;
  const uint64_t production_effect_identity =
      (static_cast<uint64_t>(g_aegp_effect_instances[0].generation) << 32) | 1;
  for (int32_t depth = 0; depth < 3; ++depth) {
    const int32_t pixel_bytes = 4 << depth;
    const auto pixels = fixture(pixel_bytes);
    for (const auto kind : {
             aexcompat::aegp_staged_item_runtime::StageKind::upstream,
             aexcompat::aegp_staged_item_runtime::StageKind::all_effects,
             aexcompat::aegp_staged_item_runtime::StageKind::downstream}) {
      if (!aexcompat::aegp_staged_item_runtime::publish_stage_world(
              aegp_comp_item_handle(), kind, production_effect_identity,
              time, step, 1, 0, formats[depth], width, height,
              width * pixel_bytes, pixels.data()))
        return false;
    }
    if (!aexcompat::aegp_staged_item_runtime::publish_world(aegp_comp_item_handle(), time, step, 1, 0,
            formats[depth], width, height, width * pixel_bytes, pixels.data())) return false;
  }
  void* options = nullptr;
  if (render_options_new_from_item(1, aegp_comp_item_handle(), &options) != 0 ||
      render_options_set_time(options, time) != 0 ||
      render_options_set_time_step(options, step) != 0) return false;
  auto checkout = [&](int32_t world_type, int16_t dx, int16_t dy, AegpRect roi,
                      int32_t field, int32_t matte, int8_t order,
                      void** out_receipt, void*** out_world) {
    return render_options_set_world_type(options, world_type) == 0 &&
        render_options_set_downsample(options, dx, dy) == 0 &&
        render_options_set_roi(options, &roi) == 0 &&
        render_options_set_field(options, field) == 0 &&
        render_options_set_matte(options, matte) == 0 &&
        render_options_set_channel_order(options, order) == 0 &&
        render_checkout_frame_reject(options, nullptr, nullptr, out_receipt) == 0 &&
        *out_receipt && get_receipt_world(*out_receipt, out_world) == 0 && *out_world;
  };
  void* rational_receipt = nullptr;
  void** rational_world = nullptr;
  if (render_options_set_time(options, {10, 48}) != 0 ||
      !checkout(1, 1, 1, {}, 0, 0, 0, &rational_receipt, &rational_world) ||
      checkin_frame(rational_receipt) != 0 || render_options_set_time(options, time) != 0)
    return false;
  for (int32_t depth = 1; depth <= 3; ++depth) {
    void* receipt = nullptr;
    void** world = nullptr;
    if (!checkout(depth, 1, 1, {}, 0, 0, 0, &receipt, &world)) return false;
    int32_t type = 0, actual_width = 0, actual_height = 0;
    void* pixels = nullptr;
    const int32_t base_error = depth == 1 ? aegp_world_get_base_addr8(world, &pixels) :
        (depth == 2 ? aegp_world_get_base_addr16(world, &pixels) :
                      aegp_world_get_base_addr32(world, &pixels));
    if (aegp_world_get_type(world, &type) != 0 || type != depth ||
        aegp_world_get_size(world, &actual_width, &actual_height) != 0 ||
        actual_width != width || actual_height != height || base_error != 0 || !pixels ||
        checkin_frame(receipt) != 0) return false;
  }
  const auto verify_pixel8 = [&](int32_t matte, int8_t order,
                                  std::array<uint8_t, 4> expected) {
    void* receipt = nullptr;
    void** world = nullptr;
    void* pixels = nullptr;
    const bool valid = checkout(1, 1, 1, {}, 0, matte, order, &receipt, &world) &&
        aegp_world_get_base_addr8(world, &pixels) == 0 && pixels &&
        std::memcmp(pixels, expected.data(), expected.size()) == 0;
    return receipt && checkin_frame(receipt) == 0 && valid;
  };
  if (!verify_pixel8(0, 0, {{64, 11, 23, 37}}) ||
      !verify_pixel8(0, 1, {{37, 23, 11, 64}}) ||
      !verify_pixel8(1, 0, {{64, 3, 6, 9}}) ||
      !verify_pixel8(2, 0, {{64, 39, 42, 45}})) return false;
  void* bgra16_receipt = nullptr;
  void** bgra16_world = nullptr;
  void* bgra16_pixels = nullptr;
  if (!checkout(2, 1, 1, {}, 0, 0, 1, &bgra16_receipt, &bgra16_world) ||
      aegp_world_get_base_addr16(bgra16_world, &bgra16_pixels) != 0 || !bgra16_pixels ||
      std::memcmp(bgra16_pixels,
          std::array<uint16_t, 4>{{4755, 2956, 1414, 8224}}.data(), 8) != 0 ||
      checkin_frame(bgra16_receipt) != 0) return false;
  void* bgra32_receipt = nullptr;
  void** bgra32_world = nullptr;
  void* bgra32_pixels = nullptr;
  if (!checkout(3, 1, 1, {}, 0, 0, 1, &bgra32_receipt, &bgra32_world) ||
      aegp_world_get_base_addr32(bgra32_world, &bgra32_pixels) != 0 || !bgra32_pixels)
    return false;
  const auto* bgra32 = static_cast<const float*>(bgra32_pixels);
  if (std::abs(bgra32[0] - 37.0f / 255.0f) > 1e-7f ||
      std::abs(bgra32[1] - 23.0f / 255.0f) > 1e-7f ||
      std::abs(bgra32[2] - 11.0f / 255.0f) > 1e-7f ||
      std::abs(bgra32[3] - 64.0f / 255.0f) > 1e-7f ||
      checkin_frame(bgra32_receipt) != 0) return false;
  const AegpRect roi{1, 1, 5, 4};
  void* first = nullptr;
  void* second = nullptr;
  void** first_world = nullptr;
  void** second_world = nullptr;
  if (!checkout(1, 2, 2, roi, 2, 2, 1, &first, &first_world) ||
      !checkout(1, 2, 2, roi, 2, 2, 1, &second, &second_world)) return false;
  AegpRect region{};
  int32_t transformed_width = 0, transformed_height = 0;
  uint8_t* transformed = nullptr;
  if (render_get_region_reject(first, &region) != 0 || region.left != 1 ||
      region.top != 1 || region.right != 3 || region.bottom != 2 ||
      aegp_world_get_size(first_world, &transformed_width, &transformed_height) != 0 ||
      transformed_width != 3 || transformed_height != 2 ||
      aegp_world_get_base_addr8(first_world,
          reinterpret_cast<void**>(&transformed)) != 0 || !transformed) return false;
  // (2,2) is inside lower-field request only when field=2 selects odd source rows;
  // this request therefore leaves the first sampled row transparent and renders row 2 false.
  const bool field_oracle = transformed[0] == 0 && transformed[1] == 0 &&
      transformed[2] == 0 && transformed[3] == 0;
  clear_staged_item_worlds_for_test();
  // Clearing the registry must not invalidate either live receipt's pinned source.
  int32_t pinned_type = 0;
  if (!field_oracle || aegp_world_get_type(second_world, &pinned_type) != 0 ||
      pinned_type != 1 || checkin_frame(second) != 0 || checkin_frame(first) != 0 ||
      checkin_frame(first) == 0 || aegp_world_get_type(first_world, &pinned_type) == 0)
    return false;

  const auto evictions_before = aexcompat::aegp_staged_item_runtime::diagnostics().evictions;
  auto pixels8 = fixture(4);
  for (std::uintptr_t index = 0; index < 40; ++index) {
    if (!aexcompat::aegp_staged_item_runtime::publish_world(
            reinterpret_cast<void*>(0x2000u + index * 16u), time, step, 1, 0,
            kPixelFormatArgb32, width, height, width * 4, pixels8.data())) return false;
  }
  if (aexcompat::aegp_staged_item_runtime::diagnostics().evictions <= evictions_before)
    return false;
  clear_staged_item_worlds_for_test();
  if (!aexcompat::aegp_staged_item_runtime::publish_world(aegp_comp_item_handle(), time, step, 1, 0,
          kPixelFormatArgb32, width, height, width * 4, pixels8.data())) return false;
  void* rejected = reinterpret_cast<void*>(1);
  if (render_options_set_quality(options, 0) != 0 ||
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) == 0 || rejected ||
      render_options_set_quality(options, 1) != 0 ||
      render_options_set_guide_layers(options, 1) != 0 ||
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) == 0 || rejected ||
      render_options_set_guide_layers(options, 0) != 0 ||
      render_options_set_time_step(options, {2, 24}) != 0 ||
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) == 0 || rejected)
    return false;
  if (render_options_set_time_step(options, step) != 0 ||
      render_options_set_time(options, {6, 24}) != 0 ||
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) == 0 || rejected ||
      render_options_set_time(options, time) != 0) return false;

  using aexcompat::aegp_staged_item_runtime::SamplingPolicy;
  using aexcompat::aegp_staged_item_runtime::StageKind;
  const auto publish_scheduler_stage =
      [&](void* item, StageKind kind, uint64_t effect, AegpTime at,
          uint8_t marker, uint64_t* identity_hash = nullptr) {
        auto pixels = pixels8;
        pixels[0] = static_cast<std::byte>(marker);
        return aexcompat::aegp_staged_item_runtime::publish_stage_world(
            item, kind, effect, at, step, 1, 0, kPixelFormatArgb32,
            width, height, width * 4, pixels.data(), identity_hash);
      };
  const auto make_request = [&](void* item, AegpTime at) {
    AegpRenderOptionsValue request{};
    request.item = item;
    request.time = at;
    request.time_step = step;
    request.world_type = 1;
    request.render_quality = 1;
    return request;
  };
  const auto same_test_time = [](AegpTime left, AegpTime right) {
    return left.scale != 0 && right.scale != 0 &&
        static_cast<int64_t>(left.value) * right.scale ==
            static_cast<int64_t>(right.value) * left.scale;
  };
  const auto checkout_registered =
      [&](const AegpRenderOptionsValue& request, ReceiptSnapshot& snapshot,
          uint8_t* first_channel = nullptr) {
        void* receipt = nullptr;
        if (aexcompat::aegp_staged_item_runtime::publish_registered_receipt(
                request, &receipt) != 0 ||
            !receipt || !aexcompat::render_receipts::snapshot(receipt, snapshot))
          return false;
        bool valid = true;
        if (first_channel) {
          void** world = nullptr;
          void* data = nullptr;
          valid = get_receipt_world(receipt, &world) == 0 && world &&
              aegp_world_get_base_addr8(world, &data) == 0 && data;
          if (valid) *first_channel = *static_cast<uint8_t*>(data);
        }
        return checkin_frame(receipt) == 0 && valid;
      };

  // Fixture 1: a three-item A -> B -> C dependency chain proves two nested
  // composition levels and post-order resolution without recursive effects.
  clear_staged_item_worlds_for_test();
  void* const nested_root = reinterpret_cast<void*>(0xa100);
  void* const nested_middle = reinterpret_cast<void*>(0xa200);
  void* const nested_leaf = reinterpret_cast<void*>(0xa300);
  std::array<void*, 1> root_dependency{{nested_middle}};
  std::array<void*, 1> middle_dependency{{nested_leaf}};
  if (!aexcompat::aegp_staged_item_runtime::register_item(
          nested_leaf, 1003, SamplingPolicy::exact, nullptr, 0, nullptr, 0) ||
      !aexcompat::aegp_staged_item_runtime::register_item(
          nested_middle, 1002, SamplingPolicy::exact,
          middle_dependency.data(), middle_dependency.size(), nullptr, 0) ||
      !aexcompat::aegp_staged_item_runtime::register_item(
          nested_root, 1001, SamplingPolicy::exact, root_dependency.data(),
          root_dependency.size(), nullptr, 0) ||
      !publish_scheduler_stage(nested_leaf, StageKind::final_item, 0, time, 31) ||
      !publish_scheduler_stage(nested_middle, StageKind::final_item, 0, time, 41) ||
      !publish_scheduler_stage(nested_root, StageKind::final_item, 0, time, 51))
    return false;
  ReceiptSnapshot nested_snapshot{};
  uint8_t nested_first_channel = 0;
  if (!checkout_registered(make_request(nested_root, time), nested_snapshot,
          &nested_first_channel) ||
      !nested_snapshot.has_stage_evidence ||
      nested_snapshot.item_identity != 1001 ||
      nested_snapshot.resolved_stage_count != 3 ||
      nested_snapshot.resolved_depth != 2 ||
      nested_snapshot.sampling_policy !=
          static_cast<uint8_t>(SamplingPolicy::exact) ||
      !same_test_time(nested_snapshot.requested_time, time) ||
      !same_test_time(nested_snapshot.source_time, time) ||
      nested_snapshot.trace_hash == 0 || nested_first_channel != 51)
    return false;
  auto nested_miss = make_request(nested_root, {6, 24});
  rejected = reinterpret_cast<void*>(1);
  if (aexcompat::aegp_staged_item_runtime::publish_registered_receipt(
          nested_miss, &rejected) == 0 || rejected)
    return false;

  // Fixture 2: independent hold/nearest items plus two effect instances.
  // A deliberately missing downstream stage must fail before receipt
  // publication; completing and republishing the ordered chain then succeeds.
  clear_staged_item_worlds_for_test();
  void* const hold_item = reinterpret_cast<void*>(0xb100);
  void* const nearest_item = reinterpret_cast<void*>(0xb200);
  void* const multiple_root = reinterpret_cast<void*>(0xb300);
  const std::array<uint64_t, 2> effects{{0xe101, 0xe202}};
  std::array<void*, 2> multiple_dependencies{{hold_item, nearest_item}};
  if (!aexcompat::aegp_staged_item_runtime::register_item(
          hold_item, 2001, SamplingPolicy::hold, nullptr, 0, nullptr, 0) ||
      !aexcompat::aegp_staged_item_runtime::register_item(
          nearest_item, 2002, SamplingPolicy::nearest, nullptr, 0,
          effects.data(), effects.size()) ||
      !aexcompat::aegp_staged_item_runtime::register_item(
          multiple_root, 2003, SamplingPolicy::exact,
          multiple_dependencies.data(), multiple_dependencies.size(),
          nullptr, 0))
    return false;
  const AegpTime before{4, 24};
  const AegpTime after{6, 24};
  const AegpTime negative_source{-2, 24};
  const AegpTime negative_request{-1, 24};
  uint64_t hold_before_hash = 0, hold_after_hash = 0;
  uint64_t nearest_before_hash = 0, nearest_after_hash = 0;
  uint64_t root_hash = 0;
  if (!publish_scheduler_stage(hold_item, StageKind::final_item, 0,
          negative_source, 12) ||
      !publish_scheduler_stage(hold_item, StageKind::final_item, 0, before,
          14, &hold_before_hash) ||
      !publish_scheduler_stage(hold_item, StageKind::final_item, 0, after,
          16, &hold_after_hash) ||
      !publish_scheduler_stage(nearest_item, StageKind::upstream, effects[0],
          before, 21) ||
      !publish_scheduler_stage(nearest_item, StageKind::all_effects, effects[0],
          before, 22) ||
      !publish_scheduler_stage(nearest_item, StageKind::final_item, 0, before,
          29, &nearest_before_hash))
    return false;
  const auto boundary_rejections_before =
      aexcompat::aegp_staged_item_runtime::diagnostics()
          .effect_boundary_rejections;
  rejected = reinterpret_cast<void*>(1);
  const auto nearest_request = make_request(nearest_item, time);
  if (aexcompat::aegp_staged_item_runtime::publish_registered_receipt(
          nearest_request, &rejected) == 0 || rejected ||
      aexcompat::aegp_staged_item_runtime::diagnostics()
              .effect_boundary_rejections <= boundary_rejections_before)
    return false;
  std::array<uint64_t, 6> boundary_hashes{};
  std::size_t boundary_index = 0;
  if (!publish_scheduler_stage(nearest_item, StageKind::downstream, effects[0],
          before, 23, &boundary_hashes[boundary_index++]))
    return false;
  for (StageKind kind : {StageKind::upstream, StageKind::all_effects,
                         StageKind::downstream}) {
    if (!publish_scheduler_stage(nearest_item, kind, effects[1], before,
            static_cast<uint8_t>(24 + boundary_index),
            &boundary_hashes[boundary_index++]))
      return false;
  }
  // Republish the first two boundaries so all six identities have explicit
  // evidence and a strictly ordered generation sequence.
  if (!publish_scheduler_stage(nearest_item, StageKind::upstream, effects[0],
          before, 21, &boundary_hashes[boundary_index++]) ||
      !publish_scheduler_stage(nearest_item, StageKind::all_effects, effects[0],
          before, 22, &boundary_hashes[boundary_index++]))
    return false;
  // The required order is effect[0] upstream/all/downstream followed by
  // effect[1]. Republish the complete chain in that order after the injected
  // failure so generation order is deterministic.
  boundary_index = 0;
  for (uint64_t effect : effects) {
    for (StageKind kind : {StageKind::upstream, StageKind::all_effects,
                           StageKind::downstream}) {
      if (!publish_scheduler_stage(nearest_item, kind, effect, before,
              static_cast<uint8_t>(31 + boundary_index),
              &boundary_hashes[boundary_index]))
        return false;
      ++boundary_index;
    }
  }
  if (!publish_scheduler_stage(nearest_item, StageKind::final_item, 0, before,
          39, &nearest_before_hash))
    return false;
  for (uint64_t effect : effects)
    for (StageKind kind : {StageKind::upstream, StageKind::all_effects,
                           StageKind::downstream})
      if (!publish_scheduler_stage(nearest_item, kind, effect, after,
              static_cast<uint8_t>(41 + static_cast<uint8_t>(kind)),
              nullptr))
        return false;
  if (!publish_scheduler_stage(nearest_item, StageKind::final_item, 0, after,
          49, &nearest_after_hash) ||
      !publish_scheduler_stage(multiple_root, StageKind::final_item, 0, time,
          59, &root_hash))
    return false;
  for (std::size_t left = 0; left < boundary_hashes.size(); ++left) {
    if (boundary_hashes[left] == 0) return false;
    for (std::size_t right = left + 1; right < boundary_hashes.size(); ++right)
      if (boundary_hashes[left] == boundary_hashes[right]) return false;
  }
  if (!hold_before_hash || !hold_after_hash || !nearest_before_hash ||
      !nearest_after_hash || !root_hash ||
      hold_before_hash == nearest_before_hash ||
      nearest_before_hash == root_hash || hold_before_hash == root_hash)
    return false;
  ReceiptSnapshot hold_snapshot{}, negative_hold_snapshot{}, nearest_snapshot{},
      multiple_snapshot{};
  const bool hold_checkout =
      checkout_registered(make_request(hold_item, time), hold_snapshot);
  const bool negative_hold_checkout = checkout_registered(
      make_request(hold_item, negative_request), negative_hold_snapshot);
  const bool nearest_checkout =
      checkout_registered(nearest_request, nearest_snapshot);
  const bool sampling_evidence_valid = hold_checkout &&
      same_test_time(hold_snapshot.source_time, before) &&
      hold_snapshot.sampling_policy ==
          static_cast<uint8_t>(SamplingPolicy::hold) &&
      negative_hold_checkout &&
      same_test_time(negative_hold_snapshot.source_time, negative_source) &&
      negative_hold_snapshot.sampling_policy ==
          static_cast<uint8_t>(SamplingPolicy::hold) &&
      nearest_checkout &&
      same_test_time(nearest_snapshot.source_time, before) &&
      nearest_snapshot.sampling_policy ==
          static_cast<uint8_t>(SamplingPolicy::nearest) &&
      nearest_snapshot.resolved_stage_count == 7;
  if (!sampling_evidence_valid) return false;
  if (
      !checkout_registered(make_request(multiple_root, time),
          multiple_snapshot) ||
      multiple_snapshot.item_identity != 2003 ||
      multiple_snapshot.resolved_stage_count != 9 ||
      multiple_snapshot.resolved_depth != 1 ||
      multiple_snapshot.trace_hash == nearest_snapshot.trace_hash)
    return false;

  constexpr int32_t mfr_width = 1024;
  constexpr int32_t mfr_height = 1024;
  std::vector<std::byte> mfr_pixels(
      static_cast<std::size_t>(mfr_width) * mfr_height * 4);
  for (std::size_t offset = 0; offset < mfr_pixels.size(); offset += 4) {
    mfr_pixels[offset] = std::byte{255};
    mfr_pixels[offset + 1] = std::byte{37};
    mfr_pixels[offset + 2] = std::byte{73};
    mfr_pixels[offset + 3] = std::byte{109};
  }
  if (!aexcompat::aegp_staged_item_runtime::publish_stage_world(
          multiple_root, StageKind::final_item, 0, time, step, 1, 0,
          kPixelFormatArgb32, mfr_width, mfr_height, mfr_width * 4,
          mfr_pixels.data()))
    return false;
  ReceiptSnapshot mfr_baseline{};
  const auto mfr_request = make_request(multiple_root, time);
  if (!checkout_registered(mfr_request, mfr_baseline) ||
      mfr_baseline.resolved_stage_count != 9 ||
      mfr_baseline.trace_hash == multiple_snapshot.trace_hash)
    return false;
  std::atomic<bool> mfr_ok{true};
  std::atomic<uint32_t> mfr_ready{};
  std::atomic<bool> mfr_start{};
  std::array<std::thread, 8> mfr_threads;
  for (auto& thread : mfr_threads) {
    thread = std::thread([&] {
      ++mfr_ready;
      while (!mfr_start.load()) std::this_thread::yield();
      ReceiptSnapshot snapshot{};
      if (!checkout_registered(mfr_request, snapshot) ||
          snapshot.trace_hash != mfr_baseline.trace_hash)
        mfr_ok = false;
    });
  }
  while (mfr_ready.load() != mfr_threads.size()) std::this_thread::yield();
  mfr_start = true;
  for (auto& thread : mfr_threads) thread.join();
  if (!mfr_ok || aexcompat::aegp_staged_item_runtime::diagnostics().in_flight != 0 ||
      aexcompat::aegp_staged_item_runtime::diagnostics().max_in_flight < 2 ||
      !async_receipt_lifetimes_balanced())
    return false;

  // Regression F4: concurrent republish and checkout must observe only
  // complete scheduler snapshots; publication order is intentionally shuffled.
  clear_staged_item_worlds_for_test();
  void* const regress_f4_item = reinterpret_cast<void*>(0xa400);
  std::array<uint64_t, 1> regress_f4_effect{{0xe401}};
  if (!aexcompat::aegp_staged_item_runtime::register_item(
          regress_f4_item, 6001, SamplingPolicy::exact, nullptr, 0,
          regress_f4_effect.data(), regress_f4_effect.size()) ||
      !publish_scheduler_stage(regress_f4_item, StageKind::downstream,
          regress_f4_effect[0], time, 24) ||
      !publish_scheduler_stage(regress_f4_item, StageKind::upstream,
          regress_f4_effect[0], time, 21) ||
      !publish_scheduler_stage(regress_f4_item, StageKind::all_effects,
          regress_f4_effect[0], time, 22) ||
      !publish_scheduler_stage(regress_f4_item, StageKind::final_item, 0,
          time, 51))
    return false;
  std::atomic<bool> f4_ok{true};
  std::atomic<uint32_t> f4_ready{};
  std::atomic<bool> f4_start{};
  auto f4_publish = [&](StageKind kind, uint8_t marker) {
    return publish_scheduler_stage(regress_f4_item, kind,
        kind == StageKind::final_item ? 0 : regress_f4_effect[0], time, marker);
  };
  std::thread f4_publisher([&] {
    ++f4_ready;
    while (!f4_start.load()) std::this_thread::yield();
    for (uint8_t iteration = 0; iteration < 32; ++iteration) {
      if (!f4_publish(StageKind::downstream, static_cast<uint8_t>(61 + iteration)) ||
          !f4_publish(StageKind::upstream, static_cast<uint8_t>(81 + iteration)) ||
          !f4_publish(StageKind::all_effects, static_cast<uint8_t>(101 + iteration)) ||
          !f4_publish(StageKind::final_item, static_cast<uint8_t>(121 + iteration))) {
        f4_ok = false;
        return;
      }
    }
  });
  std::array<std::thread, 4> f4_checkouts;
  for (auto& thread : f4_checkouts) {
    thread = std::thread([&] {
      ++f4_ready;
      while (!f4_start.load()) std::this_thread::yield();
      for (uint8_t iteration = 0; iteration < 32; ++iteration) {
        ReceiptSnapshot snapshot{};
        if (!checkout_registered(make_request(regress_f4_item, time), snapshot) ||
            snapshot.item_identity != 6001 || snapshot.resolved_stage_count != 4 ||
            !same_test_time(snapshot.source_time, time)) {
          f4_ok = false;
          return;
        }
      }
    });
  }
  while (f4_ready.load() != f4_checkouts.size() + 1) std::this_thread::yield();
  f4_start = true;
  f4_publisher.join();
  for (auto& thread : f4_checkouts) thread.join();
  if (!f4_ok || aexcompat::aegp_staged_item_runtime::diagnostics().in_flight != 0)
    return false;

  // Regression F3: every boundary for every effect must share the final
  // hold-selected source time; a mixed-time plan is rejected before output.
  clear_staged_item_worlds_for_test();
  void* const regress_f3_item = reinterpret_cast<void*>(0xa500);
  const AegpTime f3_early{3, 24};
  const AegpTime f3_late{4, 24};
  const std::array<uint64_t, 2> regress_f3_effects{{0xe501, 0xe502}};
  if (!aexcompat::aegp_staged_item_runtime::register_item(
          regress_f3_item, 7001, SamplingPolicy::hold, nullptr, 0,
          regress_f3_effects.data(), regress_f3_effects.size()))
    return false;
  for (uint64_t effect : regress_f3_effects)
    for (StageKind kind : {StageKind::upstream, StageKind::all_effects,
                           StageKind::downstream})
      if (!publish_scheduler_stage(regress_f3_item, kind, effect, f3_early,
              static_cast<uint8_t>(20 + static_cast<uint8_t>(kind))))
        return false;
  if (!publish_scheduler_stage(regress_f3_item, StageKind::final_item, 0,
          f3_early, 31))
    return false;
  for (uint64_t effect : regress_f3_effects)
    for (StageKind kind : {StageKind::upstream, StageKind::all_effects,
                           StageKind::downstream})
      if ((effect != regress_f3_effects[0] || kind != StageKind::all_effects) &&
          !publish_scheduler_stage(regress_f3_item, kind, effect, f3_late,
              static_cast<uint8_t>(40 + static_cast<uint8_t>(kind))))
        return false;
  if (!publish_scheduler_stage(regress_f3_item, StageKind::final_item, 0,
          f3_late, 51))
    return false;
  void* f3_rejected = reinterpret_cast<void*>(1);
  if (aexcompat::aegp_staged_item_runtime::publish_registered_receipt(
          make_request(regress_f3_item, time), &f3_rejected) == 0 || f3_rejected)
    return false;
  if (!publish_scheduler_stage(regress_f3_item, StageKind::all_effects,
          regress_f3_effects[0], f3_late, 61))
    return false;
  ReceiptSnapshot regress_f3_snap{};
  if (!checkout_registered(make_request(regress_f3_item, time),
          regress_f3_snap) ||
      !same_test_time(regress_f3_snap.source_time, f3_late) ||
      regress_f3_snap.resolved_stage_count != 7 ||
      regress_f3_snap.sampling_policy != static_cast<uint8_t>(SamplingPolicy::hold))
    return false;

  // Regression F2: all_effects requires the same real effect identity as its
  // upstream/downstream boundaries; zero is never a production sentinel.
  clear_staged_item_worlds_for_test();
  void* const regress_f2_item = reinterpret_cast<void*>(0xa600);
  std::array<uint64_t, 1> regress_f2_effect{{0xe601}};
  if (!aexcompat::aegp_staged_item_runtime::register_item(
          regress_f2_item, 8001, SamplingPolicy::exact, nullptr, 0,
          regress_f2_effect.data(), regress_f2_effect.size()))
    return false;
  if (publish_scheduler_stage(regress_f2_item, StageKind::all_effects,
          0, time, 22) ||
      !publish_scheduler_stage(regress_f2_item, StageKind::upstream,
          regress_f2_effect[0], time, 21) ||
      !publish_scheduler_stage(regress_f2_item, StageKind::all_effects,
          regress_f2_effect[0], time, 22) ||
      !publish_scheduler_stage(regress_f2_item, StageKind::downstream,
          regress_f2_effect[0], time, 23) ||
      !publish_scheduler_stage(regress_f2_item, StageKind::final_item, 0,
          time, 51))
    return false;
  ReceiptSnapshot regress_f2_snap{};
  if (!checkout_registered(make_request(regress_f2_item, time), regress_f2_snap) ||
      regress_f2_snap.item_identity != 8001 ||
      regress_f2_snap.resolved_stage_count != 4)
    return false;

  // Regression F1: complete metadata registration carries durable identity,
  // declared dependency, hold policy, and every real effect instance.
  clear_staged_item_worlds_for_test();
  void* const regress_f1_item = reinterpret_cast<void*>(0xa700);
  void* const regress_f1_dependency = reinterpret_cast<void*>(0xa710);
  std::array<void*, 1> regress_f1_dependencies{{regress_f1_dependency}};
  std::array<uint64_t, 2> regress_f1_effects{{0xe701, 0xe702}};
  if (!aexcompat::aegp_staged_item_runtime::register_item(
          regress_f1_dependency, 9002, SamplingPolicy::exact, nullptr, 0,
          nullptr, 0) ||
      !aexcompat::aegp_staged_item_runtime::register_item(
          regress_f1_item, 9001, SamplingPolicy::hold,
          regress_f1_dependencies.data(), regress_f1_dependencies.size(),
          regress_f1_effects.data(), regress_f1_effects.size()) ||
      !publish_scheduler_stage(regress_f1_dependency, StageKind::final_item, 0,
          time, 11))
    return false;
  if (!publish_scheduler_stage(regress_f1_item, StageKind::upstream, 0xe701,
          time, 21) ||
      !publish_scheduler_stage(regress_f1_item, StageKind::all_effects, 0xe701,
          time, 22) ||
      !publish_scheduler_stage(regress_f1_item, StageKind::downstream, 0xe701,
          time, 23) ||
      !publish_scheduler_stage(regress_f1_item, StageKind::upstream, 0xe702,
          time, 31) ||
      !publish_scheduler_stage(regress_f1_item, StageKind::all_effects, 0xe702,
          time, 32) ||
      !publish_scheduler_stage(regress_f1_item, StageKind::downstream, 0xe702,
          time, 33) ||
      !publish_scheduler_stage(regress_f1_item, StageKind::final_item, 0,
          time, 51))
    return false;
  ReceiptSnapshot regress_f1_snap{};
  if (!checkout_registered(make_request(regress_f1_item, time), regress_f1_snap) ||
      regress_f1_snap.item_identity != 9001 ||
      regress_f1_snap.resolved_stage_count != 8 ||
      regress_f1_snap.resolved_depth != 1 ||
      regress_f1_snap.sampling_policy != static_cast<uint8_t>(SamplingPolicy::hold))
    return false;

  // Direct/indirect cycles, excessive nesting, and stage-count overflow are
  // all terminal and leave no partial receipt or ownership.
  const auto cycles_before =
      aexcompat::aegp_staged_item_runtime::diagnostics().cycles_rejected;
  clear_staged_item_worlds_for_test();
  void* const direct_item = reinterpret_cast<void*>(0xc100);
  std::array<void*, 1> direct_dependency{{direct_item}};
  if (!aexcompat::aegp_staged_item_runtime::register_item(
          direct_item, 3001, SamplingPolicy::exact, direct_dependency.data(),
          direct_dependency.size(), nullptr, 0) ||
      !publish_scheduler_stage(direct_item, StageKind::final_item, 0, time, 61))
    return false;
  rejected = reinterpret_cast<void*>(1);
  if (aexcompat::aegp_staged_item_runtime::publish_registered_receipt(
          make_request(direct_item, time), &rejected) == 0 || rejected)
    return false;
  clear_staged_item_worlds_for_test();
  void* const indirect_a = reinterpret_cast<void*>(0xc200);
  void* const indirect_b = reinterpret_cast<void*>(0xc300);
  std::array<void*, 1> a_dependency{{indirect_b}};
  std::array<void*, 1> b_dependency{{indirect_a}};
  if (!aexcompat::aegp_staged_item_runtime::register_item(
          indirect_a, 3002, SamplingPolicy::exact, a_dependency.data(), 1,
          nullptr, 0) ||
      !aexcompat::aegp_staged_item_runtime::register_item(
          indirect_b, 3003, SamplingPolicy::exact, b_dependency.data(), 1,
          nullptr, 0) ||
      !publish_scheduler_stage(indirect_b, StageKind::final_item, 0, time, 62) ||
      !publish_scheduler_stage(indirect_a, StageKind::final_item, 0, time, 63))
    return false;
  rejected = reinterpret_cast<void*>(1);
  if (aexcompat::aegp_staged_item_runtime::publish_registered_receipt(
          make_request(indirect_a, time), &rejected) == 0 || rejected)
    return false;
  clear_staged_item_worlds_for_test();
  std::array<void*, 9> deep_items{};
  for (std::size_t index = 0; index < deep_items.size(); ++index)
    deep_items[index] =
        reinterpret_cast<void*>(0xd100 + static_cast<uintptr_t>(index) * 0x10);
  for (std::size_t reverse = deep_items.size(); reverse-- > 0;) {
    void* dependency =
        reverse + 1 < deep_items.size() ? deep_items[reverse + 1] : nullptr;
    if (!aexcompat::aegp_staged_item_runtime::register_item(
            deep_items[reverse], 4000 + reverse, SamplingPolicy::exact,
            dependency ? &dependency : nullptr, dependency ? 1 : 0, nullptr,
            0) ||
        !publish_scheduler_stage(deep_items[reverse], StageKind::final_item, 0,
            time, static_cast<uint8_t>(70 + reverse)))
      return false;
  }
  rejected = reinterpret_cast<void*>(1);
  if (aexcompat::aegp_staged_item_runtime::publish_registered_receipt(
          make_request(deep_items[0], time), &rejected) == 0 || rejected)
    return false;
  clear_staged_item_worlds_for_test();
  void* const stage_limited_item = reinterpret_cast<void*>(0xe100);
  std::array<uint64_t, 8> many_effects{{
      1, 2, 3, 4, 5, 6, 7, 8}};
  if (!aexcompat::aegp_staged_item_runtime::register_item(
          stage_limited_item, 5001, SamplingPolicy::exact, nullptr, 0,
          many_effects.data(), many_effects.size()))
    return false;
  for (uint64_t effect : many_effects)
    for (StageKind kind : {StageKind::upstream, StageKind::all_effects,
                           StageKind::downstream})
      if (!publish_scheduler_stage(stage_limited_item, kind, effect, time,
              static_cast<uint8_t>(80 + effect + static_cast<uint8_t>(kind))))
        return false;
  if (!publish_scheduler_stage(stage_limited_item, StageKind::final_item, 0,
          time, 99))
    return false;
  rejected = reinterpret_cast<void*>(1);
  if (aexcompat::aegp_staged_item_runtime::publish_registered_receipt(
          make_request(stage_limited_item, time), &rejected) == 0 || rejected)
    return false;
  const auto scheduler_diagnostics =
      aexcompat::aegp_staged_item_runtime::diagnostics();
  if (scheduler_diagnostics.cycles_rejected < cycles_before + 2 ||
      scheduler_diagnostics.direct_cycles_rejected == 0 ||
      scheduler_diagnostics.indirect_cycles_rejected == 0 ||
      scheduler_diagnostics.depth_limit_rejections == 0 ||
      scheduler_diagnostics.stage_limit_rejections == 0 ||
      scheduler_diagnostics.hold_hits == 0 ||
      scheduler_diagnostics.nearest_hits == 0 ||
      scheduler_diagnostics.partial_failures == 0 ||
      scheduler_diagnostics.in_flight != 0 ||
      !async_receipt_lifetimes_balanced())
    return false;

  const auto invalidations_before =
      aexcompat::aegp_staged_item_runtime::diagnostics().generation_invalidations;
  if (!verify_item_render_cycle_contract(options)) return false;
  const auto diagnostics = aexcompat::aegp_staged_item_runtime::diagnostics();
  return diagnostics.generation_invalidations > invalidations_before &&
      render_options_lifetimes_balanced() && async_receipt_lifetimes_balanced();
}

int32_t acquire_suite(const char*, int32_t, const void**);
using AegpAsyncFrameReadyCallback =
    int32_t(__cdecl*)(uint64_t, uint8_t, int32_t, void*, void*);
int32_t __cdecl render_checkout_layer_v5(void*, AegpRenderCancelV1, void*, void**);
int32_t __cdecl render_checkout_layer_async_reject(
    void*, AegpAsyncFrameReadyCallback, void*, uint64_t*);
void drain_async_layer_requests();
std::string sha256_bytes(const unsigned char*, std::size_t);
#define g_aegp_effect scene_runtime::scene_runtime_state().effect
#define g_aegp_layers scene_runtime::scene_runtime_state().layers
using LayerRenderContext = aexcompat::aegp_layer_render_runtime::Context;
using worker_runtime::EffectEntry;

struct LayerSuite2AsyncTestResult {
  std::atomic<bool> done{};
  int32_t error{4};
  void* receipt{};
};
int32_t __cdecl layer_suite2_async_test_callback(
    uint64_t, uint8_t canceled, int32_t error, void* receipt, void* refcon) {
  auto* result = static_cast<LayerSuite2AsyncTestResult*>(refcon);
  if (!result) return 4;
  result->error = canceled ? 4 : error;
  result->receipt = receipt;
  result->done.store(true);
  return 0;
}

bool verify_aegp_layer_render_options_suite2() {
  const bool saved_effect_live = g_aegp_effect_live;
  const auto saved_effect_instances = g_aegp_effect_instances;
  const auto saved_effect_leases = g_aegp_effect_leases;
  const auto saved_context = aexcompat::aegp_layer_render_runtime::context();
  const uint32_t created_before = layer_created_count();
  const uint32_t disposed_before = layer_disposed_count();
  g_aegp_effect_live = true;
  g_aegp_effect_instances[0] = {
      &g_aegp_layers[0], kAegpInstalledEffects[0].key, 0, 1, 1, true};
  g_aegp_effect_instances[0].render_ref = scene_context()->pf_effect;
  for (std::size_t index = 1; index < g_aegp_effect_instances.size(); ++index)
    g_aegp_effect_instances[index] = {};
  g_aegp_effect_leases = {};
  void* second_render_effect = nullptr;
  if (aegp_apply_effect(1, &g_aegp_layers[0],
                        kAegpInstalledEffects[0].key,
                        &second_render_effect) != 0 ||
      !second_render_effect)
    return false;
  const auto make_world = [](uint8_t red, uint8_t green, uint8_t blue) {
    std::vector<unsigned char> pixels(4 * 2 * 4);
    for (std::size_t pixel = 0; pixel < pixels.size() / 4; ++pixel) {
      pixels[pixel * 4 + 0] = 128;
      pixels[pixel * 4 + 1] = red;
      pixels[pixel * 4 + 2] = green;
      pixels[pixel * 4 + 3] = blue;
    }
    return pixels;
  };
  auto source = make_world(100, 50, 25);
  auto all_effects = make_world(40, 120, 30);
  auto downstream_pixels = make_world(20, 60, 140);
  clear_staged_item_worlds_for_test();
  LayerRenderContext context{};
  context.entry = reinterpret_cast<EffectEntry>(&verify_aegp_layer_render_options_suite2);
  context.current_time = 0;
  context.time_scale = 1;
  context.time_step = 1;
  context.total_time = 1;
  context.pixel_bytes = 4;
  context.source_argb = &source;
  context.source_width = 4;
  context.source_height = 2;
  context.all_effects_argb = &all_effects;
  context.all_effects_width = 4;
  context.all_effects_height = 2;
  context.all_effects_pixel_bytes = 4;
  context.all_effects_finalized = true;
  context.active_effect_instance = staged_effect_identity_for_render_ref(
      second_render_effect);
  aexcompat::aegp_layer_render_runtime::context() = context;

  const void* acquired = nullptr;
  bool ok = acquire_suite("AEGP Layer Render Options Suite", 2, &acquired) == 0 &&
      acquired == aexcompat::worker_runtime::host_suites::layer_render_options_suite(2);
  void* upstream = nullptr;
  ok = ok && new_from_upstream_of_effect(1, &g_aegp_effect, &upstream) == 0 && upstream &&
      set_layer_render_downsample(upstream, 2, 2) == 0 &&
      set_layer_render_world_type(upstream, 2) == 0 &&
      set_layer_render_matte(upstream, 1) == 0;
  auto checkout_hash = [&](void* options, std::string& hash,
                           uint64_t* published_effect = nullptr) {
    void* receipt = nullptr;
    void** world = nullptr;
    int32_t type = 0, width = 0, height = 0;
    void* pixels = nullptr;
    const bool checked_out = render_checkout_layer_v5(
        options, nullptr, nullptr, &receipt) == 0 && receipt &&
        get_receipt_world(receipt, &world) == 0 && world &&
        aegp_world_get_type(world, &type) == 0 && type == 2 &&
        aegp_world_get_size(world, &width, &height) == 0 && width == 2 && height == 1 &&
        aegp_world_get_base_addr16(world, &pixels) == 0 && pixels;
    if (checked_out) hash = sha256_bytes(static_cast<const unsigned char*>(pixels),
        static_cast<std::size_t>(width) * height * 8);
    ReceiptSnapshot snapshot{};
    if (checked_out && published_effect) {
      if (!aexcompat::render_receipts::snapshot(receipt, snapshot))
        return false;
      *published_effect = snapshot.effect_instance;
    }
    return checked_out && checkin_frame(receipt) == 0;
  };
  std::string upstream_hash, all_hash, downstream_hash, async_hash;
  ok = ok && checkout_hash(upstream, upstream_hash);

  void* all = nullptr;
  uint64_t all_effect_instance = 0;
  const uint64_t slot0_effect_instance =
      (static_cast<uint64_t>(g_aegp_effect_instances[0].generation) << 32) | 1;
  const uint64_t slot1_effect_instance =
      (static_cast<uint64_t>(g_aegp_effect_instances[1].generation) << 32) | 2;
  ok = ok && new_layer_render_options(1, &g_aegp_layers[0], &all) == 0 && all &&
      set_layer_render_downsample(all, 2, 2) == 0 &&
      set_layer_render_world_type(all, 2) == 0 &&
      set_layer_render_matte(all, 1) == 0 &&
      checkout_hash(all, all_hash, &all_effect_instance) &&
      all_effect_instance == slot1_effect_instance &&
      all_effect_instance != slot0_effect_instance;

  void* downstream = nullptr;
  ok = ok && new_from_downstream_of_effect(1, &g_aegp_effect, &downstream) == 0 && downstream;
  void* rejected = reinterpret_cast<void*>(1);
  ok = ok && render_checkout_layer_v5(downstream, nullptr, nullptr, &rejected) != 0 &&
      rejected == nullptr;
  aexcompat::aegp_layer_render_runtime::context().downstream_argb = &downstream_pixels;
  aexcompat::aegp_layer_render_runtime::context().downstream_width = 4;
  aexcompat::aegp_layer_render_runtime::context().downstream_height = 2;
  aexcompat::aegp_layer_render_runtime::context().downstream_pixel_bytes = 4;
  aexcompat::aegp_layer_render_runtime::context().downstream_finalized = true;
  ok = ok && set_layer_render_downsample(downstream, 2, 2) == 0 &&
      set_layer_render_world_type(downstream, 2) == 0 &&
      set_layer_render_matte(downstream, 1) == 0 &&
      checkout_hash(downstream, downstream_hash) &&
      upstream_hash != all_hash && upstream_hash != downstream_hash &&
      all_hash != downstream_hash;

  LayerSuite2AsyncTestResult async_result{};
  uint64_t request_id = 0;
  ok = ok && render_checkout_layer_async_reject(downstream,
      &layer_suite2_async_test_callback, &async_result, &request_id) == 0 && request_id != 0;
  drain_async_layer_requests();
  void** async_world = nullptr;
  void* async_pixels = nullptr;
  int32_t async_width = 0, async_height = 0;
  ok = ok && async_result.done.load() && async_result.error == 0 && async_result.receipt &&
      get_receipt_world(async_result.receipt, &async_world) == 0 && async_world &&
      aegp_world_get_size(async_world, &async_width, &async_height) == 0 &&
      aegp_world_get_base_addr16(async_world, &async_pixels) == 0 && async_pixels;
  if (async_pixels)
    async_hash = sha256_bytes(static_cast<const unsigned char*>(async_pixels),
        static_cast<std::size_t>(async_width) * async_height * 8);
  ok = ok && async_hash == downstream_hash &&
      checkin_frame(async_result.receipt) == 0;
  g_aegp_effect_live = false;
  rejected = reinterpret_cast<void*>(1);
  ok = ok && render_checkout_layer_v5(upstream, nullptr, nullptr, &rejected) != 0 &&
      rejected == nullptr;
  ok = ok && dispose_layer_render_options(upstream) == 0 &&
      dispose_layer_render_options(all) == 0 &&
      dispose_layer_render_options(downstream) == 0;
  aexcompat::aegp_layer_render_runtime::context() = saved_context;
  clear_staged_item_worlds_for_test();
  g_aegp_effect_live = saved_effect_live;
  g_aegp_effect_instances = saved_effect_instances;
  g_aegp_effect_leases = saved_effect_leases;
  return ok && async_receipt_lifetimes_balanced() &&
      layer_created_count() == created_before + 3 &&
      layer_disposed_count() == disposed_before + 3;
}

}  // namespace aexcompat::l2_detail
