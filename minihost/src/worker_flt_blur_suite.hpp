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
};

using GaussianBlur = int32_t (__cdecl *)(
    void*, const void*, float, float, int32_t, int32_t, int32_t, int32_t,
    void*);
using BoxBlur = int32_t (__cdecl *)(
    void*, const void*, float, float, int32_t, int32_t, int32_t, int32_t,
    void*);

struct Suite1 {
  GaussianBlur gaussian_blur;
  BoxBlur box_blur;
};

static_assert(sizeof(Suite1) == 2 * sizeof(void*));
static_assert(offsetof(Suite1, gaussian_blur) == 0);
static_assert(offsetof(Suite1, box_blur) == sizeof(void*));

bool configure(const Hooks&) noexcept;
const Suite1* suite1() noexcept;
bool selftest();

}  // namespace aexcompat::flt_blur
