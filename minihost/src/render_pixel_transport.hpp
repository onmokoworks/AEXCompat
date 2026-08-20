#pragma once

#include <cstdint>

namespace aexcompat::render_pixel_transport {

void rgba8_to_argb(void* destination, const unsigned char* rgba,
                   int32_t pixel_bytes);
void argb_to_rgba8(unsigned char* rgba, const void* source,
                   int32_t pixel_bytes);
void argb_to_rgba_native(void* rgba, const void* source, int32_t pixel_bytes);

// Depth conversion between a host ARGB world and float32 ARGB (issue #1271).
// The Premiere GPU-filter route (xGPUFilterEntry, the VR family) renders 32f
// frames only; an 8/16bpc session hands it a widened copy of the input and
// takes the output narrowed back to the session depth. Per channel and linear
// (premultiplied stays premultiplied); AE's 16-bit maximum is 0x8000 = 32768.
//
// Widening: 8-bit `v / 255`, 16-bit `v / 32768`, float32 copied through.
// Narrowing: NaN -> 0, clamp to [0,1], round to nearest, scaled by the depth
// maximum (the same bounding fill_world8/16 apply when a float colour lands in
// an integer world); a float32 destination keeps the value as given.
// `pixel_bytes` is the world depth (4 / 8 / 16); anything else is a no-op.
void argb_to_argb32f(float* destination, const void* source, int32_t pixel_bytes);
void argb32f_to_argb(void* destination, const float* source, int32_t pixel_bytes);

// Self-test: every 8-bit and every 16-bit channel value survives the
// widen/narrow round trip exactly, out-of-range and non-finite floats narrow
// to the depth's bounds, and float32 passes through both directions unchanged.
bool verify_argb32f_depth_conversion();

}  // namespace aexcompat::render_pixel_transport
