#include "worker_pf_private_effect_suite.hpp"

#include "worker_suite_registry.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <cstdint>
#include <cstring>
#include <cwchar>
#include <iostream>
#include <string>
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
int32_t refuse(const char* callback, const char* reason) {
  std::cerr << "stage:callback_denied callback=" << callback
            << " reason=" << reason << "\n" << std::flush;
  return kRefused;
}

// The broker's denial parser caps a callback name at 32 bytes
// (`worker_denial_identifier` in `image_render/diagnostics.rs`) and drops the
// whole line - flagging `callback_denials_truncated` - when it is longer. The
// slot 2 name this refactor inherited was `private_effect_utf16_to_multibyte`,
// 33 bytes, so every slot 2 refusal had been dropped since #1283 and reached
// the report as an unexplained 4. Both names carry the shorter prefix now, and
// both fit (29 and 27). See #1303 for the remaining audit of names this host
// builds through a helper rather than as a literal.
int32_t refuse_utf16_to_multibyte(const char* reason) {
  return refuse("pf_private_utf16_to_multibyte", reason);
}

int32_t refuse_zstring_to_utf16(const char* reason) {
  return refuse("pf_private_zstring_to_utf16", reason);
}

int32_t __cdecl utf16_to_multibyte(const wchar_t* source,
                                   int32_t destination_bytes,
                                   char* destination) {
  if (!source || !destination) return refuse_utf16_to_multibyte("null_argument");
  if (destination_bytes <= 0) return refuse_utf16_to_multibyte("destination_bytes_range");
  // Not AE's: AE takes any size. This bounds a single write when the caller's
  // claim about its own buffer is nonsense; it cannot make a lying caller safe.
  if (destination_bytes > kMaxDestinationBytes)
    return refuse_utf16_to_multibyte("destination_bytes_over_cap");
  // The source is a bare pointer with no length. A bounded scan is the only
  // defence available: a string with no terminator in reach is refused rather
  // than walked off its allocation.
  const std::size_t characters =
      wcsnlen_s(source, static_cast<std::size_t>(kMaxSourceCharacters));
  if (characters >= static_cast<std::size_t>(kMaxSourceCharacters))
    return refuse_utf16_to_multibyte("source_unterminated");
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
    return refuse_utf16_to_multibyte("conversion_failed");
  }
  std::vector<char> narrow(static_cast<std::size_t>(converted));
  if (WideCharToMultiByte(CP_UTF8, 0, source, static_cast<int>(characters),
                          narrow.data(), converted, nullptr, nullptr) !=
      converted) {
    destination[0] = '\0';
    return refuse_utf16_to_multibyte("conversion_inconsistent");
  }
  // AE's clamp. The terminator is this side's responsibility either way: the
  // caller's next step is a bounded copy out of the buffer, which needs one.
  const std::size_t copied = std::min<std::size_t>(
      narrow.size(), static_cast<std::size_t>(destination_bytes) - 1);
  std::memcpy(destination, narrow.data(), copied);
  destination[copied] = '\0';
  return 0;
}

// Slot 3, `HostZStringToUTF16String` (issue #1295). Both
// `3D Camera Tracker.aex` and `Stabilizer.aex` reach it during SMART_RENDER
// and returned its diagnosed stub's 4 as their own frame error.
//
// AE (VideoFilterHost.dll+0x42860) branches on whether the source is a ZString:
// `0x180044440` tests `source.substr(0, 3) == "$$$"` and, when it is, the
// string goes to `dvacore::config::Localizer::Get()->GetLocalizedString`
// (vtable +0x10); otherwise it is converted verbatim through
// `MediaFoundation.dll!MF::CodePageMultiByteToUTF16`. Either way the result is
// then clamped into the caller's byte budget - see `copy_into_budget` below.
//
// The localized branch is the one the callers use, and it is the one this host
// has to match exactly, because AE's own installed implementation here is
// dvacore's `DummyLocalizerImpl` - which this host installs too (issue #1283).
// Its `GetLocalizedString` (dvacore+0xd8ca0) forwards to the exported
// `dvacore::GetNonLocalizedString` (dvacore+0x1056b0), a stateless string
// transform: `DummyLocalizerImpl::LoadDictionary` is a bare `ret`, so no
// dictionary is ever consulted and there is nothing to be "not localized"
// against.
//
// The verbatim branch is where this host diverges, in the same direction and
// for the same reason as slot 2: it decodes **UTF-8** rather than AE's code
// page, so the two slots round-trip through each other. AE's code page is
// `CP_ACP` unless the UI language is one of ja/ko/ru/uk/zh-CN/zh-TW (932 / 949
// / 866 / 866 / 936 / 950, selected by `MF::GetConversionCodePage` from
// `ASL::GetLanguageID`), and all three agree on ASCII, which is what the
// observed callers pass on this branch. A byte that is not valid UTF-8 becomes
// U+FFFD rather than a refusal - `MultiByteToWideChar` is called without
// `MB_ERR_INVALID_CHARS`, the same latitude AE's own lossy code-page conversion
// takes - so a divergence on this branch is silent rather than diagnosed.
bool zstring_is_localizable(const char* source, std::size_t length) {
  // AE's test is `substr(0, 3) == "$$$"`, which needs three bytes to compare;
  // a shorter string can never take the Localizer branch.
  return length >= 3 && std::memcmp(source, "$$$", 3) == 0;
}

