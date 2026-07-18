#include "render_subsystem.h"

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <sstream>

namespace aexcompat::render {

int dispatch(RenderContext& context) {
  if (!context.request || !context.hooks.guarded_effect_main ||
      !context.hooks.cleanup || !context.hooks.dependencies_ready)
    return -1;

  // Module audit happens in the supplied guarded EffectMain path.  The flag is
  // retained here to make that dependency explicit at the translation-unit
  // boundary and to reject an unprepared audit request before any selector.
  if (context.module_audit_required && !context.hooks.dependencies_ready(context.request))
    return -2;
  if (!context.module_audit_required && !context.hooks.dependencies_ready(context.request))
    return -2;

  context.selector_started = true;
  context.primary_error = context.hooks.guarded_effect_main(context.request);

  // Cleanup is unconditional after selector admission.  The host hook owns
  // sequence/frame setdown, pre-render-data deletion, GPU setdown, suite
  // release, world unregistering, and automatic parameter checkins.
  context.cleanup_started = true;
  context.cleanup_error = context.hooks.cleanup(context.request);
  return context.primary_error != 0 ? context.primary_error : context.cleanup_error;
}

int prepare_image_request(const std::string& case_id, bool has_external_input,
                          int32_t external_width, int32_t external_height,
                          int32_t external_pixel_bytes, ImageRequest& request) {
  request.connected_map = case_id == "connected_map" || case_id == "inverted_map";
  request.partial_extent_hint = case_id == "partial_extent_hint";
  request.width = has_external_input ? external_width :
      (request.connected_map ? 11 :
       ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 13 : 16));
  request.height = has_external_input ? external_height :
      (request.connected_map ? 7 :
       ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 9 : 12));
  request.pixel_bytes = has_external_input ? external_pixel_bytes : 4;
  if (request.width <= 0 || request.height <= 0 || request.width > 4096 ||
      request.height > 4096 ||
      (request.pixel_bytes != 4 && request.pixel_bytes != 8 && request.pixel_bytes != 16))
    return -3;
  if (case_id != "default" && case_id != "identity" && case_id != "horizontal" &&
      case_id != "vertical_no_repeat" && case_id != "mixed" &&
      case_id != "amount_max" && case_id != "seed_max" && case_id != "mix_zero" &&
      case_id != "odd_dimensions" && case_id != "padded_stride" &&
      case_id != "inverted_map" && case_id != "connected_map" &&
      case_id != "request" && !request.partial_extent_hint)
    return -2;
  request.rowbytes = case_id == "padded_stride" ? 64 :
      request.width * request.pixel_bytes;
  return 0;
}

bool build_argb_input(const ImageRequest& request,
                      const std::vector<unsigned char>* external_rgba,
                      std::vector<unsigned char>& logical_argb,
                      unsigned char* strided_destination) {
  if (!strided_destination) return false;
  const std::size_t pixels = static_cast<std::size_t>(request.width) * request.height;
  if (external_rgba && external_rgba->size() != pixels * 4) return false;
  logical_argb.assign(pixels * request.pixel_bytes, 0);
  for (int32_t y = 0; y < request.height; ++y) {
    for (int32_t x = 0; x < request.width; ++x) {
      auto* argb = logical_argb.data() +
          (static_cast<std::size_t>(y) * request.width + x) * request.pixel_bytes;
      if (external_rgba) {
        const auto* rgba = external_rgba->data() +
            (static_cast<std::size_t>(y) * request.width + x) * 4;
        if (request.pixel_bytes == 4) {
          argb[0] = rgba[3]; argb[1] = rgba[0]; argb[2] = rgba[1]; argb[3] = rgba[2];
        } else if (request.pixel_bytes == 8) {
          auto* value = reinterpret_cast<uint16_t*>(argb);
          value[0] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[3]) * 32768u + 127u) / 255u);
          value[1] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[0]) * 32768u + 127u) / 255u);
          value[2] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[1]) * 32768u + 127u) / 255u);
          value[3] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[2]) * 32768u + 127u) / 255u);
        } else {
          auto* value = reinterpret_cast<float*>(argb);
          value[0] = rgba[3] / 255.0f; value[1] = rgba[0] / 255.0f;
          value[2] = rgba[1] / 255.0f; value[3] = rgba[2] / 255.0f;
        }
      } else {
        // The remaining bytes stay zero for the 16/32-bit default gradient,
        // exactly as the legacy request construction did.
        argb[0] = 255;
        argb[1] = static_cast<unsigned char>(x * 255 / (request.width - 1));
        argb[2] = static_cast<unsigned char>(y * 255 / (request.height - 1));
        argb[3] = static_cast<unsigned char>((x + y) * 255 /
                                              (request.width + request.height - 2));
      }
      std::memcpy(strided_destination + static_cast<std::size_t>(y) * request.rowbytes +
                      static_cast<std::size_t>(x) * request.pixel_bytes,
                  argb, request.pixel_bytes);
    }
  }
  return true;
}

