// Self-test for the BEE.dll BIB-installer admission decision (issues #1264,
// #1302). The host calls a function BEE.dll does not export, at an
// image-relative address, and what makes that defensible is that it refuses
// every module that is not the build the behaviour was read from. A machine
// has exactly one AE installed per version, so a corpus run exercises the
// accept path and none of the refusals - which is where the whole safety
// argument lives. These build synthetic images instead.
//
// The glue in l2_main_support.inc that turns an HMODULE into a call, and the
// SEH that contains reading a foreign image, are exercised by the real worker
// (a ShapeBlur render, which is what fails without the install).
#include "worker_bee_bib_installer.hpp"

#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {

using aexcompat::worker_runtime::bee_bib::Check;
using aexcompat::worker_runtime::bee_bib::Site;
using aexcompat::worker_runtime::bee_bib::check_text;
using aexcompat::worker_runtime::bee_bib::inspect;
using aexcompat::worker_runtime::bee_bib::kDisplacementOffset;
using aexcompat::worker_runtime::bee_bib::kExpectedDisplacement;
using aexcompat::worker_runtime::bee_bib::kImmediateOffset;
using aexcompat::worker_runtime::bee_bib::kInstallerEntry;
using aexcompat::worker_runtime::bee_bib::kInstallerOffset;
using aexcompat::worker_runtime::bee_bib::kInstructionEnd;
using aexcompat::worker_runtime::bee_bib::kResolverWordOffset;

constexpr std::uint32_t kImageSize = 0x17d3000;
constexpr std::uint32_t kTimestamp = 0x6a2ae3d2;
constexpr std::size_t kNtOffset = 0x108;

// An image shaped the way a mapped BEE.dll is, accepted by default and spoiled
// one field at a time by the checks below. Only what the decision reads is
// filled in; everything else stays zero, which is what a mapped image's gaps
// look like anyway.
std::vector<unsigned char> good_image() {
  std::vector<unsigned char> image(kResolverWordOffset + 0x1000, 0);
  auto* const dos = reinterpret_cast<IMAGE_DOS_HEADER*>(image.data());
  dos->e_magic = IMAGE_DOS_SIGNATURE;
  dos->e_lfanew = static_cast<LONG>(kNtOffset);
  auto* const nt =
      reinterpret_cast<IMAGE_NT_HEADERS64*>(image.data() + kNtOffset);
  nt->Signature = IMAGE_NT_SIGNATURE;
  nt->FileHeader.Machine = IMAGE_FILE_MACHINE_AMD64;
  nt->FileHeader.TimeDateStamp = kTimestamp;
  nt->OptionalHeader.SizeOfImage = kImageSize;
  unsigned char* const entry = image.data() + kInstallerOffset;
  std::memcpy(entry, kInstallerEntry, sizeof(kInstallerEntry));
  const std::int32_t displacement = kExpectedDisplacement;
  std::memcpy(entry + kDisplacementOffset, &displacement,
              sizeof(displacement));
  entry[kImmediateOffset] = 0;
  return image;
}

struct Check_ {
  const char* label;
  Check expected;
  Check actual;
};

std::vector<Check_> checks;
// Conditions that are not a comparison of two `Check`s. Kept apart so a
// failure prints the condition's own name instead of a made-up expected/actual
// pair of enum spellings, which reads backwards.
std::vector<const char*> failed_conditions;
std::size_t condition_count = 0;

void expect(const char* label, Check expected,
            const std::vector<unsigned char>& image) {
  Site site{};
  checks.push_back({label, expected, inspect(image.data(), image.size(), site)});
}

// A refusal has to name the module it was about, or the trace it produces
// cannot be tied afterwards to the build that was refused - which is the
// reason the caller prints these fields on every outcome. Answers whether the
// refusal carried them, so the two groups (past the NT headers, and before
// them) can be pinned separately.
bool refusal_carries_identity(const std::vector<unsigned char>& image,
                              Check expected) {
  Site site{};
  return inspect(image.data(), image.size(), site) == expected &&
         site.image_size == kImageSize && site.timestamp == kTimestamp;
}

void expect_true(const char* label, bool condition) {
  ++condition_count;
  if (!condition) failed_conditions.push_back(label);
}

}  // namespace