// `dvacore::AsciiToUTF16` (dvacore+0x24bc80) widens with `MOVSX`, one byte at a
// time, so byte 0xE3 becomes U+FFE3 - not U+00E3, and not whatever a code page
// would say. Zero-extending here would look identical on ASCII and diverge on
// every other byte, which is exactly the kind of difference that only shows up
// against the AE oracle.
std::wstring sign_extended_widen(const char* source, std::size_t length) {
  std::wstring wide;
  wide.reserve(length);
  for (std::size_t index = 0; index < length; ++index)
    wide.push_back(static_cast<wchar_t>(
        static_cast<std::uint16_t>(static_cast<std::int16_t>(
            static_cast<std::int8_t>(source[index])))));
  return wide;
}

// `dvacore::GetNonLocalizedString`, transcribed. The whole source is widened
// first and the split happens on the wide copy, which is why the `$$$` marker
// survives on every path that does not split.
std::wstring non_localized_string(const char* source, std::size_t length) {
  std::wstring wide = sign_extended_widen(source, length);
  // All three conditions are re-tested there on the narrow string: strictly
  // longer than the marker (so `"$$$"` alone comes back untouched), the marker
  // itself, and an `=` somewhere in the source.
  if (length <= 3 || !zstring_is_localizable(source, length) ||
      std::memchr(source, '=', length) == nullptr)
    return wide;
  const auto equals = wide.find(L'=');
  if (equals == std::wstring::npos) return wide;
  // The first `=`, so a value may itself contain more of them.
  wide.erase(0, equals + 1);
  // A second delimiter, searched only inside the value: everything from
  // `#{comment}` on is dropped.
  const auto comment = wide.find(L"#{comment}");
  if (comment != std::wstring::npos) wide.resize(comment);
  return wide;
}

bool utf8_widen(const char* source, std::size_t length, std::wstring& wide) {
  if (length == 0) {
    wide.clear();
    return true;
  }
  // MultiByteToWideChar reports failure as 0 and 0 is not a legal answer for a
  // non-empty source, so this is the failure test.
  const int converted = MultiByteToWideChar(CP_UTF8, 0, source,
                                            static_cast<int>(length), nullptr, 0);
  if (converted <= 0) return false;
  wide.assign(static_cast<std::size_t>(converted), L'\0');
  return MultiByteToWideChar(CP_UTF8, 0, source, static_cast<int>(length),
                             wide.data(), converted) == converted;
}

// AE's clamp, transcribed from `0x180042935`: the byte budget buys
// `budget / 2 - 1` code units, the terminated run is `chars * 2 + 2` bytes, and
// the `memcpy` length is the smaller of that and the budget. The `min` is what
// makes an odd or 1-byte budget write a partial code unit rather than one byte
// past the caller's claim, and truncation is silent - AE returns 0 for it too.
// Truncation is by code unit, so a surrogate pair straddling the boundary is
// cut in half; AE cuts it in the same place.
void copy_into_budget(std::wstring& wide, int32_t destination_bytes,
                      wchar_t* destination) {
  const std::size_t budget = static_cast<std::size_t>(destination_bytes);
  const std::size_t max_characters = budget >= 2 ? budget / 2 - 1 : 0;
  if (wide.size() > max_characters) wide.resize(max_characters);
  // `c_str()` guarantees the terminator, so this run is `size() + 1` units.
  const std::size_t needed = (wide.size() + 1) * sizeof(wchar_t);
  std::memcpy(destination, wide.c_str(), (std::min)(needed, budget));
}

