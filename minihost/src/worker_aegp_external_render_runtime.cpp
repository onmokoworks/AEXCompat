#include "worker_aegp_external_render_runtime.hpp"

#include "worker_render_receipts.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <atomic>
#include <cstring>
#include <memory>
#include <mutex>
#include <vector>

namespace aexcompat::aegp_external_render_runtime {
namespace {

using render_options::ItemValue;
using suite_abi::AegpRect;
using suite_abi::AegpTime;
using world_registry::PlatformWorldBacking;

struct ExternalRenderedFrame {
  ItemValue options{};
  AegpRect rendered_region{};
  uint32_t timestamp{};
  uint32_t ticks_to_render{};
  std::shared_ptr<PlatformWorldBacking> backing;
};

constexpr std::size_t kMaxExternalRenderCache = 16;
Hooks g_hooks{};
std::atomic<uint32_t> g_project_generation{1};
std::atomic<bool> g_timestamp_exhausted{};
std::mutex g_mutex;
std::vector<ExternalRenderedFrame> g_cache;
uint32_t g_frames_checked_in{};

bool same_options(const ItemValue& left, const ItemValue& right) {
  return left.item == right.item && left.time.value == right.time.value &&
      left.time.scale == right.time.scale && left.time_step.value == right.time_step.value &&
      left.time_step.scale == right.time_step.scale && left.field == right.field &&
      left.world_type == right.world_type && left.downsample_x == right.downsample_x &&
      left.downsample_y == right.downsample_y &&
      std::memcmp(&left.roi, &right.roi, sizeof(left.roi)) == 0 && left.matte == right.matte &&
      left.channel_order == right.channel_order &&
      left.render_guide_layers == right.render_guide_layers &&
      left.render_quality == right.render_quality;
}

void store_timestamp(void* output, uint32_t value) {
  std::memcpy(output, &value, sizeof(value));
}

bool read_timestamp(const void* timestamp, uint32_t& value) {
  if (!timestamp) return false;
  std::memcpy(&value, timestamp, sizeof(value));
  return value != 0;
}

}  // namespace

void configure(Hooks hooks) noexcept {
  g_hooks = hooks;
  render_receipts::configure_scene_generation_reader(&project_generation);
}

uint32_t project_generation() noexcept { return g_project_generation.load(); }

void bump_project_generation() noexcept {
  render_receipts::SceneGenerationMutationGuard publication_guard;
  uint32_t current = g_project_generation.load();
  for (;;) {
    const uint32_t next = current == UINT32_MAX ? UINT32_MAX : current + 1;
    if (next == current) {
      g_timestamp_exhausted.store(true);
      {
        std::lock_guard<std::mutex> lock(g_mutex);
        g_cache.clear();
      }
      render_receipts::invalidate_all_scene_generations(current);
      if (g_hooks.invalidate_staged_items) g_hooks.invalidate_staged_items();
      return;
    }
    if (g_project_generation.compare_exchange_weak(current, next)) {
      {
        std::lock_guard<std::mutex> lock(g_mutex);
        g_cache.clear();
      }
      render_receipts::invalidate_all_scene_generations(next);
      if (g_hooks.invalidate_staged_items) g_hooks.invalidate_staged_items();
      return;
    }
  }
}

bool cache_empty() noexcept {
  std::lock_guard<std::mutex> lock(g_mutex);
  return g_cache.empty();
}

Diagnostics diagnostics() noexcept {
  std::lock_guard<std::mutex> lock(g_mutex);
  return {g_frames_checked_in, g_cache.size(), g_timestamp_exhausted.load()};
}

int32_t publish_cached_receipt(const ItemValue& options, void** output, bool* cache_hit) {
  if (output) *output = nullptr;
  if (cache_hit) *cache_hit = false;
  if (!output || !cache_hit || g_timestamp_exhausted.load()) return 4;
  const uint32_t current_timestamp = g_project_generation.load();
  ItemValue cached_options{};
  AegpRect cached_region{};
  std::shared_ptr<PlatformWorldBacking> backing;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    const auto cached = std::find_if(g_cache.begin(), g_cache.end(), [&](const auto& frame) {
      return frame.timestamp == current_timestamp && same_options(frame.options, options);
    });
    if (cached == g_cache.end()) return 0;
    cached_options = cached->options;
    cached_region = cached->rendered_region;
    backing = cached->backing;
  }
  if (!backing || !backing->world.data || backing->world.width <= 0 ||
      backing->world.height <= 0 || backing->world.rowbytes <= 0) return 4;
  const int32_t type = world_registry::aegp_world_type_from_format(backing->pixel_format);
  const int32_t pixel_bytes = type == 1 ? 4 : (type == 2 ? 8 : (type == 3 ? 16 : 0));
  const uint64_t tight_rowbytes = static_cast<uint64_t>(backing->world.width) * pixel_bytes;
  const uint64_t bytes = tight_rowbytes * backing->world.height;
  if (!pixel_bytes || backing->world.rowbytes < tight_rowbytes || bytes == 0 ||
      bytes > render_receipts::kMaxReceiptBytes) return 4;
  std::unique_ptr<render_receipts::ReceiptDraft> receipt;
  try {
    receipt = std::make_unique<render_receipts::ReceiptDraft>();
    receipt->pixels.resize(static_cast<std::size_t>(bytes));
    std::lock_guard<std::mutex> pixels_lock(backing->pixels_mutex);
    for (int32_t y = 0; y < backing->world.height; ++y)
      std::memcpy(receipt->pixels.data() + static_cast<std::size_t>(y) * tight_rowbytes,
          static_cast<const std::byte*>(backing->world.data) +
              static_cast<std::size_t>(y) * backing->world.rowbytes,
          static_cast<std::size_t>(tight_rowbytes));
  } catch (...) { return 4; }
  if (current_timestamp != g_project_generation.load() || g_timestamp_exhausted.load()) return 4;
  receipt->pixel_format = backing->pixel_format;
  receipt->has_render_options = true;
  receipt->render_options = cached_options;
  receipt->rendered_region = cached_region;
  receipt->render_timestamp = current_timestamp;
  receipt->world.world_flags = type == 1 ? 0 : 1;
  receipt->world.data = receipt->pixels.data();
  receipt->world.rowbytes = static_cast<int32_t>(tight_rowbytes);
  receipt->world.width = backing->world.width;
  receipt->world.height = backing->world.height;
  receipt->world.extent_hint = {0, 0, backing->world.width, backing->world.height};
  receipt->world.pix_aspect_ratio = {1, 1};
  if (current_timestamp != g_project_generation.load() || g_timestamp_exhausted.load()) return 4;
  if (render_receipts::register_scene_receipt(
          std::move(receipt), current_timestamp, output) != 0)
    return 4;
  *cache_hit = true;
  return 0;
}

