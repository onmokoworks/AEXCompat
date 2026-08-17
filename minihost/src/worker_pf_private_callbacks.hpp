#pragma once

#include "worker_world_safety.hpp"

#include <cstdint>

// AE's private PF_UtilCallbacks.get_callback_addr ids, recovered from AE 2026
// (26.3) by hooking the live dispatcher (PF.dll+0x17c30) with Frida while
// AfterFX rendered Bulge / Compound Blur / CC Cross Blur / Matte Choker
// (issue #985) and reading the returned functions in Ghidra:
//
//   id -5 -> PF.dll `PFp_GaussianValue` (0x52f70), the same for every
//            (quality, mode): double(double), the falloff curve Bulge lerps with.
//   id -2 -> FLT.dll 0x311f0 when mode == 1 (PF_MF_Alpha_STRAIGHT), 0x30ff0
//            otherwise (mode 0 is PF_MF_Alpha_PREMUL; mode 2 answered the same
//            entry), the same for every quality: an in-place separable blur of
//            one PF_LayerDef through PF.dll's PF_BoxBlur1D / PF_GaussianBlur1D.
//            The two entry points differ only in the alpha type they hand the
//            blur nodes: 0x311f0 treats the world as straight alpha
//            (premultiplies, blurs, unpremultiplies), 0x30ff0 as premultiplied
//            (blurs the channels as they are).
//
// Signature of the id -2 function, from the three callers and FLT.dll:
//   PF_Err fn(PF_InData* in_data, void* unused, double radius,
//             void* unused_progress, int32_t flags, PF_LayerDef* world)
// `in_data->quality` and `in_data->effect_ref` are read out of the block; the
// second and fourth arguments are ignored by AE (Matte Choker passes an int*
// it accumulates itself in the fourth). `flags` are the FLT blur flags: 0x0f
// channel bits (A=1,R=2,G=4,B=8), 0x10 repeat edge pixels, 0x20 vertical,
// 0x40 horizontal, 0x100 never use the box approximation.
//
// Kernel selection (FLT.dll FUN_1800313f0), per axis: with quality 0 the
// radius is scaled by 1.4 and one box pass runs, with quality 1 three passes;
// rho = scale * radius / 2.71 (float). rho > 1 (and no 0x100) selects
// PF_BoxBlur1D with a per-pass box of half-width ceil(rho) whose two end taps
// weigh 1 - (ceil(rho) - rho) (kept in 1/1024 units); otherwise
// PF_GaussianBlur1D with integer weights w[0] = 255,
// w[i] = (int)(PFp_GaussianValue(i / (radius + 1)) * 255) for i <= ceil(radius).
// A radius of 0 builds no node and the call succeeds with the world untouched
// (CC Cross Blur at its default radius 0 called it twice with radius 0 and
// rendered its input in AE; the probe's own radius table starts at 0.1). A
// flags word naming neither axis is treated the same way; that case has no
// observed caller and is the host's reading of the same FLT.dll code, not a
// measurement.
// docs/PRIVATE_CALLBACK_IDS_OBSERVATION_2026-08-17.md records the captures.
namespace aexcompat::pf_private {

inline constexpr int32_t kBadCallbackParam = 516;  // PF_Err_BAD_CALLBACK_PARAM

inline constexpr int32_t kFlagAlpha = 0x01;
inline constexpr int32_t kFlagRed = 0x02;
inline constexpr int32_t kFlagGreen = 0x04;
inline constexpr int32_t kFlagBlue = 0x08;
inline constexpr int32_t kFlagChannels = 0x0f;
inline constexpr int32_t kFlagRepeatEdge = 0x10;
inline constexpr int32_t kFlagVertical = 0x20;
inline constexpr int32_t kFlagHorizontal = 0x40;
inline constexpr int32_t kFlagNoBox = 0x100;
inline constexpr int32_t kKnownFlags = kFlagChannels | kFlagRepeatEdge |
    kFlagVertical | kFlagHorizontal | kFlagNoBox;
inline constexpr float kMaximumRadius = 4096.0f;

// id -5: PFp_GaussianValue, byte-for-byte from PF.dll:
//   x > 1.0 -> 0.0, else 1.0 - (1.0 - exp((x * -2.378) * x)) * 1.102
double __cdecl gaussian_value(double x) noexcept;

// id -2. `mode == 1` (PF_MF_Alpha_STRAIGHT) of the get_callback_addr request
// selects the straight entry, anything else the premultiplied one.
int32_t __cdecl blur_straight(void* in_data, void* unused, double radius,
                              void* unused_progress, int32_t flags, void* world);
int32_t __cdecl blur_premultiplied(void* in_data, void* unused, double radius,
                                   void* unused_progress, int32_t flags,
                                   void* world);

// Resolves the one in-place operand (registered or foreign, the FLT blur
// suite's admission) into its dispatch format.
using ResolveWorld = bool (*)(void* world, world_safety::DispatchWorldFormat&);

struct Hooks {
  void* effect_ref{};
  ResolveWorld resolve_world{};
};

bool configure(const Hooks&) noexcept;

// The blur core on already-resolved pixels, for the self-test and for
// callers that hold a format: `straight` selects the 0x311f0 alpha handling.
// Returns 0 or kBadCallbackParam; `reason` names a refusal.
int32_t blur_resolved(const world_safety::DispatchWorldFormat& world,
                      float radius, int32_t flags, int32_t quality,
                      bool straight, const char*& reason);

}  // namespace aexcompat::pf_private
