#pragma once

#include <cstdint>

namespace aexcompat::pf_gaussian_kernel {

// PF_UtilCallbacks.gaussian_kernel (PF_UtilCallbacks+0x50), the kernel
// generator the SDK documents in AE_EffectCB.h:
//
//   PF_Err (*gaussian_kernel)(PF_ProgPtr effect_ref, A_FpLong kRadius,
//                             PF_KernelFlags flags, A_FpLong multiplier,
//                             A_long *diameter, void *kernel);
//
// The values follow AE's own PF.dll `PF_GaussianKernel` (AE 2026, export
// ordinal 337, read in Ghidra for issue #1253) so a plug-in that convolves
// with the answer (Inner/Outer Key's edge blur builds a 1D NORMALIZED
// kernel of longs through it from RENDER) sees the weights AE gives it:
//
//   r = (short)ceil(kRadius); diameter = 2r + 1
//   1D (PF_KernelFlag_1D): one row; 2D: (2r+1) rows, y in [-r, r]
//   for x in [-r, r]: d = hypot(x, y) / (kRadius + 1)
//     g = d <= 1 ? 1 - (1 - exp(-2.378 d^2)) * 1.102 : 0     (PFp_GaussianValue)
//     v = g * 255 [* multiplier when it is not 1.0]
//     kernel[y][x] = clamp((int)(v + 0.5), 0, 255); sum += v (per stored entry)
//   PF_KernelFlag_NORMALIZED: every stored entry is then scaled by
//     (255 * entries) / sum, truncated toward zero, and NOT re-clamped
//     (a radius-1 1D kernel comes back as 192, 380, 192).
//
// The kernel is written as A_long (32-bit) values whatever the USE_CHAR /
// USE_FIXED bits say, matching PF.dll (the SDK header notes only USE_LONG is
// implemented). AE answers A_Err_PARAMETER (3) for a negative radius or a
// null diameter/kernel pointer and never touches the buffer then; this host
// additionally answers PF_Err_BAD_CALLBACK_PARAM (516) for a null effect_ref
// or a radius above the bound below (a 2D kernel of that radius is 8193 x 8193
// A_longs, 268 MB, already far past any plug-in's blur; the bound keeps a
// runaway radius from becoming a runaway write). Every non-zero answer is an
// argument refusal made before anything is written; the callback has no other
// failure mode.
inline constexpr int32_t kMaxRadius = 4096;

int32_t __cdecl gaussian_kernel(void* effect_ref, double radius, uint32_t flags,
                                double multiplier, int32_t* diameter, void* kernel);

}  // namespace aexcompat::pf_gaussian_kernel
