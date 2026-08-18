#include "render_pixel_transport.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>

namespace aexcompat::render_pixel_transport {

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

void argb_to_argb32f(float* destination, const void* source,
                     int32_t pixel_bytes) {
  if (pixel_bytes == 4) {
    const auto* pixel = static_cast<const unsigned char*>(source);
    for (int channel = 0; channel < 4; ++channel)
      destination[channel] = pixel[channel] / 255.0f;
  } else if (pixel_bytes == 8) {
    const auto* pixel = static_cast<const uint16_t*>(source);
    for (int channel = 0; channel < 4; ++channel)
      destination[channel] = pixel[channel] / 32768.0f;
  } else if (pixel_bytes == 16) {
    std::memcpy(destination, source, sizeof(float) * 4);
  }
}

void argb32f_to_argb(void* destination, const float* source,
                     int32_t pixel_bytes) {
  // Same bounding as convert_argb_color (worker_pf_world_transform_runtime.cpp)
  // applies to a float colour landing in an integer world: NaN -> 0, then
  // clamp; +inf therefore narrows to the maximum and -inf to 0. (argb_to_rgba8
  // above predates both and sends every non-finite value to 0.)
  // NaN compares false against both clamp ends, so it would pass straight
  // through to lround; map it to 0 explicitly for a deterministic channel.
  const auto bounded = [](float value) {
    return value == value ? std::clamp(value, 0.0f, 1.0f) : 0.0f;
  };
  if (pixel_bytes == 4) {
    auto* pixel = static_cast<unsigned char*>(destination);
    for (int channel = 0; channel < 4; ++channel)
      pixel[channel] = static_cast<unsigned char>(
          std::lround(bounded(source[channel]) * 255.0f));
  } else if (pixel_bytes == 8) {
    auto* pixel = static_cast<uint16_t*>(destination);
    for (int channel = 0; channel < 4; ++channel)
      pixel[channel] = static_cast<uint16_t>(
          std::lround(bounded(source[channel]) * 32768.0f));
  } else if (pixel_bytes == 16) {
    std::memcpy(destination, source, sizeof(float) * 4);
  }
}

bool verify_argb32f_depth_conversion() {
  // Every 8-bit value round-trips exactly (v / 255 * 255 rounds back to v).
  for (uint32_t value = 0; value <= 255; ++value) {
    const unsigned char in[4] = {static_cast<unsigned char>(value),
                                 static_cast<unsigned char>(255 - value),
                                 static_cast<unsigned char>(value / 2), 255};
    float wide[4]{};
    unsigned char back[4]{};
    argb_to_argb32f(wide, in, 4);
    if (wide[0] < 0.0f || wide[0] > 1.0f || wide[3] != 1.0f) return false;
    argb32f_to_argb(back, wide, 4);
    if (std::memcmp(in, back, sizeof(in)) != 0) return false;
  }
  // Every 16-bit value in AE's 0..32768 range round-trips exactly (32768 is a
  // power of two, so v / 32768 is exact in float and scales back exactly).
  for (uint32_t value = 0; value <= 32768; ++value) {
    const uint16_t in[4] = {static_cast<uint16_t>(value),
                            static_cast<uint16_t>(32768 - value),
                            static_cast<uint16_t>(value / 3), 32768};
    float wide[4]{};
    uint16_t back[4]{};
    argb_to_argb32f(wide, in, 8);
    if (wide[0] < 0.0f || wide[0] > 1.0f || wide[3] != 1.0f) return false;
    argb32f_to_argb(back, wide, 8);
    if (std::memcmp(in, back, sizeof(in)) != 0) return false;
  }
  // 8-bit and 16-bit agree on the widened value of their respective maxima
  // and midpoints, so a session depth change is only a quantisation change.
  {
    const unsigned char mid8[4] = {255, 128, 0, 51};
    const uint16_t mid16[4] = {32768, 16384, 0, 6554};
    float wide8[4]{}, wide16[4]{};
    argb_to_argb32f(wide8, mid8, 4);
    argb_to_argb32f(wide16, mid16, 8);
    if (wide8[0] != 1.0f || wide16[0] != 1.0f) return false;
    // 8-bit has no exact midpoint: 128 / 255 sits one half-step above 0.5,
    // and 6554 / 32768 one part in 32768 above 51 / 255 = 0.2, so the bound
    // is one 8-bit step, not half of one.
    if (wide16[1] != 0.5f || std::fabs(wide8[1] - 0.5f) > 1.0f / 255.0f)
      return false;
    if (std::fabs(wide8[3] - wide16[3]) > 1.0f / 255.0f) return false;
  }
  // Narrowing bounds: over-range clamps to the maximum, negative to 0, NaN to
  // 0, and the midpoint rounds to nearest (127.5 -> 128, 0.5 * 32768 exact).
  {
    const float hdr[4] = {2.0f, -1.0f, std::numeric_limits<float>::quiet_NaN(),
                          0.5f};
    unsigned char out8[4]{};
    uint16_t out16[4]{};
    argb32f_to_argb(out8, hdr, 4);
    argb32f_to_argb(out16, hdr, 8);
    if (out8[0] != 255 || out8[1] != 0 || out8[2] != 0 || out8[3] != 128)
      return false;
    if (out16[0] != 32768 || out16[1] != 0 || out16[2] != 0 ||
        out16[3] != 16384)
      return false;
    if (std::numeric_limits<float>::has_infinity) {
      const float inf[4] = {std::numeric_limits<float>::infinity(),
                            -std::numeric_limits<float>::infinity(), 0.0f, 1.0f};
      argb32f_to_argb(out8, inf, 4);
      argb32f_to_argb(out16, inf, 8);
      if (out8[0] != 255 || out8[1] != 0 || out16[0] != 32768 || out16[1] != 0)
        return false;
    }
    // A float32 destination keeps HDR / negative values as given, both ways.
    float through[4]{};
    argb32f_to_argb(through, hdr, 16);
    if (through[0] != 2.0f || through[1] != -1.0f || through[3] != 0.5f ||
        through[2] == through[2])
      return false;
    float wide[4]{};
    argb_to_argb32f(wide, hdr, 16);
    if (wide[0] != 2.0f || wide[1] != -1.0f || wide[3] != 0.5f) return false;
  }
  // An unknown depth writes nothing.
  {
    const float value[4] = {1.0f, 1.0f, 1.0f, 1.0f};
    unsigned char untouched[16];
    std::memset(untouched, 0xCC, sizeof(untouched));
    argb32f_to_argb(untouched, value, 12);
    float wide[4] = {9.0f, 9.0f, 9.0f, 9.0f};
    argb_to_argb32f(wide, untouched, 12);
    for (unsigned char byte : untouched)
      if (byte != 0xCC) return false;
    for (float channel : wide)
      if (channel != 9.0f) return false;
  }
  return true;
}

}  // namespace aexcompat::render_pixel_transport
