#pragma once

#include "worker_world_safety.hpp"

#include <cstddef>
#include <cstdint>

namespace aexcompat::flt_blur {

inline constexpr char kSuiteName[] = "FLT Blur Suite";
inline constexpr int32_t kSuiteVersion1 = 1;

inline constexpr int32_t kChannelAlpha = 0x01;
inline constexpr int32_t kChannelRed = 0x02;
inline constexpr int32_t kChannelGreen = 0x04;
inline constexpr int32_t kChannelBlue = 0x08;
inline constexpr int32_t kRepeatEdgePixels = 0x10;
inline constexpr int32_t kVertical = 0x20;
inline constexpr int32_t kHorizontal = 0x40;

using ResolveWorld = bool (*)(
    const void*, world_safety::DispatchWorldFormat&);
using AcquireSuite = int32_t (__cdecl *)(const char*, int32_t, const void**);
using ReleaseSuite = int32_t (__cdecl *)(const char*, int32_t);
// The foreign-operand admission hooks (issue #1069, the copy_world8 pattern
// from issue #1037): the declared-stride bounds check, the "registry already
// knows this reference" refusal, the "host allocated this pixel base" refusal,
// and the session's negotiated pixel format ("argb8"/"argb16"/"argb32f") as
// the format anchor when neither operand resolves.
using BoundedWorld = bool (*)(void*, int32_t, unsigned char*&, int32_t&,
                              int32_t&, int32_t&);
using WorldReferenceKnown = bool (*)(const void*);
using WorldPixelsOwned = bool (*)(void*);
using SessionPixelFormat = const char* (*)();

// ABI recovered from the four AE 2026 callers tracked by issue #737. Both
// worlds are borrowed for the duration of the call: source is read-only and
// destination is the only caller-owned storage written. The callbacks never
// retain either pointer. Invalid identity, world layout, format, flags,
// radius, iteration count, quality, or progress range returns
// PF_Err_BAD_CALLBACK_PARAM (4); allocation failure does the same and never
// reports success with a partially valid output.

struct Hooks {
  void* effect_ref{};
  ResolveWorld resolve_world{};
  AcquireSuite acquire_suite{};
  ReleaseSuite release_suite{};
  // Optional as a group: with any of the four unset the suite keeps the
  // registry-only resolution (foreign operands stay refused), so a caller that
  // wires only the original four hooks keeps the pre-#1069 behaviour.
  BoundedWorld bounded_world{};
  WorldReferenceKnown world_reference_known{};
  WorldPixelsOwned world_pixels_owned{};
  SessionPixelFormat session_pixel_format{};
};

using GaussianBlur = int32_t (__cdecl *)(
    void*, const void*, float, float, int32_t, int32_t, int32_t, int32_t,
    void*);
using BoxBlur = int32_t (__cdecl *)(
    void*, const void*, float, float, int32_t, int32_t, int32_t, int32_t,
    void*);
// Slot 2, recovered symbol-for-symbol from FLT.dll's
// FLT_ComputeDirectionalBlurRadii (issue #1093). Pure geometry, no worlds: it
// projects a directional-blur smear onto x and y. The two magnitude scales, the
// angle in degrees, and a shared magnitude go in; the two half-extents (rounded
// up) come out through the pointers. Returns 0 always in AE; the host adds a
// null-pointer guard.
using ComputeDirectionalBlurRadii = int32_t (__cdecl *)(
    double, double, double, double, int32_t*, int32_t*);
// Slot 3 (offset 0x18), FLT_DirectionalBlur, reached through the
// FLT_DirectionalBlurC wrapper (FLT.dll 0x1800325b0 -> 0x180030d00). The one
// observed caller is Directional Blur (DirectionalBlur.aex FUN_18000e040,
// issue #1145). ABI recovered from that call site and FLT.dll's own
// FUN_18002ebb0/FUN_180032690:
//   (effect_ref, quality, downsample_x, downsample_y, length,
//    direction_degrees, source_world, destination_world)
// The plug-in only calls this with length > 0 and after clearing the
// destination; a centered directional motion blur of `source` is written into
// `destination`. Bit depth of both worlds matches (FLT asserts it). AE's
// visible smear extent is length*downsample_x*|sin| on x and
// length*downsample_y*|cos| on y (FLT FUN_180032690 param_6[4]/[5]), the same
// sin->x / cos->y convention as slot 2. AE's internal normalization runs
// through the RenderGraph engine (RG_ExecuteGraph, FLT.dll FUN_180031a50),
// which is not reproducible outside AE; the host implements the observable
// contract (a directional box blur with transparent-black edges, matching AE's
// extent geometry) rather than that engine. Returns 0 on success; invalid
// identity, quality, length, downsample, world layout, format, or an extent
// that would exceed the tap bound returns PF_Err_BAD_CALLBACK_PARAM (4), never
// a partially valid output.
using DirectionalBlur = int32_t (__cdecl *)(
    void*, int32_t, double, double, double, double, const void*, void*);

struct Suite1 {
  GaussianBlur gaussian_blur;
  BoxBlur box_blur;
  ComputeDirectionalBlurRadii compute_directional_blur_radii;
  DirectionalBlur directional_blur;
};

static_assert(sizeof(Suite1) == 4 * sizeof(void*));
static_assert(offsetof(Suite1, gaussian_blur) == 0);
static_assert(offsetof(Suite1, box_blur) == sizeof(void*));
static_assert(offsetof(Suite1, compute_directional_blur_radii) == 2 * sizeof(void*));
static_assert(offsetof(Suite1, directional_blur) == 3 * sizeof(void*));

bool configure(const Hooks&) noexcept;
const Suite1* suite1() noexcept;
bool selftest();

}  // namespace aexcompat::flt_blur
