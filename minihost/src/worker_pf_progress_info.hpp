#pragma once

#include <cstddef>
#include <cstdint>

// The object behind `PF_InData.effect_ref` (issue #1212, Echo / CannedWarp
// rows; PF.dll's own name for it is `PF_ProgressInfo`, from the exported
// signatures `PFp_Convolve<T>(PF_ProgressInfo*, ...)` and
// `PF_AreaSample_CPlusPlus<T,N>(PF_ProgressInfo*, ...)`).
//
// The SDK calls `effect_ref` an opaque `PF_ProgPtr` and reaches abort/progress
// only through `in_data->inter`. Adobe-bundled effects and PF.dll do not:
//
//   * PF.dll `FUN_180055960` (the body behind `PF_TransferRect_S`, reached from
//     the exported `PF_TransferRect` Echo imports) and `PFp_Convolve<T>` read
//     `effect_ref[0]` as a refcon and `effect_ref[2]` (+0x10) as
//     `PF_Err (*)(refcon, current, total)`, calling it once per row; a null
//     `effect_ref` selects a built-in no-op instead, a null slot does not
//     (PF.dll dereferences it without a check, so AE always fills it).
//   * CannedWarp's RENDER (`FUN_180001700`) calls the same +0x10 slot with the
//     same three arguments once per output row, from `in_data->effect_ref`.
//   * Echo's SMART_RENDER writes `effect_ref[2] = effect_ref[1]` (+0x10 <- +8)
//     for the duration of its PF_TransferRect loop and restores +0x10 after,
//     i.e. it replaces the progress slot with the +8 slot; PF.dll then calls
//     that +8 function with the progress arguments. So +8 is a function with
//     a compatible calling shape whose job Echo is happy to substitute for
//     progress reporting; the reading taken here is "the abort poll" (an
//     inference from Echo's use, not an observation of AE's own value: a
//     capture of AE's live effect_ref is what would name it).
//
// The host used to hand out a 4-byte tag object, so those reads landed in the
// worker's neighbouring statics: `+0x10` was whatever followed the tag
// (observed as `stage:selector_seh ... access=execute fault=null|heap_or_unknown`
// with the return address in Echo / CannedWarp / PF.dll), and Echo's write
// corrupted host memory. The handle is now a real object with the observed
// layout: the two slots forward to the host's own abort / progress callbacks
// (the ones `in_data->inter` carries), the refcon is the object itself so a
// forwarded call still names the effect the host owns, and every unobserved
// byte is zero.
//
// Identity checks across the host (`effect_ref == &g_effect`) are unchanged:
// this is the same single object, just with a layout.
namespace aexcompat::worker_runtime::pf_progress_info {

using AbortFn = int32_t(__cdecl*)(void* refcon);
using ProgressFn = int32_t(__cdecl*)(void* refcon, int32_t current, int32_t total);

struct alignas(16) EffectRefObject {
  void* refcon{};             // 0x00: first argument of the two callbacks
  AbortFn abort_fn{};         // 0x08: see the Echo note above
  ProgressFn progress_fn{};   // 0x10: PF.dll / CannedWarp progress poll
  std::byte reserved_18[0x80 - 0x18]{};
};
static_assert(offsetof(EffectRefObject, refcon) == 0x00);
static_assert(offsetof(EffectRefObject, abort_fn) == 0x08);
static_assert(offsetof(EffectRefObject, progress_fn) == 0x10);
static_assert(sizeof(EffectRefObject) == 0x80);

// Publishes the layout on `object` (refcon = &object, the two forwarding
// slots). Called before every plug-in bootstrap; a plug-in that overwrote a
// slot (Echo restores its own edit, a crashed one might not) gets the contract
// back at the next hand-out. Idempotent.
void publish(EffectRefObject& object) noexcept;

// True when `object` carries exactly the published layout.
bool published(const EffectRefObject& object) noexcept;

// Calls the two slots take (for the self-test and the report).
uint32_t abort_slot_calls() noexcept;
uint32_t progress_slot_calls() noexcept;

bool selftest();

}  // namespace aexcompat::worker_runtime::pf_progress_info