int main() {
  // The build this was read from. `Site` names both offsets and carries the
  // identity the trace records.
  {
    const auto image = good_image();
    Site site{};
    const Check check = inspect(image.data(), image.size(), site);
    checks.push_back({"ae_2026_shaped_image_is_accepted", Check::Ok, check});
    expect_true("accepted_site_names_the_installer",
                site.installer_offset == kInstallerOffset);
    expect_true("accepted_site_names_the_resolver_word",
                site.resolver_offset == kResolverWordOffset);
    expect_true("accepted_site_carries_the_identity",
                site.image_size == kImageSize && site.timestamp == kTimestamp);
  }

  // Not an image at all, or one whose header walk cannot be trusted.
  // A real pointer with nothing behind it, so what refuses it is the bound and
  // not the null test - an empty vector would not separate the two, because
  // MSVC's `data()` returns null for one and the null test would short-circuit
  // first. The null test is the check below.
  expect("truncated_before_dos_header_is_refused", Check::NotAnImage,
         std::vector<unsigned char>(1, 0));
  {
    // The shape the worker actually calls this in: a real mapping's extent is
    // not known up front, so it vouches for everything and leans on its SEH.
    // With `available` that large every extent looks readable, and the null
    // test is the only thing between a null module handle and a dereference
    // of address zero - which the bounded case above cannot show.
    Site site{};
    checks.push_back({"null_image_is_refused_even_unbounded", Check::NotAnImage,
                      inspect(nullptr, SIZE_MAX, site)});
  }
  {
    auto image = good_image();
    reinterpret_cast<IMAGE_DOS_HEADER*>(image.data())->e_magic = 0;
    expect("wrong_dos_magic_is_refused", Check::NotAnImage, image);
  }
  {
    auto image = good_image();
    reinterpret_cast<IMAGE_DOS_HEADER*>(image.data())->e_lfanew = -1;
    expect("negative_e_lfanew_is_refused", Check::NotAnImage, image);
  }
  {
    auto image = good_image();
    // Past the header span this is willing to walk: a value that would
    // otherwise be a readable offset inside this buffer.
    reinterpret_cast<IMAGE_DOS_HEADER*>(image.data())->e_lfanew = 0x20000;
    expect("out_of_range_e_lfanew_is_refused", Check::NotAnImage, image);
  }
  {
    auto image = good_image();
    reinterpret_cast<IMAGE_NT_HEADERS64*>(image.data() + kNtOffset)->Signature =
        0;
    expect("wrong_nt_signature_is_refused", Check::NotAnImage, image);
  }
  {
    // Truncated before the NT headers are readable: refused rather than read.
    std::vector<unsigned char> image(sizeof(IMAGE_DOS_HEADER), 0);
    auto* const dos = reinterpret_cast<IMAGE_DOS_HEADER*>(image.data());
    dos->e_magic = IMAGE_DOS_SIGNATURE;
    dos->e_lfanew = static_cast<LONG>(kNtOffset);
    expect("unreadable_nt_headers_are_refused", Check::NotAnImage, image);
  }

  {
    auto image = good_image();
    reinterpret_cast<IMAGE_NT_HEADERS64*>(image.data() + kNtOffset)
        ->FileHeader.Machine = IMAGE_FILE_MACHINE_I386;
    expect("non_amd64_is_refused_as_such", Check::NotAmd64, image);
  }

  // AE 2024's shape: the module maps less than the offsets need. Refused
  // before anything at those offsets is read.
  {
    auto image = good_image();
    reinterpret_cast<IMAGE_NT_HEADERS64*>(image.data() + kNtOffset)
        ->OptionalHeader.SizeOfImage = 0x1571000;
    expect("image_too_small_for_the_offsets_is_refused", Check::OutsideImage,
           image);
  }
  {
    // SizeOfImage says the offsets are there but the caller can only vouch for
    // less. The bound the caller gives is honoured, not just the header's.
    auto image = good_image();
    Site site{};
    checks.push_back({"available_bound_is_honoured", Check::OutsideImage,
                      inspect(image.data(), kInstallerOffset + 4, site)});
  }

  // AE 2025's shape: the offsets are inside the image, but the code offset
  // holds a different function. The prologue is what catches it, and it is
  // caught before the resolver word is read - on that build the resolver
  // offset lands in .reloc and reads non-null, which would otherwise be
  // reported as "BEE already has a resolver".
  {
    auto image = good_image();
    static constexpr unsigned char kOtherFunction[] = {0x6d, 0xa0, 0x41, 0x8b,
                                                       0xcc, 0x48, 0x89, 0x4d};
    std::memcpy(image.data() + kInstallerOffset, kOtherFunction,
                sizeof(kOtherFunction));
    expect("a_different_function_at_the_offset_is_refused", Check::EntryMismatch,
           image);
  }
  {
    auto image = good_image();
    image[kInstallerOffset + sizeof(kInstallerEntry) - 1] = 0x3E;
    expect("a_changed_prologue_byte_is_refused", Check::EntryMismatch, image);
  }
  {
    // The `cmp` compares against something other than zero, so it is not the
    // null test the idempotence claim rests on.
    auto image = good_image();
    image[kInstallerOffset + kImmediateOffset] = 1;
    expect("a_non_zero_immediate_is_refused", Check::EntryMismatch, image);
  }
  {
    // The instruction names a different word than the one this reads back.
    // This is what ties the two hardcoded offsets to each other: change one
    // without the other and the module is refused instead of being read at an
    // offset that now holds something else.
    auto image = good_image();
    const std::int32_t displacement = kExpectedDisplacement + 8;
    std::memcpy(image.data() + kInstallerOffset + kDisplacementOffset,
                &displacement, sizeof(displacement));
    expect("a_displacement_naming_another_word_is_refused", Check::EntryMismatch,
           image);
  }
  // That the expected displacement names the resolver word is arithmetic on
  // three compile-time constants, so the header settles it with a
  // static_assert. A runtime check for it could only restate its own inputs.

  // Which refusals name the module they refused. Everything from the machine
  // check onwards does, because `inspect` records the identity as soon as the
  // NT headers are readable; a refusal that stops before that cannot, and the
  // caller's trace says so rather than implying a build of size zero.
  {
    auto image = good_image();
    reinterpret_cast<IMAGE_NT_HEADERS64*>(image.data() + kNtOffset)
        ->FileHeader.Machine = IMAGE_FILE_MACHINE_I386;
    expect_true("not_amd64_refusal_names_the_module",
                refusal_carries_identity(image, Check::NotAmd64));
  }
  {
    auto image = good_image();
    reinterpret_cast<IMAGE_NT_HEADERS64*>(image.data() + kNtOffset)
        ->OptionalHeader.SizeOfImage = 0x1571000;
    Site site{};
    expect_true(
        "outside_image_refusal_names_the_module",
        inspect(image.data(), image.size(), site) == Check::OutsideImage &&
            site.image_size == 0x1571000 && site.timestamp == kTimestamp);
  }
  {
    auto image = good_image();
    image[kInstallerOffset + kImmediateOffset] = 1;
    expect_true("entry_mismatch_refusal_names_the_module",
                refusal_carries_identity(image, Check::EntryMismatch));
  }
  {
    // The other side: a refusal that never reached the headers has nothing to
    // name, and reports zeros rather than a stale or invented identity. The
    // site is poisoned first, so this tests `inspect` clearing it and not the
    // test's own value-initialization.
    auto image = good_image();
    reinterpret_cast<IMAGE_DOS_HEADER*>(image.data())->e_magic = 0;
    Site site{};
    site.image_size = 0xdeadbeef;
    site.timestamp = 0xfeedface;
    site.installer_offset = 1;
    site.resolver_offset = 1;
    expect_true("not_an_image_refusal_names_nothing",
                inspect(image.data(), image.size(), site) ==
                        Check::NotAnImage &&
                    site.image_size == 0 && site.timestamp == 0 &&
                    site.installer_offset == 0 && site.resolver_offset == 0);
  }

  bool passed = true;
  std::string failures;
  const auto add = [&failures](const std::string& text) {
    if (!failures.empty()) failures += ",";
    failures += "\"" + text + "\"";
  };
  for (const auto& check : checks) {
    if (check.expected == check.actual) continue;
    passed = false;
    add(std::string(check.label) + ":expected=" + check_text(check.expected) +
        ":actual=" + check_text(check.actual));
  }
  for (const char* label : failed_conditions) {
    passed = false;
    add(std::string(label) + ":condition_did_not_hold");
  }
  std::printf(
      "{\"worker_bee_bib_installer_selftest\":\"%s\",\"checks\":%zu,"
      "\"failures\":[%s]}\n",
      passed ? "passed" : "failed", checks.size() + condition_count,
      failures.c_str());
  return passed ? 0 : 1;
}
