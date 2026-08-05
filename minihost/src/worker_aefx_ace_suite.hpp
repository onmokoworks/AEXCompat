#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::aefx_ace {

inline constexpr char kSuiteName[] = "AEFX ACE Suite";
inline constexpr int32_t kSuiteVersion1 = 1;

// Recovered from `Photo Filter.aex` (AE 2026), the caller issue #776 tracked.
// It acquires the suite in RENDER, converts one packed `PF_Pixel8` (the filter
// colour parameter) and then every scanline of its input into a private
// buffer, scales the three colour channels there, and converts back into the
// output scanline. Both directions take the same shape:
//
//   int32_t (__cdecl *)(int32_t pixels, bool high_quality,
//                       const void* source, void* destination)
//
// The caller's first argument is the pixel count, the second is
// `in_data->quality == PF_Quality_HI` (in_data offset 0xc0 in the frozen ABI
// observation, `analysis/AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json`), and the
// working buffer it allocates is 8 bytes per pixel while its worlds are 4,
// which fixes the two formats: slot 0 widens `PF_Pixel8` to `PF_Pixel16` and
// slot 2 narrows it back. The caller reads the widened channels at byte
// offsets 2/4/6 and scales them against 32768, so the working representation
// is `PF_Pixel16` with AE's 0..32768 range, not a private encoding.
//
// The quality argument is one byte, not a word: the caller emits `sete dl`
// straight into the register, leaving the rest of EDX holding whatever was
// there before. Declaring it `int32_t` and validating against 0/1 rejects
// every real call, which is how this was found.
//
// Slot 1 was never called, so its signature is unknown; it is published as a
// diagnosed unsupported slot rather than guessed at.
//
// This host has no colour management, so both conversions are the range
// mapping the rest of the worker already uses between 8-bit and 16-bit worlds
// (`render_pixel_transport.cpp`). The conversion is therefore a depth change
// and not a working-space transform: an effect that depends on ACE performing
// a profile conversion will see AE-equivalence drift, which is recorded here
// rather than hidden behind an approximation.

// The suite takes raw spans and a caller-supplied count, with no world or
// effect identity to check them against, so the real length of either buffer
// is unknowable here: this bounds how much a wrong count can touch, it does
// not make the call safe. `from_working` writes into the caller's output
// scanline, which is host-owned world memory, and containment for a plug-in
// that lies about the count is the worker process and its Job Object. A call
// past the bound is refused rather than clamped.
inline constexpr int32_t kMaxPixelsPerCall = 1 << 20;

// The caller emits `sete dl`, which writes one byte, so the parameter must be
// one byte here too.
static_assert(sizeof(bool) == 1);

using ConvertPixels = int32_t (__cdecl *)(int32_t, bool, const void*, void*);

// Only slots 0 and 2 were observed, which bounds the table from below and not
// from above. Every slot past the two implementations is a diagnosed
// unsupported stub so that a caller reaching further produces a recorded
// diagnostic instead of an indirect call through whatever follows the table.
inline constexpr std::size_t kSlotCount = 16;

struct Suite1 {
  ConvertPixels to_working;
  void* unsupported_slot1;
  ConvertPixels from_working;
  void* unsupported_tail[kSlotCount - 3];
};

static_assert(sizeof(Suite1) == kSlotCount * sizeof(void*));
static_assert(offsetof(Suite1, to_working) == 0);
static_assert(offsetof(Suite1, unsupported_slot1) == sizeof(void*));
static_assert(offsetof(Suite1, from_working) == 2 * sizeof(void*));
static_assert(offsetof(Suite1, unsupported_tail) == 3 * sizeof(void*));

const Suite1* suite1() noexcept;
bool selftest();

}  // namespace aexcompat::aefx_ace
