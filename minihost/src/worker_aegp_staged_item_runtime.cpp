#include "worker_aegp_staged_item_runtime.hpp"

#include "worker_render_receipts.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <limits>
#include <memory>
#include <mutex>
#include <numeric>
#include <vector>

namespace aexcompat::aegp_staged_item_runtime {
namespace {

using render_options::ItemValue;
using render_receipts::ReceiptDraft;
using suite_abi::AegpRect;
using suite_abi::AegpTime;

struct NormalizedRational {
  int64_t numerator{};
  uint64_t denominator{};
  bool valid{};

  friend bool operator==(const NormalizedRational& left,
                         const NormalizedRational& right) {
    return left.valid == right.valid && (!left.valid ||
        (left.numerator == right.numerator && left.denominator == right.denominator));
  }
};

NormalizedRational normalize_rational(AegpTime time) noexcept {
  if (time.scale == 0) return {};
  const int64_t numerator = time.value;
  const uint64_t denominator = time.scale;
  const uint64_t magnitude = numerator < 0
      ? static_cast<uint64_t>(-numerator) : static_cast<uint64_t>(numerator);
  const uint64_t divisor = std::gcd(magnitude, denominator);
  return {numerator / static_cast<int64_t>(divisor), denominator / divisor, true};
}

struct StagedItemWorld {
  void* item{};
  AegpTime time{};
  AegpTime time_step{};
  int8_t quality{1};
  uint8_t guide_layers{};
  int32_t pixel_format{};
  int32_t width{};
  int32_t height{};
  int32_t rowbytes{};
  uint32_t project_generation{};
  struct StageIdentity {
    void* item{};
    NormalizedRational time{};
    NormalizedRational time_step{};
    int8_t quality{};
    uint8_t guide_layers{};
    int32_t pixel_format{};
    int32_t width{};
    int32_t height{};
    int32_t rowbytes{};
    uint32_t project_generation{};