int32_t __cdecl zstring_to_utf16(const char* source, int32_t destination_bytes,
                                 wchar_t* destination) {
  // AE checks neither, and access-violates on a null source inside `strlen`.
  // Refusing is this host's standing divergence for a callback argument it
  // cannot make sense of.
  if (!source || !destination) return refuse_zstring_to_utf16("null_argument");
  if (destination_bytes <= 0)
    return refuse_zstring_to_utf16("destination_bytes_range");
  if (destination_bytes > kMaxDestinationBytes)
    return refuse_zstring_to_utf16("destination_bytes_over_cap");
  // The source is a bare pointer with no length; the cap counts bytes here.
  const std::size_t length =
      strnlen_s(source, static_cast<std::size_t>(kMaxSourceCharacters));
  if (length >= static_cast<std::size_t>(kMaxSourceCharacters))
    return refuse_zstring_to_utf16("source_unterminated");
  std::wstring wide;
  // The clamp is inside the `try` with the conversion: this is a `__cdecl`
  // callback AE calls directly, and an exception escaping into AE's frames is
  // undefined behaviour. `resize` only ever shrinks here, so nothing in the
  // clamp is expected to throw - the guard is about where the boundary is, not
  // about an expected failure.
  try {
    if (zstring_is_localizable(source, length)) {
      wide = non_localized_string(source, length);
    } else if (!utf8_widen(source, length, wide)) {
      // Through the same clamp as a success: a one-byte budget holds half a
      // terminator and writing the whole one would be a byte past the
      // caller's claim, which is exactly what this function must never do.
      wide.clear();
      copy_into_budget(wide, destination_bytes, destination);
      return refuse_zstring_to_utf16("conversion_failed");
    }
    copy_into_budget(wide, destination_bytes, destination);
  } catch (...) {
    // AE turns an escaped C++ exception here into 0x200; this host has one
    // code for a refused callback, and the marker above is what tells the two
    // apart.
    return refuse_zstring_to_utf16("conversion_threw");
  }
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
    built.zstring_to_utf16 = &zstring_to_utf16;
    for (std::size_t slot = kZStringToUtf16Slot + 1; slot < kSlotCount; ++slot)
      built.unsupported_tail[slot - kZStringToUtf16Slot - 1] = stubs[slot];
    return built;
  }();
  return suite;
}

}  // namespace

const Suite* suite() noexcept { return &table(); }

// Slot 3, checked against `dvacore::GetNonLocalizedString` and against AE's
// byte-budget clamp. `expected` is compared including its terminator, so a
// missing NUL fails.
static bool zstring_case(const Suite& published, const char* source,
                         const wchar_t* expected) {
  wchar_t destination[256];
  std::fill(std::begin(destination), std::end(destination), L'z');
  if (published.zstring_to_utf16(source, sizeof(destination), destination) != 0)
    return false;
  return std::wcscmp(destination, expected) == 0;
}

