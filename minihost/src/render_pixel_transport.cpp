#include "render_pixel_transport.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>

namespace aexcompat::render_pixel_transport {

#if defined(AEXCOMPAT_RENDER_WORKER) || defined(AEXCOMPAT_SMART_WORKER)
void rgba8_to_argb(void* destination, const unsigned char* rgba,
                   int32_t pixel_bytes) {
  if (pixel_bytes == 4) {
    auto* pixel = static_cast<unsigned char*>(destination);
    pixel[0] = rgba[3]; pixel[1] = rgba[0]; pixel[2] = rgba[1]; pixel[3] = rgba[2];
  } else if (pixel_bytes == 8) {
    auto* pixel = static_cast<uint16_t*>(destination);
    pixel[0] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[3]) * 32768u + 127u) / 255u);
    pixel[1] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[0]) * 32768u + 127u) / 255u);
    pixel[2] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[1]) * 32768u + 127u) / 255u);
    pixel[3] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[2]) * 32768u + 127u) / 255u);
  } else {
    auto* pixel = static_cast<float*>(destination);
    pixel[0] = rgba[3] / 255.0f; pixel[1] = rgba[0] / 255.0f;
    pixel[2] = rgba[1] / 255.0f; pixel[3] = rgba[2] / 255.0f;
  }
}

void argb_to_rgba8(unsigned char* rgba, const void* source,
                   int32_t pixel_bytes) {
  if (pixel_bytes == 4) {
    const auto* pixel = static_cast<const unsigned char*>(source);
    rgba[0] = pixel[1]; rgba[1] = pixel[2]; rgba[2] = pixel[3]; rgba[3] = pixel[0];
  } else if (pixel_bytes == 8) {
    const auto* pixel = static_cast<const uint16_t*>(source);
    const auto channel = [](uint16_t value) {
      return static_cast<unsigned char>((std::min<uint32_t>(value, 32768u) * 255u + 16384u) / 32768u);
    };
    rgba[0] = channel(pixel[1]); rgba[1] = channel(pixel[2]);
    rgba[2] = channel(pixel[3]); rgba[3] = channel(pixel[0]);
  } else {
    const auto* pixel = static_cast<const float*>(source);
    const auto channel = [](float value) {
      if (!std::isfinite(value)) value = 0.0f;
      return static_cast<unsigned char>(std::lround(std::clamp(value, 0.0f, 1.0f) * 255.0f));
    };
    rgba[0] = channel(pixel[1]); rgba[1] = channel(pixel[2]);
    rgba[2] = channel(pixel[3]); rgba[3] = channel(pixel[0]);
  }
}

void argb_to_rgba_native(void* rgba, const void* source, int32_t pixel_bytes) {
  if (pixel_bytes == 4) {
    argb_to_rgba8(static_cast<unsigned char*>(rgba), source, pixel_bytes);
  } else if (pixel_bytes == 8) {
    const auto* pixel = static_cast<const uint16_t*>(source);
    auto* output = static_cast<uint16_t*>(rgba);
    output[0] = pixel[1]; output[1] = pixel[2]; output[2] = pixel[3]; output[3] = pixel[0];
  } else {
    const auto* pixel = static_cast<const float*>(source);
    auto* output = static_cast<float*>(rgba);
    output[0] = pixel[1]; output[1] = pixel[2]; output[2] = pixel[3]; output[3] = pixel[0];
  }
}
#endif

}  // namespace aexcompat::render_pixel_transport