namespace {
constexpr uint32_t kMaxWorldDumps = 32;
constexpr uint64_t kMaxWorldDumpBytes = 1ull << 30;

const char* world_dump_extension(int32_t pixel_bytes) {
  return pixel_bytes == 16 ? "rgba32f-le" :
      (pixel_bytes == 8 ? "rgba16le" : "rgba8");
}

uint32_t crc32_ieee(const unsigned char* data, std::size_t size) {
  static const auto table = [] {
    std::array<uint32_t, 256> built{};
    for (uint32_t index = 0; index < 256; ++index) {
      uint32_t value = index;
      for (int bit = 0; bit < 8; ++bit)
        value = (value >> 1) ^ ((value & 1u) ? 0xEDB88320u : 0u);
      built[index] = value;
    }
    return built;
  }();
  uint32_t crc = 0xFFFFFFFFu;
  for (std::size_t index = 0; index < size; ++index)
    crc = (crc >> 8) ^ table[(crc ^ data[index]) & 0xFFu];
  return crc ^ 0xFFFFFFFFu;
}
}  // namespace

void dump_world_snapshot(RenderTelemetry& telemetry, const std::string& stage,
                         const unsigned char* packed_argb, int32_t width,
                         int32_t height, int32_t pixel_bytes) {
  if (!telemetry.dump_directory || telemetry.dump_directory->empty() || !packed_argb ||
      width <= 0 || height <= 0 || !telemetry.dumps_written || !telemetry.dumps_skipped ||
      !telemetry.dump_bytes || !telemetry.hooks.argb_to_rgba_native) return;
  const uint64_t bytes = static_cast<uint64_t>(width) * height * pixel_bytes;
  if (*telemetry.dumps_written >= kMaxWorldDumps ||
      bytes > kMaxWorldDumpBytes - *telemetry.dump_bytes) {
    ++*telemetry.dumps_skipped;
    return;
  }
  std::vector<unsigned char> rgba(static_cast<std::size_t>(bytes));
  for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(width) * height; ++pixel)
    telemetry.hooks.argb_to_rgba_native(rgba.data() + pixel * pixel_bytes,
                                         packed_argb + pixel * pixel_bytes, pixel_bytes);
  char name[128];
  std::snprintf(name, sizeof(name), "%03u-%s-%dx%d.%s", *telemetry.dumps_written,
                stage.c_str(), width, height, world_dump_extension(pixel_bytes));
  std::ofstream file(*telemetry.dump_directory / name, std::ios::binary | std::ios::out);
  if (!file || !file.write(reinterpret_cast<const char*>(rgba.data()), rgba.size())) {
    ++*telemetry.dumps_skipped;
    return;
  }
  ++*telemetry.dumps_written;
  *telemetry.dump_bytes += bytes;
}

void record_output_checksum_detail(RenderTelemetry& telemetry,
                                   const unsigned char* rgba, int32_t width,
                                   int32_t height, int32_t pixel_bytes) {
  if (!telemetry.output_checksum_detail || !rgba || width <= 0 || height <= 0 ||
      !telemetry.output_row_crc32 || !telemetry.output_channel_sha256 ||
      !telemetry.hooks.sha256_bytes) return;
  const std::size_t row_bytes = static_cast<std::size_t>(width) * pixel_bytes;
  telemetry.output_row_crc32->clear();
  telemetry.output_row_crc32->reserve(static_cast<std::size_t>(height));
  for (int32_t row = 0; row < height; ++row)
    telemetry.output_row_crc32->push_back(crc32_ieee(rgba + row * row_bytes, row_bytes));
  const std::size_t sample_bytes = static_cast<std::size_t>(pixel_bytes) / 4;
  std::vector<unsigned char> plane(static_cast<std::size_t>(width) * height * sample_bytes);
  for (std::size_t channel = 0; channel < 4; ++channel) {
    for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(width) * height; ++pixel)
      std::memcpy(plane.data() + pixel * sample_bytes,
                  rgba + pixel * pixel_bytes + channel * sample_bytes, sample_bytes);
    (*telemetry.output_channel_sha256)[channel] =
        telemetry.hooks.sha256_bytes(plane.data(), plane.size());
  }
}

std::string world_debug_report_json(const RenderTelemetry& telemetry) {
  std::ostringstream json;
  const uint32_t written = telemetry.dumps_written ? *telemetry.dumps_written : 0;
  const uint32_t skipped = telemetry.dumps_skipped ? *telemetry.dumps_skipped : 0;
  const uint64_t bytes = telemetry.dump_bytes ? *telemetry.dump_bytes : 0;
  json << ",\"world_dumps_written\":" << written
       << ",\"world_dumps_skipped\":" << skipped
       << ",\"world_dump_bytes\":" << bytes;
  if (telemetry.output_checksum_detail && telemetry.output_row_crc32 &&
      telemetry.output_channel_sha256) {
    json << ",\"output_row_crc32\":[";
    for (std::size_t row = 0; row < telemetry.output_row_crc32->size(); ++row) {
      if (row) json << ',';
      char text[12];
      std::snprintf(text, sizeof(text), "\"%08x\"", (*telemetry.output_row_crc32)[row]);
      json << text;
    }
    json << "],\"output_channel_sha256\":[";
    for (std::size_t channel = 0; channel < 4; ++channel) {
      if (channel) json << ',';
      json << '"' << (*telemetry.output_channel_sha256)[channel] << '"';
    }
    json << ']';
  }
  return json.str();
}

}  // namespace aexcompat::render
