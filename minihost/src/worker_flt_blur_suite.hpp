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

struct Suite1 {
  GaussianBlur gaussian_blur;
  BoxBlur box_blur;
  ComputeDirectionalBlurRadii compute_directional_blur_radii;
};

static_assert(sizeof(Suite1) == 3 * sizeof(void*));
static_assert(offsetof(Suite1, gaussian_blur) == 0);
static_assert(offsetof(Suite1, box_blur) == sizeof(void*));
static_assert(offsetof(Suite1, compute_directional_blur_radii) == 2 * sizeof(void*));

bool configure(const Hooks&) noexcept;
const Suite1* suite1() noexcept;
bool selftest();

}  // namespace aexcompat::flt_blur
