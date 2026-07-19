#include "worker_aegp_item_render_runtime.hpp"

#include "worker_render_receipts.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <cstring>
#include <memory>

namespace aexcompat::aegp_item_render_runtime {
namespace {

using render_options::ItemValue;
using render_receipts::ReceiptDraft;
using suite_abi::AegpRect;

constexpr int32_t kSyntheticCompWidth = 17;
constexpr int32_t kSyntheticCompHeight = 9;
Hooks g_hooks{};

// Bounded synthetic-item contract, not an Adobe rendering model. Output pixels sample
// source (x * downsample_x, y * downsample_y); ROI and fields are evaluated there.
// Field 0 renders all rows, 1 renders even source rows, and 2 renders odd source rows.
std::array<uint8_t, 4> synthetic_pixel(const ItemValue& options,
                                       int32_t source_x, int32_t source_y) {
  const int64_t scaled_time = static_cast<int64_t>(options.time.value) * 256 /
      options.time.scale;
  const uint8_t time = static_cast<uint8_t>(scaled_time);
  return {{static_cast<uint8_t>(64 + source_x * 11 + source_y * 7 + time),
           static_cast<uint8_t>(source_x * 17 + time),
           static_cast<uint8_t>(source_y * 29 + time * 3),
           static_cast<uint8_t>(source_x * 5 + source_y * 13 + time * 7)}};
}

void populate_synthetic_pixels_impl(ReceiptDraft& receipt, int32_t type,
                                    int32_t width, int32_t height) {
  const auto& options = receipt.render_options;
  const AegpRect source_roi = options.roi.left == 0 && options.roi.top == 0 &&
      options.roi.right == 0 && options.roi.bottom == 0
      ? AegpRect{0, 0, kSyntheticCompWidth, kSyntheticCompHeight} : options.roi;
  const int32_t pixel_bytes = type == 1 ? 4 : (type == 2 ? 8 : 16);
  for (int32_t y = 0; y < height; ++y) {
    const int32_t source_y = y * options.downsample_y;
    for (int32_t x = 0; x < width; ++x) {
      const int32_t source_x = x * options.downsample_x;
      const bool in_roi = source_x >= source_roi.left && source_x < source_roi.right &&
          source_y >= source_roi.top && source_y < source_roi.bottom;
      const bool in_field = options.field == 0 ||
          (options.field == 1 && (source_y & 1) == 0) ||
          (options.field == 2 && (source_y & 1) != 0);
      if (!in_roi || !in_field) continue;
      auto pixel = synthetic_pixel(options, source_x, source_y);
      if (options.matte == 1) pixel[0] = 255;
      if (options.channel_order == 1)
        pixel = {{pixel[3], pixel[2], pixel[1], pixel[0]}};
      std::byte* destination = receipt.pixels.data() +
          (static_cast<std::size_t>(y) * width + x) * pixel_bytes;
      if (type == 1) {
        std::memcpy(destination, pixel.data(), 4);
      } else if (type == 2) {
        std::array<uint16_t, 4> converted{};
        for (std::size_t channel = 0; channel < 4; ++channel)
          converted[channel] = static_cast<uint16_t>(pixel[channel]) * 257u;
        std::memcpy(destination, converted.data(), sizeof(converted));
      } else {
        std::array<float, 4> converted{};
        for (std::size_t channel = 0; channel < 4; ++channel)
          converted[channel] = static_cast<float>(pixel[channel]) / 255.0f;
        std::memcpy(destination, converted.data(), sizeof(converted));
      }
    }
  }
}

}  // namespace

void configure(Hooks hooks) noexcept { g_hooks = hooks; }

void populate_synthetic_pixels(ReceiptDraft& receipt, int32_t type,
                               int32_t width, int32_t height) {
  populate_synthetic_pixels_impl(receipt, type, width, height);
}

int32_t publish_synthetic(int32_t pixel_format, void** output,
                          const ItemValue* options) {
  if (!output) return 4;
  *output = nullptr;
  const int32_t type = world_registry::aegp_world_type_from_format(pixel_format);
  const int32_t bytes_per_pixel = type == 1 ? 4 : (type == 2 ? 8 : (type == 3 ? 16 : 0));
  const int32_t width = options
      ? (kSyntheticCompWidth + options->downsample_x - 1) / options->downsample_x : 8;
  const int32_t height = options
      ? (kSyntheticCompHeight + options->downsample_y - 1) / options->downsample_y : 4;
  const uint64_t bytes = static_cast<uint64_t>(width) * height * bytes_per_pixel;
  if (!bytes_per_pixel || bytes > render_receipts::kMaxReceiptBytes) return 4;
  std::unique_ptr<ReceiptDraft> receipt;
  try {
    receipt = std::make_unique<ReceiptDraft>();
    receipt->pixels.resize(static_cast<std::size_t>(bytes));
  } catch (...) { return 4; }
  receipt->pixel_format = pixel_format;
  if (options) {
    receipt->has_render_options = true;
    receipt->render_options = *options;
    const AegpRect full{0, 0, kSyntheticCompWidth, kSyntheticCompHeight};
    const bool full_roi = options->roi.left == 0 && options->roi.top == 0 &&
        options->roi.right == 0 && options->roi.bottom == 0;
    const AegpRect clipped = full_roi ? full : AegpRect{
        (std::max)(0, options->roi.left), (std::max)(0, options->roi.top),
        (std::min)(kSyntheticCompWidth, options->roi.right),
        (std::min)(kSyntheticCompHeight, options->roi.bottom)};
    const auto ceil_div = [](int32_t value, int16_t divisor) {
      return value <= 0 ? 0 : (value + divisor - 1) / divisor;
    };
    receipt->rendered_region = {ceil_div(clipped.left, options->downsample_x),
        ceil_div(clipped.top, options->downsample_y),
        ceil_div(clipped.right, options->downsample_x),
        ceil_div(clipped.bottom, options->downsample_y)};
  } else {
    receipt->rendered_region = {0, 0, width, height};
  }
  receipt->world.world_flags = type == 1 ? 0 : 1;
  receipt->world.data = receipt->pixels.data();
  receipt->world.rowbytes = width * bytes_per_pixel;
  receipt->world.width = width;
  receipt->world.height = height;
  receipt->world.extent_hint = {0, 0, width, height};
  receipt->world.pix_aspect_ratio = {1, 1};
  if (options) populate_synthetic_pixels(*receipt, type, width, height);
  return render_receipts::register_receipt(std::move(receipt), output);
}

int32_t publish_receipt(void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  if (!receipt || !g_hooks.publish_staged) return 4;
  return g_hooks.publish_staged(options, receipt);
}

int32_t checkout(void* options, Cancel check_cancel, void* cancel_refcon, void** out) {
  if (out) *out = nullptr;
  if (!out || !g_hooks.snapshot_options || !g_hooks.publish_cached ||
      !g_hooks.publish_staged) return 4;
  if (check_cancel) {
    uint8_t canceled = 0;
    const int32_t cancel_error = check_cancel(cancel_refcon, &canceled);
    if (cancel_error != 0) return cancel_error;
    if (canceled != 0) return 4;
  }
  ItemValue snapshot{};
  if (!g_hooks.snapshot_options(options, snapshot)) return 4;
  bool cache_hit = false;
  const int32_t cache_error = g_hooks.publish_cached(snapshot, out, &cache_hit);
  if (cache_error != 0 || cache_hit) return cache_error;
  return g_hooks.publish_staged(options, out);
}

}  // namespace aexcompat::aegp_item_render_runtime
