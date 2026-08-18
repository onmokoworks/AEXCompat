#include "worker_pf_private_effect_suite.hpp"

#include "worker_suite_registry.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <cstring>
#include <cwchar>
#include <iostream>
#include <vector>

namespace aexcompat::pf_private_effect {
namespace {

// AE's own implementation (VideoFilterHost.dll+0x42680) builds a
// `std::wstring` from `wcslen(source)`, converts it through
// `MediaFoundation.dll!MF::UTF16ToCodePageMultiByte` (i.e. the platform code
// page), clamps the result to `destination_bytes - 1`, copies it, terminates
// it and returns 0.
//
// This host diverges twice, deliberately, and both divergences are visible to
// the AE-oracle work rather than hidden:
//
//   * It converts to **UTF-8**, not the platform code page. Neither encoding
//     reaches the worker's JSON report - `bounded_diagnostic_text` in
//     worker_report.cpp drops every byte outside 0x20..0x7e before a name is
//     written - so the choice is not observable there. It matters for the
//     buffer the plug-in goes on to use: UTF-8 represents every name losslessly,
//     while a code page substitutes for characters it does not have. The
//     observed callers pass ASCII, where the two agree. Losslessly is about
//     the untruncated conversion: the clamp below cuts on a byte, so a
//     truncated non-ASCII name can end in a partial UTF-8 sequence - AE's
//     clamp has the same property in its own encoding.
//   * It refuses a `destination_bytes` above `kMaxDestinationBytes`, where AE
//     accepts any size. The caller's claim about its own buffer is all either
//     side has, so this bounds a single write when that claim is nonsense; it
//     cannot make a lying caller safe.
//
// What it does *not* diverge on any more is truncation: like AE, a converted
// string longer than the buffer is clamped rather than refused. Refusing
// bought no safety - both write strictly inside the caller's claimed bound -
// and turned a name AE would merely shorten into a PARAMS_SETUP failure.
// The refusals below answer with the same code a diagnosed stub does, so
// without a marker they would be indistinguishable from "the plug-in called a
// slot this host does not implement" - and `unsupported_suite_calls` would
// stay empty while a host refusal went out as the plug-in's frame error. Same
// always-on marker the other host-callback refusals use.
int32_t refuse(const char* reason) {
  std::cerr << "stage:callback_denied callback=private_effect_utf16_to_multibyte"
               " reason=" << reason << "\n" << std::flush;
  return kRefused;
}

int32_t __cdecl utf16_to_multibyte(const wchar_t* source,
                                   int32_t destination_bytes,
                                   char* destination) {
  if (!source || !destination) return refuse("null_argument");
  if (destination_bytes <= 0) return refuse("destination_bytes_range");
  // Not AE's: AE takes any size. This bounds a single write when the caller's
  // claim about its own buffer is nonsense; it cannot make a lying caller safe.
  if (destination_bytes > kMaxDestinationBytes)
    return refuse("destination_bytes_over_cap");
  // The source is a bare pointer with no length. A bounded scan is the only
  // defence available: a string with no terminator in reach is refused rather
  // than walked off its allocation.
  const std::size_t characters =
      wcsnlen_s(source, static_cast<std::size_t>(kMaxSourceCharacters));
  if (characters >= static_cast<std::size_t>(kMaxSourceCharacters))
    return refuse("source_unterminated");
  if (characters == 0) {
    destination[0] = '\0';
    return 0;
  }
  // WideCharToMultiByte reports failure as 0, never negative, and 0 is not a
  // legal answer for a non-empty source - so this is the failure test, and a
  // refusal rather than an empty name reported as success.
  const int converted = WideCharToMultiByte(
      CP_UTF8, 0, source, static_cast<int>(characters), nullptr, 0, nullptr,
      nullptr);
  if (converted <= 0) {
    destination[0] = '\0';
    return refuse("conversion_failed");
  }
  std::vector<char> narrow(static_cast<std::size_t>(converted));
  if (WideCharToMultiByte(CP_UTF8, 0, source, static_cast<int>(characters),
                          narrow.data(), converted, nullptr, nullptr) !=
      converted) {
    destination[0] = '\0';
    return refuse("conversion_inconsistent");
  }
  // AE's clamp. The terminator is this side's responsibility either way: the
  // caller's next step is a bounded copy out of the buffer, which needs one.
  const std::size_t copied = std::min<std::size_t>(
      narrow.size(), static_cast<std::size_t>(destination_bytes) - 1);
  std::memcpy(destination, narrow.data(), copied);
  destination[copied] = '\0';
  return 0;
}

const Suite& table() {
  static const Suite suite = [] {
    // Entry i of this array reports slot i, so every slot the observation did
    // not implement names the slot the caller actually reached.
    const auto& stubs = worker_runtime::unsupported_suite_slots<
        worker_runtime::UnsupportedSuiteId::pf_ae_private_effect, kSlotCount>();
    Suite built{};
    for (std::size_t slot = 0; slot < kUtf16ToMultibyteSlot; ++slot)
      built.unsupported_head[slot] = stubs[slot];
    built.utf16_to_multibyte = &utf16_to_multibyte;
    for (std::size_t slot = kUtf16ToMultibyteSlot + 1; slot < kSlotCount; ++slot)
      built.unsupported_tail[slot - kUtf16ToMultibyteSlot - 1] = stubs[slot];
    return built;
  }();
  return suite;
}

}  // namespace

const Suite* suite() noexcept { return &table(); }

bool selftest() {
  const Suite* published = suite();
  if (!published || !published->utf16_to_multibyte) return false;

  // Every slot but the implemented one answers with the diagnosed refusal
  // instead of running, and the stubs are distinct from each other, or a
  // diagnostic would name the wrong slot.
  const auto* slots = reinterpret_cast<void* const*>(published);
  char scratch[64]{};
  for (std::size_t slot = 0; slot < kSlotCount; ++slot) {
    if (slot == kUtf16ToMultibyteSlot) continue;
    if (!slots[slot]) return false;
    const auto stub = reinterpret_cast<HostUtf16ToMultibyteString>(slots[slot]);
    if (stub(L"x", static_cast<int32_t>(sizeof(scratch)), scratch) != kRefused)
      return false;
    for (std::size_t earlier = 0; earlier < slot; ++earlier) {
      if (earlier == kUtf16ToMultibyteSlot) continue;
      if (slots[earlier] == slots[slot]) return false;
    }
  }

  // The conversion the callers use: a name in, a NUL-terminated narrow string
  // out at the length the caller's copy will find.
  char destination[256]{};
  destination[0] = 'z';
  if (published->utf16_to_multibyte(L"Analyze", sizeof(destination),
                                    destination) != 0)
    return false;
  if (std::strcmp(destination, "Analyze") != 0) return false;

  // An empty source is a valid name, not a malformed call.
  destination[0] = 'z';
  if (published->utf16_to_multibyte(L"", sizeof(destination), destination) != 0)
    return false;
  if (destination[0] != '\0') return false;

  // Non-ASCII goes out as UTF-8, which is what the report carries. U+00E9 is
  // two bytes there and one in every ANSI code page that has it, so this
  // distinguishes the two encodings.
  const wchar_t accented[] = {0x00e9, 0};
  destination[0] = 'z';
  if (published->utf16_to_multibyte(accented, sizeof(destination),
                                    destination) != 0)
    return false;
  if (static_cast<unsigned char>(destination[0]) != 0xc3 ||
      static_cast<unsigned char>(destination[1]) != 0xa9 ||
      destination[2] != '\0')
    return false;

  // The exact fit is accepted whole; one byte over is clamped and
  // terminated the way AE clamps it, not refused, and nothing is written
  // past the caller's claimed length (`tight[8]` is the sentinel).
  char exact[8]{};
  if (published->utf16_to_multibyte(L"1234567", sizeof(exact), exact) != 0)
    return false;
  if (std::strcmp(exact, "1234567") != 0) return false;
  char tight[9];
  std::memset(tight, 'z', sizeof(tight));
  if (published->utf16_to_multibyte(L"12345678", 8, tight) != 0)
    return false;
  if (std::strcmp(tight, "1234567") != 0) return false;
  if (tight[8] != 'z') return false;
  // The degenerate bound: one byte holds the terminator and nothing else.
  // This is where `destination_bytes - 1` is 0 on an unsigned type, which
  // is the arithmetic a later edit is likeliest to break.
  char single[2];
  std::memset(single, 'z', sizeof(single));
  if (published->utf16_to_multibyte(L"ab", 1, single) != 0) return false;
  if (single[0] != '\0' || single[1] != 'z') return false;

  if (published->utf16_to_multibyte(nullptr, sizeof(destination), destination) !=
      kRefused)
    return false;
  if (published->utf16_to_multibyte(L"x", sizeof(destination), nullptr) !=
      kRefused)
    return false;
  if (published->utf16_to_multibyte(L"x", 0, destination) != kRefused)
    return false;
  if (published->utf16_to_multibyte(L"x", -1, destination) != kRefused)
    return false;
  if (published->utf16_to_multibyte(L"x", kMaxDestinationBytes + 1,
                                    destination) != kRefused)
    return false;

  // A source with no terminator inside the bound is refused rather than
  // scanned past its end.
  std::vector<wchar_t> unterminated(
      static_cast<std::size_t>(kMaxSourceCharacters) + 1, L'x');
  if (published->utf16_to_multibyte(unterminated.data(), sizeof(destination),
                                    destination) != kRefused)
    return false;

  return true;
}

}  // namespace aexcompat::pf_private_effect