    friend bool operator==(const StageIdentity& left, const StageIdentity& right) {
      return left.item == right.item && left.time == right.time &&
          left.time_step == right.time_step && left.quality == right.quality &&
          left.guide_layers == right.guide_layers &&
          left.pixel_format == right.pixel_format && left.width == right.width &&
          left.height == right.height && left.rowbytes == right.rowbytes &&
          left.project_generation == right.project_generation;
    }
  } identity{};
  uint64_t stage_generation{};
  std::shared_ptr<const std::vector<std::byte>> backing;
};

struct ItemRenderStackKey {
  void* item{};
  AegpTime time{};
  uint32_t project_generation{};
};

constexpr std::size_t kMaxStagedItemWorlds = 32;
Hooks g_hooks{};
std::mutex g_mutex;
std::vector<StagedItemWorld> g_worlds;
std::atomic<uint64_t> g_stage_generation{1};
thread_local std::vector<ItemRenderStackKey> g_render_stack;
std::atomic<uint32_t> g_published{};
std::atomic<uint32_t> g_cache_hits{};
std::atomic<uint32_t> g_cache_misses{};
std::atomic<uint32_t> g_cycles_rejected{};
std::atomic<uint32_t> g_generation_invalidations{};
std::atomic<uint32_t> g_evictions{};
uint32_t g_cache_generation{};

int32_t pixel_bytes_for(int32_t pixel_format) {
  if (pixel_format == world_registry::kPixelFormatArgb32) return 4;
  if (pixel_format == world_registry::kPixelFormatArgb64) return 8;
  if (pixel_format == world_registry::kPixelFormatArgb128) return 16;
  return 0;
}

bool same_rational(const AegpTime& left, const AegpTime& right) {
  return left.scale != 0 && right.scale != 0 &&
      normalize_rational(left) == normalize_rational(right);
}

StagedItemWorld::StageIdentity make_stage_identity(
    void* item, AegpTime time, AegpTime time_step, int8_t quality,
    uint8_t guide_layers, int32_t pixel_format, int32_t width, int32_t height,
    int32_t rowbytes, uint32_t project_generation) noexcept {
  return {item, normalize_rational(time), normalize_rational(time_step), quality,
          guide_layers, pixel_format, width, height, rowbytes, project_generation};
}

void invalidate_generation_locked() {
  if (!g_worlds.empty() || g_cache_generation != 0) ++g_generation_invalidations;
  g_worlds.clear();
  g_cache_generation = 0;
}

void ensure_generation(uint32_t generation) {
  std::lock_guard<std::mutex> lock(g_mutex);
  if (g_cache_generation != 0 && g_cache_generation != generation)
    invalidate_generation_locked();
  g_cache_generation = generation;
}

bool stack_contains(const ItemRenderStackKey& key) {
  return std::any_of(g_render_stack.begin(), g_render_stack.end(), [&](const auto& active) {
    return active.item == key.item && active.project_generation == key.project_generation &&
        same_rational(active.time, key.time);
  });
}

struct StackScope {
  bool entered{};
  explicit StackScope(ItemRenderStackKey key) {
    if (stack_contains(key)) return;
    g_render_stack.push_back(key);
    entered = true;
  }
  ~StackScope() { if (entered) g_render_stack.pop_back(); }
};

bool snapshot_world(const ItemValue& options, StagedItemWorld& stage) {
  const int32_t pixel_format = options.world_type == 1 ? world_registry::kPixelFormatArgb32 :
      (options.world_type == 2 ? world_registry::kPixelFormatArgb64 :
       (options.world_type == 3 ? world_registry::kPixelFormatArgb128 : 0));
  if (!g_hooks.project_generation) return false;
  const uint32_t generation = g_hooks.project_generation();
  if (generation == 0) return false;
  ensure_generation(generation);
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = std::find_if(g_worlds.rbegin(), g_worlds.rend(), [&](const auto& value) {
    return value.item == options.item && same_rational(value.time, options.time) &&
        same_rational(value.time_step, options.time_step) &&
        value.quality == options.render_quality &&
        value.guide_layers == options.render_guide_layers &&
        value.pixel_format == pixel_format && value.project_generation == generation &&
        value.identity.item == options.item &&
        value.identity.time == normalize_rational(options.time) &&
        value.identity.time_step == normalize_rational(options.time_step) &&
        value.identity.quality == options.render_quality &&
        value.identity.guide_layers == options.render_guide_layers &&
        value.identity.pixel_format == pixel_format &&
        value.identity.project_generation == generation;
  });
  if (found == g_worlds.rend() || !found->backing) {
    ++g_cache_misses;
    return false;
  }
  stage = *found;
  ++g_cache_hits;
  return true;
}

float read_channel(const std::byte* pixel, int32_t pixel_bytes, int channel) {
  if (pixel_bytes == 4)
    return static_cast<float>(reinterpret_cast<const uint8_t*>(pixel)[channel]) / 255.0f;
  if (pixel_bytes == 8)
    return static_cast<float>(reinterpret_cast<const uint16_t*>(pixel)[channel]) / 32768.0f;
  float value = 0.0f;
  std::memcpy(&value, pixel + channel * sizeof(float), sizeof(value));
  return std::isfinite(value) ? value : 0.0f;
}

void write_channel(std::byte* pixel, int32_t pixel_bytes, int channel, float value) {
  value = std::clamp(value, 0.0f, 1.0f);
  if (pixel_bytes == 4)
    reinterpret_cast<uint8_t*>(pixel)[channel] = static_cast<uint8_t>(std::lround(value * 255.0f));
  else if (pixel_bytes == 8)
    reinterpret_cast<uint16_t*>(pixel)[channel] = static_cast<uint16_t>(std::lround(value * 32768.0f));
  else
    std::memcpy(pixel + channel * sizeof(float), &value, sizeof(value));
}

int32_t transform(const StagedItemWorld& stage, const ItemValue& options,
                  std::unique_ptr<ReceiptDraft>& receipt) {
  const int32_t pixel_bytes = pixel_bytes_for(stage.pixel_format);
  if (!stage.backing || !pixel_bytes || options.downsample_x <= 0 || options.downsample_y <= 0 ||
      options.field < 0 || options.field > 2 || options.matte < 0 || options.matte > 2 ||
      options.channel_order < 0 || options.channel_order > 1) return 4;
  const uint64_t source_bytes = static_cast<uint64_t>(stage.rowbytes) * stage.height;
  if (stage.width <= 0 || stage.height <= 0 || stage.rowbytes < stage.width * pixel_bytes ||
      source_bytes != stage.backing->size()) return 4;
  const int32_t width = (stage.width + options.downsample_x - 1) / options.downsample_x;
  const int32_t height = (stage.height + options.downsample_y - 1) / options.downsample_y;
  const uint64_t bytes = static_cast<uint64_t>(width) * height * pixel_bytes;
  if (bytes == 0 || bytes > render_receipts::kMaxReceiptBytes) return 4;
  try {
    receipt = std::make_unique<ReceiptDraft>();
    receipt->pixels.resize(static_cast<std::size_t>(bytes));
  } catch (...) { return 4; }
  receipt->staged_source_pin = stage.backing;
  const bool zero_roi = options.roi.left == 0 && options.roi.top == 0 &&
      options.roi.right == 0 && options.roi.bottom == 0;
  const AegpRect roi = zero_roi ? AegpRect{0, 0, stage.width, stage.height} :
      AegpRect{(std::max)(0, options.roi.left), (std::max)(0, options.roi.top),
          (std::min)(stage.width, options.roi.right), (std::min)(stage.height, options.roi.bottom)};
  const float background = 0x3030 / 65535.0f;
  for (int32_t y = 0; y < height; ++y) {
    const int32_t source_y = y * options.downsample_y;
    for (int32_t x = 0; x < width; ++x) {
      const int32_t source_x = x * options.downsample_x;
      const bool in_roi = source_x >= roi.left && source_x < roi.right &&
          source_y >= roi.top && source_y < roi.bottom;
      const bool in_field = options.field == 0 || (options.field == 1 && (source_y & 1) == 0) ||
          (options.field == 2 && (source_y & 1) != 0);
      if (!in_roi || !in_field) continue;
      const std::byte* source = stage.backing->data() +
          static_cast<std::size_t>(source_y) * stage.rowbytes +
          static_cast<std::size_t>(source_x) * pixel_bytes;
      std::array<float, 4> argb{};
      for (int c = 0; c < 4; ++c) argb[c] = read_channel(source, pixel_bytes, c);
      if (options.matte == 1)
        for (int c = 1; c < 4; ++c) argb[c] *= argb[0];
      else if (options.matte == 2)
        for (int c = 1; c < 4; ++c)
          argb[c] = argb[c] * argb[0] + background * (1.0f - argb[0]);
      const std::array<float, 4> packed = options.channel_order == 0 ? argb :
          std::array<float, 4>{{argb[3], argb[2], argb[1], argb[0]}};
      std::byte* destination = receipt->pixels.data() +
          (static_cast<std::size_t>(y) * width + x) * pixel_bytes;
      for (int c = 0; c < 4; ++c) write_channel(destination, pixel_bytes, c, packed[c]);
    }
  }
  const auto ceil_div = [](int32_t value, int32_t divisor) {
    return value <= 0 ? 0 : (value + divisor - 1) / divisor;
  };
  receipt->pixel_format = stage.pixel_format;
  receipt->has_render_options = true;
  receipt->render_options = options;
  receipt->rendered_region = {ceil_div(roi.left, options.downsample_x),
      ceil_div(roi.top, options.downsample_y), ceil_div(roi.right, options.downsample_x),
      ceil_div(roi.bottom, options.downsample_y)};
  receipt->render_timestamp = stage.project_generation;
  receipt->world.data = receipt->pixels.data();
  receipt->world.rowbytes = width * pixel_bytes;
  receipt->world.world_flags = pixel_bytes == 4 ? 0 : 1;
  receipt->world.width = width;
  receipt->world.height = height;
  receipt->world.extent_hint = {0, 0, width, height};
  receipt->world.pix_aspect_ratio = {1, 1};
  return 0;
}

}  // namespace

void configure(Hooks hooks) noexcept { g_hooks = hooks; }

void clear() noexcept {
  std::lock_guard<std::mutex> lock(g_mutex);
  invalidate_generation_locked();
}

bool publish_world(void* item, AegpTime time, AegpTime time_step, int8_t quality,
                   uint8_t guide_layers, int32_t pixel_format, int32_t width,
                   int32_t height, int32_t rowbytes, const void* pixels) {
  const int32_t pixel_bytes = pixel_bytes_for(pixel_format);
  const uint64_t tight_rowbytes = static_cast<uint64_t>(width) * pixel_bytes;
  const uint64_t tight_bytes = tight_rowbytes * height;
  if (!g_hooks.project_generation || !item || time.scale == 0 || time_step.scale == 0 ||
      time_step.value <= 0 || quality < 0 || quality > 1 || guide_layers > 1 || !pixel_bytes ||
      width <= 0 || height <= 0 || width > 4096 || height > 4096 ||
      rowbytes < tight_rowbytes || !pixels || tight_bytes == 0 ||
      tight_bytes > render_receipts::kMaxReceiptBytes) return false;
  std::shared_ptr<std::vector<std::byte>> backing;
  try {
    backing = std::make_shared<std::vector<std::byte>>(static_cast<std::size_t>(tight_bytes));
    for (int32_t y = 0; y < height; ++y)
      std::memcpy(backing->data() + static_cast<std::size_t>(y) * tight_rowbytes,
          static_cast<const std::byte*>(pixels) + static_cast<std::size_t>(y) * rowbytes,
          static_cast<std::size_t>(tight_rowbytes));
  } catch (...) { return false; }
  const uint32_t project_generation = g_hooks.project_generation();
  if (project_generation == 0) return false;
  ensure_generation(project_generation);
  const uint64_t stage_generation = g_stage_generation.fetch_add(1);
  if (stage_generation == 0) return false;
  const auto identity = make_stage_identity(item, time, time_step, quality, guide_layers,
      pixel_format, width, height, static_cast<int32_t>(tight_rowbytes), project_generation);
  if (!identity.time.valid || !identity.time_step.valid) return false;
  StagedItemWorld stage{item, time, time_step, quality, guide_layers, pixel_format, width, height,
      static_cast<int32_t>(tight_rowbytes), project_generation, identity, stage_generation,
      std::move(backing)};
  std::lock_guard<std::mutex> lock(g_mutex);
  auto same_key = [&](const auto& value) {
    return value.item == item && same_rational(value.time, time) &&
        same_rational(value.time_step, time_step) && value.quality == quality &&
        value.guide_layers == guide_layers && value.pixel_format == pixel_format &&
        value.width == width && value.height == height && value.rowbytes == tight_rowbytes &&
        value.project_generation == project_generation && value.identity == identity;
  };
  const auto existing = std::find_if(g_worlds.begin(), g_worlds.end(), same_key);
  if (existing != g_worlds.end()) *existing = std::move(stage);
  else if (g_worlds.size() == kMaxStagedItemWorlds) {
    const auto oldest = std::min_element(g_worlds.begin(), g_worlds.end(),
        [](const auto& left, const auto& right) { return left.stage_generation < right.stage_generation; });
    *oldest = std::move(stage);
    ++g_evictions;
  } else {
    try { g_worlds.push_back(std::move(stage)); } catch (...) { return false; }
  }
  ++g_published;
  return true;
}

int32_t publish_receipt(void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  if (!receipt || !g_hooks.project_generation) return 4;
  ItemValue snapshot{};
  if (!render_options::snapshot_item(options, snapshot)) return 4;
  const ItemRenderStackKey key{snapshot.item, snapshot.time, g_hooks.project_generation()};
  if (stack_contains(key)) { ++g_cycles_rejected; return 4; }
  StackScope scope(key);
  if (!scope.entered) return 4;
  StagedItemWorld stage{};
  if (snapshot_world(snapshot, stage)) {
    std::unique_ptr<ReceiptDraft> draft;
    if (transform(stage, snapshot, draft) != 0) return 4;
    return render_receipts::register_receipt(std::move(draft), receipt);
  }
  if (!g_hooks.synthetic_receipts_enabled || !g_hooks.synthetic_receipts_enabled() ||
      snapshot.matte == 2 || !g_hooks.publish_synthetic) return 4;
  const int32_t pixel_format = snapshot.world_type == 1 ? world_registry::kPixelFormatArgb32 :
      (snapshot.world_type == 2 ? world_registry::kPixelFormatArgb64 :
       (snapshot.world_type == 3 ? world_registry::kPixelFormatArgb128 : 0));
  return pixel_format ? g_hooks.publish_synthetic(pixel_format, receipt, &snapshot) : 4;
}

bool verify_recursion_guard(void* item, AegpTime time, void* options, Checkout checkout) {
  if (!item || !options || !checkout || !g_hooks.project_generation) return false;
  void* rejected = reinterpret_cast<void*>(1);
  const ItemRenderStackKey direct{item, time, g_hooks.project_generation()};
  StackScope outer(direct);
  if (!outer.entered || checkout(options, nullptr, nullptr, &rejected) == 0 || rejected) return false;
  StackScope nested_other({reinterpret_cast<void*>(0x7770), time, g_hooks.project_generation()});
  return nested_other.entered && stack_contains(direct);
}

Diagnostics diagnostics() noexcept {
  return {g_published.load(), g_cache_hits.load(), g_cache_misses.load(),
          g_cycles_rejected.load(), g_generation_invalidations.load(), g_evictions.load()};
}

}  // namespace aexcompat::aegp_staged_item_runtime
