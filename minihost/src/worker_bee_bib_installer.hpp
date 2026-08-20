#pragma once

#include <windows.h>

#include <cstddef>
#include <cstdint>
#include <cstring>

// Whether a mapped BEE.dll is the build whose BIB resolver installer this host
// knows how to call (issues #1264, #1302).
//
// BEE.dll resolves the interfaces it draws with through a BIB proc-address
// resolver it keeps at BEE.dll+0x16a13e0. Nothing in a plug-in's closure
// installs it, and the only function that writes it is not exported, so the
// host calls that function at an image-relative address. This header is the
// decision about whether that call is safe to make; the glue that turns an
// HMODULE into a call lives in l2_main_support.inc.
//
// The decision is kept separate from the Win32 lookups so the refusal paths
// can be exercised without a real BEE.dll, following the same split (and the
// same reason) as worker_legacy_support_init.hpp: the whole safety argument
// for calling an unexported function lives in the refusals, and the one AE
// build a given machine has installed exercises none of them.
namespace aexcompat::worker_runtime::bee_bib {

// The installer entry, read from AE 2026's BEE.dll: the function's prologue
// plus the opcode and ModRM of the `cmp qword ptr [rip+disp32], imm8` that
// makes it idempotent (it returns 1 without touching anything when the
// resolver is already set).
//
//   48 89 5C 24 08    mov [rsp+8], rbx
//   57                push rdi
//   48 83 EC 40       sub rsp, 0x40
//   48 8B C1          mov rax, rcx
//   48 83 3D ...      cmp qword ptr [rip+disp32], imm8
inline constexpr unsigned char kInstallerEntry[] = {
    0x48, 0x89, 0x5C, 0x24, 0x08,  // mov [rsp+8], rbx
    0x57,                          // push rdi
    0x48, 0x83, 0xEC, 0x40,        // sub rsp, 0x40
    0x48, 0x8B, 0xC1,              // mov rax, rcx
    0x48, 0x83, 0x3D,              // cmp qword ptr [rip+disp32], imm8
};

// Where the `cmp` puts its operands: disp32 at +16..+19, imm8 at +20, and the
// instruction ends at +21 - which is the rip its displacement is relative to.
inline constexpr std::size_t kDisplacementOffset = 16;
inline constexpr std::size_t kImmediateOffset = 20;
inline constexpr std::size_t kInstructionEnd = 21;

inline constexpr std::size_t kInstallerOffset = 0xc568a0;
inline constexpr std::size_t kResolverWordOffset = 0x16a13e0;

// What the `cmp` must be reading if the two offsets above describe the same
// build. This is the whole point of decoding the displacement rather than
// byte-comparing it: on its own each offset is an unchecked constant, and
// requiring the instruction at the code offset to name the data offset is
// what ties them together, so an edit to one without the other is refused
// instead of silently reading whichever word now lives there.
inline constexpr std::int32_t kExpectedDisplacement = static_cast<std::int32_t>(
    kResolverWordOffset - kInstallerOffset - kInstructionEnd);

// A sanity bound on `e_lfanew`. The loader validated it for a mapped module,
// so this is not what keeps the read safe - the SEH the caller wraps this in
// is. It is here so a wild value produces a refusal instead of pointer
// arithmetic that lands somewhere plausible.
inline constexpr std::size_t kMaxHeaderSpan = 0x10000;

// That the displacement really does name the resolver word is arithmetic on
// three constants, so it is settled here rather than checked at runtime: a
// test for it could only ever restate its own inputs.
static_assert(kInstallerOffset + kInstructionEnd +
                      static_cast<std::size_t>(
                          static_cast<std::ptrdiff_t>(kExpectedDisplacement)) ==
                  kResolverWordOffset,
              "the expected displacement must name the resolver word");

enum class Check {
  // The build this was read from: `site` names where to call and what to read.
  Ok,
  NotAnImage,
  NotAmd64,
  // The offsets fall outside what this module actually maps.
  OutsideImage,
  // The bytes at the code offset are not the installer, or the instruction
  // there does not name the expected data offset.
  EntryMismatch,
  // Never returned by `inspect`; the caller's SEH handler reports it when
  // reading the image faulted. Deliberately not just "faulted": the caller
  // has a second fault to report - the call into the installer itself - and
  // those two are the outcomes this whole design exists to tell apart.
  ReadFaulted,
};

inline const char* check_text(Check check) noexcept {
  switch (check) {
    case Check::Ok:
      return "ok";
    case Check::NotAnImage:
      return "not_an_image";
    case Check::NotAmd64:
      return "not_amd64";
    case Check::OutsideImage:
      return "offset_outside_image";
    case Check::EntryMismatch:
      return "entry_mismatch";
    case Check::ReadFaulted:
      return "read_faulted";
  }
  return "unknown";
}

// Filled in as far as the inspection got. `image_size` and `timestamp` are set
// as soon as the NT headers are readable, so they identify the module that was
// refused as well as one that was accepted - a refusal that names no build
// cannot be tied afterwards to the build it was about.
struct Site {
  std::size_t installer_offset;
  std::size_t resolver_offset;
  std::uint32_t image_size;
  std::uint32_t timestamp;
};

// `available` is how many bytes past `image` the caller can vouch for. A
// caller holding a real mapping cannot know that up front - the answer is in
// the headers it is about to read - so it passes SIZE_MAX and relies on the
// SEH it wraps this in. A test passes the size of its buffer, which is what
// lets the refusal paths run against synthetic images.
inline Check inspect(const unsigned char* image, std::size_t available,
                     Site& site) noexcept {
  site = Site{};
  // `bytes <= available` is tested before the subtraction so it cannot
  // underflow. The null test is not redundant with it: the real caller passes
  // SIZE_MAX, where every extent looks readable and only this stops a null.
  const auto readable = [image, available](std::size_t offset,
                                           std::size_t bytes) {
    return image != nullptr && bytes <= available && offset <= available - bytes;
  };

  if (!readable(0, sizeof(IMAGE_DOS_HEADER))) return Check::NotAnImage;
  const auto* const dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(image);
  if (dos->e_magic != IMAGE_DOS_SIGNATURE) return Check::NotAnImage;

  const LONG raw_nt_offset = dos->e_lfanew;
  if (raw_nt_offset < 0) return Check::NotAnImage;
  const auto nt_offset = static_cast<std::size_t>(raw_nt_offset);
  if (nt_offset > kMaxHeaderSpan - sizeof(IMAGE_NT_HEADERS64))
    return Check::NotAnImage;
  if (!readable(nt_offset, sizeof(IMAGE_NT_HEADERS64)))
    return Check::NotAnImage;
  const auto* const nt =
      reinterpret_cast<const IMAGE_NT_HEADERS64*>(image + nt_offset);
  if (nt->Signature != IMAGE_NT_SIGNATURE) return Check::NotAnImage;

  site.image_size = nt->OptionalHeader.SizeOfImage;
  site.timestamp = nt->FileHeader.TimeDateStamp;
  if (nt->FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64)
    return Check::NotAmd64;

  if (kInstallerOffset + kInstructionEnd > site.image_size ||
      kResolverWordOffset + sizeof(void*) > site.image_size)
    return Check::OutsideImage;
  if (!readable(kInstallerOffset, kInstructionEnd) ||
      !readable(kResolverWordOffset, sizeof(void*)))
    return Check::OutsideImage;

  const unsigned char* const entry = image + kInstallerOffset;
  if (std::memcmp(entry, kInstallerEntry, sizeof(kInstallerEntry)) != 0)
    return Check::EntryMismatch;
  if (entry[kImmediateOffset] != 0) return Check::EntryMismatch;
  std::int32_t displacement = 0;
  std::memcpy(&displacement, entry + kDisplacementOffset, sizeof(displacement));
  if (displacement != kExpectedDisplacement) return Check::EntryMismatch;

  site.installer_offset = kInstallerOffset;
  site.resolver_offset = kResolverWordOffset;
  return Check::Ok;
}

}  // namespace aexcompat::worker_runtime::bee_bib
