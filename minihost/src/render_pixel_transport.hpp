#pragma once

#include <cstddef>
#include <cstdint>
#include <vector>

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

/// Brings a captured frame of `pixels` ARGB pixels to `pixel_bytes` in place,
/// through the float32 pair above.
///
/// Returns the depth the frame arrived at, or 0 when it did not arrive at a
/// depth this frame could have been rendered at - a capture invariant failure
/// for the caller to report, not something to convert. A frame already at
/// `pixel_bytes` is left untouched.
///
/// The admissible arrival depths are exactly two, and nothing else converts:
/// `dispatched_pixel_bytes`, the depth the plug-in was handed its worlds at
/// (`effect_bootstrap::dispatch_pixel_bytes`), and 16, because the GPU
/// negotiation transport (#1072) hands the plug-in float32 worlds whatever
/// depth was dispatched when it is entered by a case_id
/// (`gpu_opencl_float32` / `gpu_directx_float32`, which only the 32-bpc
/// `--smart-session32-opencl-v1` / `-directx-v1` commands carry) or by the
/// frame loop's `force_gpu_retry` - a 32-bpc OpenCL
/// session against a plug-in narrowed to 8 bits captures at 16. (Its automatic
/// entry looks at the dispatched depth, so that one only arrives at 16 when 16
/// was dispatched.)
/// (The Premiere GPU-filter route, #1271, narrows its own download to the
/// plan's depth before publishing, so it never arrives wide.) Admitting any
/// recognised stride instead would swallow the case this check exists for: a
/// render path that captured at a depth nobody dispatched is malformed
/// output, and widening it silently would report it as a good frame.
int32_t conform_pixel_depth(std::vector<unsigned char>& captured,
                            std::size_t pixels, int32_t pixel_bytes,
                            int32_t dispatched_pixel_bytes);

// Self-test: a frame already at the target depth is untouched (not round-
// tripped, which would clamp a 16-bit channel above 32768), widening and
// narrowing agree with the per-pixel pair, a ragged stride and a stride nobody
// dispatched are refused, and an empty frame is only accepted for zero pixels.
bool verify_pixel_depth_conform();

}  // namespace aexcompat::render_pixel_transport