int32_t __cdecl timestamp(void* output) {
  if (!output || g_timestamp_exhausted.load()) return 4;
  store_timestamp(output, g_project_generation.load());
  return 0;
}

int32_t __cdecl changed(void* item, const void* start_raw, const void* duration_raw,
                        const void* timestamp_value, uint8_t* out) {
  if (out) *out = 0;
  uint32_t observed = 0;
  const auto* start = static_cast<const AegpTime*>(start_raw);
  const auto* duration = static_cast<const AegpTime*>(duration_raw);
  if (!out || !g_hooks.valid_item || !g_hooks.valid_item(item) || !start || !duration ||
      start->scale == 0 || duration->scale == 0 || duration->value < 0 ||
      !read_timestamp(timestamp_value, observed)) return 4;
  if (g_timestamp_exhausted.load()) { *out = 1; return 0; }
  *out = observed != g_project_generation.load();
  return 0;
}

int32_t __cdecl worthwhile(void* options, const void* timestamp_value, uint8_t* out) {
  if (out) *out = 0;
  uint32_t observed = 0;
  ItemValue snapshot{};
  if (!out || !render_options::snapshot_item(options, snapshot) ||
      !read_timestamp(timestamp_value, observed)) return 4;
  if (g_timestamp_exhausted.load() || observed != g_project_generation.load()) return 0;
  std::lock_guard<std::mutex> lock(g_mutex);
  *out = std::none_of(g_cache.begin(), g_cache.end(), [&](const auto& frame) {
    return frame.timestamp == observed && same_options(frame.options, snapshot);
  });
  return 0;
}

int32_t __cdecl checkin_rendered(void* options, const void* timestamp_value,
                                 uint32_t ticks_to_render, void* image) {
  ItemValue snapshot{};
  uint32_t observed = 0;
  if (!render_options::snapshot_item(options, snapshot) ||
      !read_timestamp(timestamp_value, observed) || g_timestamp_exhausted.load()) return 4;
  std::lock_guard<std::mutex> lock(g_mutex);
  if (observed != g_project_generation.load() || g_timestamp_exhausted.load()) return 4;
  std::shared_ptr<PlatformWorldBacking> backing;
  if (!world_registry::snapshot_platform_world(image, backing) ||
      world_registry::aegp_world_type_from_format(backing->pixel_format) != snapshot.world_type)
    return 4;
  const int32_t width = backing->world.width;
  const int32_t height = backing->world.height;
  AegpRect rendered_region{0, 0, width, height};
  if (snapshot.roi.left != 0 || snapshot.roi.top != 0 || snapshot.roi.right != 0 ||
      snapshot.roi.bottom != 0) {
    const auto ceil_div = [](int32_t value, int32_t divisor) {
      return value <= 0 ? 0 : (value + divisor - 1) / divisor;
    };
    rendered_region = {(std::max)(0, snapshot.roi.left / snapshot.downsample_x),
        (std::max)(0, snapshot.roi.top / snapshot.downsample_y),
        (std::min)(width, ceil_div(snapshot.roi.right, snapshot.downsample_x)),
        (std::min)(height, ceil_div(snapshot.roi.bottom, snapshot.downsample_y))};
    if (rendered_region.right < rendered_region.left ||
        rendered_region.bottom < rendered_region.top) return 4;
  }
  auto cached = std::find_if(g_cache.begin(), g_cache.end(), [&](const auto& frame) {
    return frame.timestamp == observed && same_options(frame.options, snapshot);
  });
  if (cached == g_cache.end() && g_cache.size() < kMaxExternalRenderCache) {
    try { g_cache.reserve(g_cache.size() + 1); } catch (...) { return 4; }
    cached = g_cache.end();
  }
  std::shared_ptr<PlatformWorldBacking> adopted;
  if (!world_registry::adopt_platform_world(image, adopted) || adopted != backing) return 4;
  const ExternalRenderedFrame frame{snapshot, rendered_region, observed, ticks_to_render,
                                    std::move(adopted)};
  if (cached != g_cache.end()) *cached = frame;
  else if (g_cache.size() >= kMaxExternalRenderCache) g_cache.front() = frame;
  else g_cache.push_back(frame);
  ++g_frames_checked_in;
  return 0;
}

}  // namespace aexcompat::aegp_external_render_runtime