bool selftest() {
  const Suite* published = suite();
  if (!published || !published->utf16_to_multibyte || !published->zstring_to_utf16)
    return false;

  // Every slot but the implemented ones answers with the diagnosed refusal
  // instead of running, and the stubs are distinct from each other, or a
  // diagnostic would name the wrong slot.
  const auto* slots = reinterpret_cast<void* const*>(published);
  char scratch[64]{};
  for (std::size_t slot = 0; slot < kSlotCount; ++slot) {
    if (slot == kUtf16ToMultibyteSlot || slot == kZStringToUtf16Slot) continue;
    if (!slots[slot]) return false;
    const auto stub = reinterpret_cast<HostUtf16ToMultibyteString>(slots[slot]);
    if (stub(L"x", static_cast<int32_t>(sizeof(scratch)), scratch) != kRefused)
      return false;
    for (std::size_t earlier = 0; earlier < slot; ++earlier) {
      if (earlier == kUtf16ToMultibyteSlot || earlier == kZStringToUtf16Slot)
        continue;
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

  // ---- slot 3, `HostZStringToUTF16String` (issue #1295) ----
  //
  // The ZString convention `dvacore::GetNonLocalizedString` implements: the
  // default text is what follows the first `=`, and only a source strictly
  // longer than the marker with an `=` in it is split at all.
  if (!zstring_case(*published, "$$$/apd/analysis/parameter/name/x=Analyze",
                    L"Analyze"))
    return false;
  // The first `=`, so a value keeps any further ones.
  if (!zstring_case(*published, "$$$/x=a=b", L"a=b")) return false;
  // The marker alone is not stripped, and neither is a key with no `=`.
  if (!zstring_case(*published, "$$$", L"$$$")) return false;
  if (!zstring_case(*published, "$$$/A/B", L"$$$/A/B")) return false;
  // An empty default text is a default text.
  if (!zstring_case(*published, "$$$/x=", L"")) return false;
  // The second delimiter, and it is searched only inside the value.
  if (!zstring_case(*published, "$$$/x=hi#{comment}note", L"hi")) return false;
  if (!zstring_case(*published, "$$$/x=#{comment}hi", L"")) return false;
  if (!zstring_case(*published, "$$$/a#{comment}b=hi", L"hi")) return false;
  // Not a ZString: the verbatim branch, which leaves an `=` alone.
  if (!zstring_case(*published, "abc=def", L"abc=def")) return false;
  if (!zstring_case(*published, "", L"")) return false;

  // The widening the localized branch uses is `MOVSX`, not a code page and not
  // a zero-extension: byte 0xE3 becomes U+FFE3. This is the assertion that
  // fails if somebody "simplifies" it to `static_cast<wchar_t>(byte)`.
  const wchar_t sign_extended[] = {0xFFE3, 0x0041, 0};
  if (!zstring_case(*published, "$$$/x=\xE3\x41", sign_extended)) return false;

  // AE's byte-budget clamp, at the three boundaries the arithmetic turns on.
  // `budget / 2 - 1` code units fit, the copy is `min(chars * 2 + 2, budget)`
  // bytes, and nothing past the caller's claim is touched.
  {
    wchar_t exact_fit[4];
    std::fill(std::begin(exact_fit), std::end(exact_fit), L'z');
    if (published->zstring_to_utf16("$$$/x=abc", 8, exact_fit) != 0) return false;
    if (std::wcscmp(exact_fit, L"abc") != 0) return false;
  }
  {
    // One code unit too many: clamped and terminated, still 0, and the
    // sentinel past the budget is untouched.
    wchar_t tight[5];
    std::fill(std::begin(tight), std::end(tight), L'z');
    if (published->zstring_to_utf16("$$$/x=abcd", 8, tight) != 0) return false;
    if (std::wcscmp(tight, L"abc") != 0) return false;
    if (tight[4] != L'z') return false;
  }
  {
    // Two bytes hold the terminator and nothing else.
    wchar_t only_terminator[2];
    std::fill(std::begin(only_terminator), std::end(only_terminator), L'z');
    if (published->zstring_to_utf16("$$$/x=ab", 2, only_terminator) != 0)
      return false;
    if (only_terminator[0] != L'\0' || only_terminator[1] != L'z') return false;
  }
  {
    // One byte: AE writes half a code unit rather than the whole terminator,
    // and stays inside the caller's claim. Read back through a byte view so
    // the untouched sentinel after it is visible.
    alignas(wchar_t) unsigned char single[3] = {0xAA, 0xAA, 0xAA};
    auto* half = reinterpret_cast<wchar_t*>(single);
    if (published->zstring_to_utf16("$$$/x=ab", 1, half) != 0) return false;
    if (single[0] != 0x00 || single[1] != 0xAA || single[2] != 0xAA)
      return false;
  }

  wchar_t wide_destination[256]{};
  if (published->zstring_to_utf16(nullptr, sizeof(wide_destination),
                                  wide_destination) != kRefused)
    return false;
  if (published->zstring_to_utf16("x", sizeof(wide_destination), nullptr) !=
      kRefused)
    return false;
  if (published->zstring_to_utf16("x", 0, wide_destination) != kRefused)
    return false;
  if (published->zstring_to_utf16("x", -1, wide_destination) != kRefused)
    return false;
  if (published->zstring_to_utf16("x", kMaxDestinationBytes + 1,
                                  wide_destination) != kRefused)
    return false;
  {
    std::vector<char> unterminated_bytes(
        static_cast<std::size_t>(kMaxSourceCharacters) + 1, 'x');
    if (published->zstring_to_utf16(unterminated_bytes.data(),
                                    sizeof(wide_destination),
                                    wide_destination) != kRefused)
      return false;
  }

  return true;
}

}  // namespace aexcompat::pf_private_effect
