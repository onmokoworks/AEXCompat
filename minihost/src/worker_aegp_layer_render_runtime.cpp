#include "worker_aegp_layer_render_runtime.hpp"

#include "worker_render_receipts.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <cstring>
#include <limits>
#include <memory>
#include <utility>

namespace aexcompat::aegp_layer_render_runtime {
namespace {

using render_options::LayerEffectBoundary;
using render_options::LayerValue;
using render_receipts::ReceiptDraft;

Hooks g_hooks{};
thread_local Context g_context{};

}  // namespace

void configure(Hooks hooks) noexcept { g_hooks = hooks; }
Context& context() noexcept { return g_context; }
bool active() noexcept { return g_context.entry != nullptr; }
Context replace_context(Context next) {
  Context previous = std::move(g_context);
  g_context = std::move(next);
  return previous;
}
void clear_context() noexcept { g_context = {}; }

int32_t publish_from_context(const Context& context, const LayerValue& options,
                             void** receipt) {
  if (receipt) *receipt = nullptr;
  if (!receipt || !g_hooks.project_generation ||
      context.project_generation == 0 ||
      context.project_generation ==
          (std::numeric_limits<uint32_t>::max)() ||
      g_hooks.project_generation() != context.project_generation)
    return 4;
  const uint32_t receipt_generation = context.project_generation;
  const int32_t pixel_format = options.world_type == 1 ? world_registry::kPixelFormatArgb32 :
      (options.world_type == 2 ? world_registry::kPixelFormatArgb64 :
       (options.world_type == 3 ? world_registry::kPixelFormatArgb128 : 0));
  const int32_t pixel_bytes = options.world_type == 1 ? 4 :
      (options.world_type == 2 ? 8 : (options.world_type == 3 ? 16 : 0));
  const bool current_time = options.time.scale != 0 && context.time_scale != 0 &&
      static_cast<int64_t>(options.time.value) * context.time_scale ==
      static_cast<int64_t>(context.current_time) * options.time.scale;
  const bool wants_downstream = options.effect_boundary == LayerEffectBoundary::downstream;
  const bool wants_all = options.effect_boundary == LayerEffectBoundary::all &&
      context.all_effects_finalized;
  const auto* selected_pixels = wants_downstream ? context.downstream_argb :
      (wants_all ? context.all_effects_argb : context.source_argb);
  const int32_t selected_width = wants_downstream ? context.downstream_width :
      (wants_all ? context.all_effects_width : context.source_width);
  const int32_t selected_height = wants_downstream ? context.downstream_height :
      (wants_all ? context.all_effects_height : context.source_height);
  const int32_t source_pixel_bytes = wants_downstream ? context.downstream_pixel_bytes :
      (wants_all ? context.all_effects_pixel_bytes : context.pixel_bytes);
  if (!context.entry || !selected_pixels || pixel_format == 0 ||
      options.time_step.scale == 0 || options.time_step.value <= 0 || !current_time ||
      options.downsample_x <= 0 || options.downsample_y <= 0 || options.matte == 2 ||
      !g_hooks.effect_boundary_live || !g_hooks.effect_boundary_live(options) ||
      (wants_downstream && !context.downstream_finalized) || selected_width <= 0 ||
      selected_height <= 0 || selected_width > 4096 || selected_height > 4096) return 4;
  const uint64_t source_bytes = static_cast<uint64_t>(selected_width) *
      selected_height * source_pixel_bytes;
  const int32_t width = (selected_width + options.downsample_x - 1) / options.downsample_x;
  const int32_t height = (selected_height + options.downsample_y - 1) / options.downsample_y;
  const uint64_t bytes = static_cast<uint64_t>(width) * height * pixel_bytes;
  if ((source_pixel_bytes != 4 && source_pixel_bytes != 8 && source_pixel_bytes != 16) ||
      selected_pixels->size() != source_bytes || bytes == 0 ||
      bytes > render_receipts::kMaxReceiptBytes) return 4;
  std::unique_ptr<ReceiptDraft> loaded_receipt;
  try {
    loaded_receipt = std::make_unique<ReceiptDraft>();
    loaded_receipt->pixels.resize(static_cast<std::size_t>(bytes));
    for (int32_t y = 0; y < height; ++y) {
      const int32_t source_y = y * options.downsample_y;
      for (int32_t x = 0; x < width; ++x) {
        const int32_t source_x = x * options.downsample_x;
        const unsigned char* source = selected_pixels->data() +
            (static_cast<std::size_t>(source_y) * selected_width + source_x) * source_pixel_bytes;
        std::array<float, 4> channels{};
        if (source_pixel_bytes == 4) {
          for (int c = 0; c < 4; ++c) channels[c] = source[c] / 255.0f;
        } else if (source_pixel_bytes == 8) {
          const auto* values = reinterpret_cast<const uint16_t*>(source);
          for (int c = 0; c < 4; ++c) channels[c] = values[c] / 65535.0f;
        } else {
          std::memcpy(channels.data(), source, sizeof(channels));
        }
        if (options.matte == 1)
          for (int c = 1; c < 4; ++c) channels[c] *= channels[0];
        unsigned char* destination = reinterpret_cast<unsigned char*>(loaded_receipt->pixels.data()) +
            (static_cast<std::size_t>(y) * width + x) * pixel_bytes;
        if (pixel_bytes == 4) {
          for (int c = 0; c < 4; ++c)
            destination[c] = static_cast<unsigned char>(
                std::clamp(channels[c], 0.0f, 1.0f) * 255.0f + 0.5f);
        } else if (pixel_bytes == 8) {
          std::array<uint16_t, 4> values{};
          for (int c = 0; c < 4; ++c)
            values[c] = static_cast<uint16_t>(
                std::clamp(channels[c], 0.0f, 1.0f) * 65535.0f + 0.5f);
          std::memcpy(destination, values.data(), sizeof(values));
        } else {
          std::memcpy(destination, channels.data(), sizeof(channels));
        }
      }
    }
  } catch (...) { return 4; }
  loaded_receipt->pixel_format = pixel_format;
  loaded_receipt->rendered_region = {0, 0, width, height};
  loaded_receipt->world.data = loaded_receipt->pixels.data();
  loaded_receipt->world.rowbytes = width * pixel_bytes;
  loaded_receipt->world.world_flags = pixel_bytes == 4 ? 0 : 1;
  loaded_receipt->world.width = width;
  loaded_receipt->world.height = height;
  loaded_receipt->world.extent_hint = {0, 0, width, height};
  loaded_receipt->world.pix_aspect_ratio = {1, 1};
  const auto stage_kind =
      options.effect_boundary == LayerEffectBoundary::upstream
      ? aegp_staged_item_runtime::StageKind::upstream
      : (options.effect_boundary == LayerEffectBoundary::downstream
             ? aegp_staged_item_runtime::StageKind::downstream
             : aegp_staged_item_runtime::StageKind::all_effects);
  uint64_t stage_identity_hash = 0;
  void* const item = g_hooks.current_item ? g_hooks.current_item() : nullptr;
  if (!item ||
      (g_hooks.prepare_staged_item
           ? !g_hooks.prepare_staged_item(item)
           : !aegp_staged_item_runtime::has_item_registration(item)) ||
      !g_hooks.current_effect_instance || !g_hooks.publish_scheduler_stage)
    return 4;
  const uint64_t effect_instance =
      g_hooks.current_effect_instance(context, options);
  if (effect_instance == 0 ||
      !g_hooks.publish_scheduler_stage(item, stage_kind, effect_instance,
          options.time, options.time_step, 1, 0, pixel_format, width, height,
          width * pixel_bytes, loaded_receipt->pixels.data(),
          &stage_identity_hash, receipt_generation))
    return 4;
  loaded_receipt->has_stage_evidence = true;
  loaded_receipt->stage_identity_hash = stage_identity_hash;
  loaded_receipt->effect_instance = effect_instance;
  loaded_receipt->requested_time = options.time;
  loaded_receipt->source_time = options.time;
  loaded_receipt->stage_kind = static_cast<uint8_t>(stage_kind);
  loaded_receipt->sampling_policy =
      static_cast<uint8_t>(aegp_staged_item_runtime::SamplingPolicy::exact);
  return render_receipts::register_scene_receipt(
      std::move(loaded_receipt), receipt_generation, receipt);
}

int32_t publish(const LayerValue& options, void** receipt) {
  if (g_hooks.is_render_worker && g_hooks.is_render_worker())
    return publish_from_context(g_context, options, receipt);
  if (receipt) *receipt = nullptr;
  return 4;
}

bool capture_async_source(const LayerValue& options,
                          aegp_async_layer::SourceSnapshot& output) {
  const auto& context = g_context;
  const bool downstream = options.effect_boundary == LayerEffectBoundary::downstream;
  const bool all = options.effect_boundary == LayerEffectBoundary::all &&
      context.all_effects_finalized;
  const auto* pixels = downstream ? context.downstream_argb :
      (all ? context.all_effects_argb : context.source_argb);
  const int32_t width = downstream ? context.downstream_width :
      (all ? context.all_effects_width : context.source_width);
  const int32_t height = downstream ? context.downstream_height :
      (all ? context.all_effects_height : context.source_height);
  const int32_t pixel_bytes = downstream ? context.downstream_pixel_bytes :
      (all ? context.all_effects_pixel_bytes : context.pixel_bytes);
  if (!context.entry || !pixels || !g_hooks.project_generation ||
      context.project_generation == 0 ||
      g_hooks.project_generation() != context.project_generation ||
      (downstream && !context.downstream_finalized) ||
      width <= 0 || height <= 0 || pixel_bytes <= 0) return false;
  output.entry = reinterpret_cast<void*>(context.entry);
  output.pixel_bytes = pixel_bytes;
  output.width = width;
  output.height = height;
  output.current_time = context.current_time;
  output.time_scale = context.time_scale;
  output.project_generation = context.project_generation;
  output.pixels = *pixels;
  return true;
}

int32_t publish_async_source(const aegp_async_layer::SourceSnapshot& source,
                             const LayerValue& options, void** receipt) {
  Context context{};
  context.entry = reinterpret_cast<worker_runtime::EffectEntry>(source.entry);
  context.pixel_bytes = source.pixel_bytes;
  context.source_argb = &source.pixels;
  context.source_width = source.width;
  context.source_height = source.height;
  context.current_time = source.current_time;
  context.time_scale = source.time_scale;
  context.project_generation = source.project_generation;
  if (options.effect_boundary == LayerEffectBoundary::downstream) {
    context.downstream_argb = &source.pixels;
    context.downstream_width = source.width;
    context.downstream_height = source.height;
    context.downstream_pixel_bytes = source.pixel_bytes;
    context.downstream_finalized = true;
  } else if (options.effect_boundary == LayerEffectBoundary::all) {
    context.all_effects_argb = &source.pixels;
    context.all_effects_width = source.width;
    context.all_effects_height = source.height;
    context.all_effects_pixel_bytes = source.pixel_bytes;
    context.all_effects_finalized = true;
  }
  return publish_from_context(context, options, receipt);
}

}  // namespace aexcompat::aegp_layer_render_runtime
