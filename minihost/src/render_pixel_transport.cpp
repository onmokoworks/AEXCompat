#include "render_pixel_transport.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <vector>

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

int32_t conform_pixel_depth(std::vector<unsigned char>& captured,
                            std::size_t pixels, int32_t pixel_bytes,
                            int32_t dispatched_pixel_bytes) {
  if (pixel_bytes != 4 && pixel_bytes != 8 && pixel_bytes != 16) return 0;
  if (dispatched_pixel_bytes != 4 && dispatched_pixel_bytes != 8 &&
      dispatched_pixel_bytes != 16) return 0;
  // No pixels means nothing to convert, and only an empty buffer is a whole
  // number of them. Answering `pixel_bytes` keeps "nothing happened" distinct
  // from the 0 that means "this buffer is not frames of pixels".
  if (pixels == 0) return captured.empty() ? pixel_bytes : 0;
  if (captured.size() % pixels != 0) return 0;
  const std::size_t stride = captured.size() / pixels;
  // Only the depth this frame was dispatched at, or the float32 the GPU
  // negotiation transport hands the plug-in whatever was dispatched when it is
  // entered by a gpu_*_float32 case_id or by force_gpu_retry (#1072). Any
  // other stride is malformed output.
  if (stride != static_cast<std::size_t>(dispatched_pixel_bytes) && stride != 16)
    return 0;
  const auto captured_pixel_bytes = static_cast<int32_t>(stride);
  if (captured_pixel_bytes == pixel_bytes) return captured_pixel_bytes;
  std::vector<unsigned char> conformed(pixels * static_cast<std::size_t>(pixel_bytes));
  for (std::size_t pixel = 0; pixel < pixels; ++pixel) {
    float wide[4]{};
    argb_to_argb32f(wide, captured.data() + pixel * stride, captured_pixel_bytes);
    argb32f_to_argb(conformed.data() + pixel * static_cast<std::size_t>(pixel_bytes),
                    wide, pixel_bytes);
  }
  captured.swap(conformed);
  return captured_pixel_bytes;
}

bool verify_pixel_depth_conform() {
  const std::vector<int32_t> depths{4, 8, 16};
  for (const int32_t from : depths) {
    for (const int32_t to : depths) {
      // Two pixels of known channels, converted as a frame and per pixel; the
      // frame conversion has to be the per-pixel pair applied twice and
      // nothing else. The 16-bit fixture carries a channel above AE's 0x8000
      // maximum so that a same-depth frame put through the float round trip
      // (which clamps it) is distinguishable from one left alone.
      std::vector<unsigned char> frame(2 * static_cast<std::size_t>(from));
      for (std::size_t index = 0; index < frame.size(); ++index)
        frame[index] = static_cast<unsigned char>(index * 7 + 1);
      if (from == 8) {
        const uint16_t above_maximum = 40000;
        std::memcpy(frame.data(), &above_maximum, sizeof(above_maximum));
      }
      std::vector<unsigned char> expected(2 * static_cast<std::size_t>(to));
      for (std::size_t pixel = 0; pixel < 2; ++pixel) {
        float wide[4]{};
        argb_to_argb32f(wide, frame.data() + pixel * from, from);
        argb32f_to_argb(expected.data() + pixel * to, wide, to);
      }
      std::vector<unsigned char> conformed = frame;
      if (conform_pixel_depth(conformed, 2, to, from) != from) return false;
      if (from == to) {
        if (conformed != frame) return false;  // untouched, not round-tripped
      } else if (conformed != expected) {
        return false;
      }
      // The float32 the GPU negotiation transport captures is admitted
      // whatever was dispatched, because its case_id and retry entries hand
      // the plug-in float32 worlds without looking at the dispatched depth
      // (#1072).
      if (from == 16) {
        std::vector<unsigned char> gpu = frame;
        if (conform_pixel_depth(gpu, 2, to, to) != 16) return false;
      }
    }
  }
  // Refusals: a stride that is not a whole pixel, a stride that is whole but
  // that nobody dispatched, a target that is not a depth, and a dispatched
  // depth that is not one either.
  std::vector<unsigned char> ragged(9);
  if (conform_pixel_depth(ragged, 2, 4, 4) != 0) return false;
  std::vector<unsigned char> unknown_stride(2 * 12);
  if (conform_pixel_depth(unknown_stride, 2, 4, 4) != 0) return false;
  std::vector<unsigned char> undispatched(2 * 4);
  if (conform_pixel_depth(undispatched, 2, 16, 8) != 0) return false;
  // The case the admission rule exists for, and the only one the caller's
  // size check cannot stand in for: a stride that matches the slot's depth
  // while matching nothing that was dispatched. Admitting any recognised
  // stride would let this through untouched and it would be packed into the
  // slot and reported as a good frame.
  std::vector<unsigned char> slot_sized_but_undispatched(2 * 8);
  if (conform_pixel_depth(slot_sized_but_undispatched, 2, 8, 4) != 0) return false;
  std::vector<unsigned char> fine(2 * 4);
  if (conform_pixel_depth(fine, 2, 5, 4) != 0) return false;
  if (conform_pixel_depth(fine, 2, 4, 5) != 0) return false;
  // A zero-pixel frame is only consistent with an empty buffer.
  std::vector<unsigned char> empty;
  if (conform_pixel_depth(empty, 0, 8, 8) != 8) return false;
  std::vector<unsigned char> not_empty(4);
  if (conform_pixel_depth(not_empty, 0, 8, 8) != 0) return false;
  // The refused buffers must come back untouched.
  return ragged.size() == 9 && unknown_stride.size() == 24 &&
      undispatched.size() == 8 && slot_sized_but_undispatched.size() == 16 &&
      not_empty.size() == 4;
}

}  // namespace aexcompat::render_pixel_transport
