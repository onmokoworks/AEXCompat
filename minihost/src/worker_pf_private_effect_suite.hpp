#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::pf_private_effect {

inline constexpr char kSuiteName[] = "PF AE Private Effect Suite";
// `VideoFilterHost.dll`'s `RegisterPrivateEffectSuite` (0x1800443d0) registers
// one table pointer (0x1801aa030) three times, at versions 3, 5 and 6, through
// `SweetPeaSupport::RegisterSuite`. There is no version 4 and no per-version
// layout: the three versions are the same table (issue #1283).
inline constexpr int32_t kSuiteVersion3 = 3;
inline constexpr int32_t kSuiteVersion5 = 5;
inline constexpr int32_t kSuiteVersion6 = 6;

// AE's table is exactly ten function pointers: 0x1801aa030..0x1801aa078, with
// the `.rdata` string pool starting at 0x1801aa080. The ten `__FUNCTION__`
// strings that follow name the slots in order:
//
//   0  RegisterForIdleEvents              5  GetEffectName
//   1  UnRegisterForIdleEvents            6  PF_GetCurrentState_Async
//   2  HostUTF16ToMultibyteString         7  SetSequenceDataNeedsSerialization
//   3  HostZStringToUTF16String           8  GetEffectNodeID
//   4  PushSingleIdleEvent                9  GetEffectRef
//
// Slot 8 and slot 9 are anchored independently of the string order (each
// implementation `lea`s its own name string: 0x180042e39 -> 0x1801aa210 and
// 0x180043234 -> 0x1801aa2e0), and slot 2 is anchored by the caller: both
// `3D Camera Tracker.aex` and `Stabilizer.aex` load `[suite+0x10]` and call it
// with (const wchar_t*, 0x100, char*). Slots 5 and 6 are the one pair whose
// implementation addresses are not monotonic with the string order, so their
// assignment is positional only (recorded in #1291); nothing observed calls
// either.
//
// The stored table entries themselves were read out of `.rdata` for #1295 and
// are, in slot order: 0x180042640, 0x180042660, 0x180042680, 0x180042860,
// 0x180042b40, 0x180042da0, 0x180042a10, 0x180042dc0, 0x180042e00,
// 0x180043200. That order is *not* monotonic (slot 6 precedes slot 5), which
// is why the positional assignment above had to be read from the table rather
// than inferred from addresses.
//
// The published table is longer than AE's ten so that a caller reading past
// the tenth entry reaches a diagnosed stub rather than whatever `.rdata`
// follows. The count matches the slot probe's, so every slot the probe can
// observe is a slot this table answers.
inline constexpr std::size_t kSlotCount = 32;
inline constexpr std::size_t kHostSlotCount = 10;
inline constexpr std::size_t kUtf16ToMultibyteSlot = 2;
inline constexpr std::size_t kZStringToUtf16Slot = 3;
static_assert(kHostSlotCount < kSlotCount,
              "the published table has to be longer than AE's for the "
              "over-read to land on a diagnosed stub");
static_assert(kUtf16ToMultibyteSlot < kHostSlotCount);
static_assert(kZStringToUtf16Slot < kHostSlotCount);
static_assert(kZStringToUtf16Slot == kUtf16ToMultibyteSlot + 1,
              "the Suite layout below places the two implemented slots "
              "adjacently");
// The `--self-test-pf-private-effect-suite` route reports these as literals in
// worker_fixed_selftest_routing.cpp, and tests/ asserts them. Changing any of
// them has to change that string too - including the implemented-slot list,
// which is `[2,3]` there.
static_assert(kSlotCount == 32 && kHostSlotCount == 10 &&
                  kUtf16ToMultibyteSlot == 2 && kZStringToUtf16Slot == 3,
              "update the self-test route's metadata literals as well");

// Bounds for the implemented slots. Neither is AE's: the source is a bare
// pointer with no length, so a cap is the only way to stop an unterminated
// string from being walked off its allocation, and the destination length is
// the caller's own claim about a buffer this side cannot see. Both are far
// above the observed use (parameter names, `dest_bytes` = 0x100). The source
// cap counts UTF-16 code units for slot 2 and bytes for slot 3, matching what
// each slot's source pointer addresses.
inline constexpr int32_t kMaxSourceCharacters = 1 << 16;
inline constexpr int32_t kMaxDestinationBytes = 1 << 16;

// What `record_unsupported_suite_call` returns for a diagnosed slot, so a
// refused conversion and a refused slot reach the plug-in as the same error.
// AE's own table answers 0, 0x203 (unimplemented - what its slots 0 and 5
// return) or 0x200 (failure), so a caller that distinguishes those sees this
// host's 4 instead. It is the reason #1295's boundary reads as
// `frame_error:4` rather than AE's 0x200.
//
// Because the two are the same number, they are told apart by their
// diagnostics, not by their code: a reached stub is recorded in
// `unsupported_suite_calls` by `record_unsupported_suite_call`, and a refused
// conversion emits a `stage:callback_denied callback=... reason=...` marker
// like the host's other callback refusals. Neither is silent.
inline constexpr int32_t kRefused = 4;

using HostUtf16ToMultibyteString =
    int32_t (__cdecl *)(const wchar_t* source, int32_t destination_bytes,
                        char* destination);
// Slot 3, `HostZStringToUTF16String` (issue #1295). The middle argument is a
// **byte** count, not a character count, exactly as in slot 2: AE's
// implementation (VideoFilterHost.dll+0x42860) sign-extends it, computes
// `count / 2 - 1` as the character budget and `memcpy`s
// `min(chars * 2 + 2, count)` bytes into the destination.
using HostZStringToUtf16String =
    int32_t (__cdecl *)(const char* source, int32_t destination_bytes,
                        wchar_t* destination);

struct Suite {
  void* unsupported_head[kUtf16ToMultibyteSlot];
  HostUtf16ToMultibyteString utf16_to_multibyte;
  HostZStringToUtf16String zstring_to_utf16;
  void* unsupported_tail[kSlotCount - kZStringToUtf16Slot - 1];
};

static_assert(sizeof(Suite) == kSlotCount * sizeof(void*));
static_assert(offsetof(Suite, utf16_to_multibyte) ==
              kUtf16ToMultibyteSlot * sizeof(void*));
static_assert(offsetof(Suite, zstring_to_utf16) ==
              kZStringToUtf16Slot * sizeof(void*));

const Suite* suite() noexcept;
bool selftest();

}  // namespace aexcompat::pf_private_effect
