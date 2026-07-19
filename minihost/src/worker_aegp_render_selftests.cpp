#include "worker_aegp_render_selftests.hpp"
#include "worker_aegp_render_options.hpp"
#include "worker_aegp_staged_item_runtime.hpp"
#include "worker_render_receipts.hpp"
#include "worker_world_registry.hpp"
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
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
extern bool g_synthetic_receipt_test_mode;
extern int g_async_manager;
extern std::array<uint8_t, 4> g_render_options_baseline8, g_render_options_time8,
    g_render_options_downsample8, g_render_options_roi_outside8,
    g_render_options_roi_inside8, g_render_options_field_excluded8,
    g_render_options_matte8;
extern std::array<uint16_t, 4> g_render_options_argb16;
extern std::array<float, 4> g_render_options_argb32f;
bool render_options_lifetimes_balanced();
bool async_receipt_lifetimes_balanced();
void populate_synthetic_item_pixels(ReceiptDraft&, int32_t, int32_t, int32_t);
void* aegp_comp_item_handle();
int32_t __cdecl checkout_item_frame_async(void*, uint32_t, void*, void**);
int32_t get_receipt_world(void*, void***);
int32_t checkin_frame(void*);
int32_t __cdecl render_get_region_reject(void*, void*);
using AegpRenderCancelV1 = int32_t(__cdecl*)(void*, uint8_t*);
int32_t __cdecl render_checkout_frame_reject(void*, AegpRenderCancelV1, void*, void**);
int32_t publish_item_receipt(void*, void**);
void clear_staged_item_worlds_for_test();
bool verify_item_render_cycle_contract(void*);

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
    populate_synthetic_item_pixels(probe, type, width, height);
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
          publish_item_receipt(local, &local_receipt) != 0 ||
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
  for (int32_t depth = 0; depth < 3; ++depth) {
    const int32_t pixel_bytes = 4 << depth;
    const auto pixels = fixture(pixel_bytes);
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

  auto pixels8 = fixture(4);
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

  if (!verify_item_render_cycle_contract(options)) return false;
  return render_options_lifetimes_balanced() && async_receipt_lifetimes_balanced();
}

}  // namespace aexcompat::l2_detail
