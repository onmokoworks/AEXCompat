#include "worker_aefx_ace_suite.hpp"

#include "worker_suite_registry.hpp"

#include <algorithm>
#include <array>
#include <cstdint>
#include <utility>

namespace aexcompat::aefx_ace {
namespace {

// What `record_unsupported_suite_call` already returns for a diagnosed slot,
// so a refused conversion and a refused slot reach the plug-in as the same
// error rather than two unrelated ones. This is not
// PF_Err_BAD_CALLBACK_PARAM (516).
constexpr int32_t kRefused = 4;
constexpr uint32_t kMaxChannel8 = 255;
constexpr uint32_t kMaxChannel16 = 32768;
constexpr std::size_t kPixel8Bytes = 4;
constexpr std::size_t kPixel16Bytes = 8;

// The same mapping `render_pixel_transport.cpp` uses between 8-bit and 16-bit
// worlds, so a plug-in that reaches pixels through this suite and one that
// reaches them through a converted world agree on the values.
uint16_t widen(unsigned char value) noexcept {
  return static_cast<uint16_t>(
      (static_cast<uint32_t>(value) * kMaxChannel16 + 127u) / kMaxChannel8);
}

unsigned char narrow(uint16_t value) noexcept {
  return static_cast<unsigned char>(
      (std::min<uint32_t>(value, kMaxChannel16) * kMaxChannel8 + 16384u) /
      kMaxChannel16);
}

// A caller may pass any pointer pair; the only defence available here is to
// reject the shapes that cannot be a valid conversion. Sizes differ per
// direction, so each caller passes its own span lengths.
bool valid_request(int32_t pixels, const void* source, void* destination,
                   std::size_t source_bytes, std::size_t destination_bytes,
                   const void* widened) noexcept {
  // Zero is a degenerate rect, not a malformed call: AE treats it as a
  // no-op, so refusing it would fail a plug-in that guards an empty region
  // this way. It converts nothing and still needs valid spans.
  if (pixels < 0 || pixels > kMaxPixelsPerCall) return false;
  if (!source || !destination) return false;
  // Compared as integers: relational comparison of pointers into unrelated
  // objects is unspecified, and the spans here belong to different
  // allocations by construction.
  const auto source_begin = reinterpret_cast<uintptr_t>(source);
  const auto destination_begin = reinterpret_cast<uintptr_t>(destination);
  // Only the side read or written as `uint16_t` needs alignment; the 8-bit
  // side is a byte span and requiring it there would refuse valid scanlines.
  if (reinterpret_cast<uintptr_t>(widened) % alignof(uint16_t) != 0)
    return false;
  // Overlapping spans would make the result depend on iteration order, and no
  // observed caller converts in place.
  return source_begin + source_bytes <= destination_begin ||
      destination_begin + destination_bytes <= source_begin;
}

// `high_quality` selects a sampling strategy in AE's colour engine. This host
// converts by range mapping in both cases, so it is accepted and ignored
// rather than refused: refusing the high-quality call would stop a plug-in
// that renders correctly at draft quality.
int32_t __cdecl to_working(int32_t pixels, bool /*high_quality*/,
                           const void* source, void* destination) {
  const auto count = static_cast<std::size_t>(pixels > 0 ? pixels : 0);
  if (!valid_request(pixels, source, destination, count * kPixel8Bytes,
                     count * kPixel16Bytes, destination))
    return kRefused;
  const auto* input = static_cast<const unsigned char*>(source);
  auto* output = static_cast<uint16_t*>(destination);
  for (std::size_t pixel = 0; pixel < count; ++pixel) {
    for (std::size_t channel = 0; channel < 4; ++channel)
      output[pixel * 4 + channel] = widen(input[pixel * kPixel8Bytes + channel]);
  }
  return 0;
}

int32_t __cdecl from_working(int32_t pixels, bool /*high_quality*/,
                             const void* source, void* destination) {
  const auto count = static_cast<std::size_t>(pixels > 0 ? pixels : 0);
  if (!valid_request(pixels, source, destination, count * kPixel16Bytes,
                     count * kPixel8Bytes, source))
    return kRefused;
  const auto* input = static_cast<const uint16_t*>(source);
  auto* output = static_cast<unsigned char*>(destination);
  for (std::size_t pixel = 0; pixel < count; ++pixel) {
    for (std::size_t channel = 0; channel < 4; ++channel)
      output[pixel * kPixel8Bytes + channel] = narrow(input[pixel * 4 + channel]);
  }
  return 0;
}

const Suite1& table() {
  static const Suite1 suite = [] {
    // Entry i of this array reports slot i, so slot 1 and the tail past the
    // two implementations each name the slot the caller actually reached.
    const auto& stubs = worker_runtime::unsupported_suite_slots<
        worker_runtime::UnsupportedSuiteId::aefx_ace_1, kSlotCount>();
    Suite1 built{&to_working, stubs[1], &from_working, {}};
    for (std::size_t slot = 3; slot < kSlotCount; ++slot)
      built.unsupported_tail[slot - 3] = stubs[slot];
    return built;
  }();
  return suite;
}

}  // namespace

const Suite1* suite1() noexcept { return &table(); }

bool selftest() {
  const Suite1* suite = suite1();
  if (!suite || !suite->to_working || !suite->from_working) return false;

  // Every slot the observation did not identify answers with the diagnosed
  // refusal instead of running, including when a caller invokes it with the
  // four arguments the identified slots take. The stubs must also be distinct
  // from each other, or a diagnostic would name the wrong slot.
  const auto* slots = reinterpret_cast<void* const*>(suite);
  unsigned char probe8[kPixel8Bytes]{};
  uint16_t probe16[4]{};
  for (std::size_t slot = 0; slot < kSlotCount; ++slot) {
    if (slot == 0 || slot == 2) continue;
    if (!slots[slot]) return false;
    const auto stub = reinterpret_cast<ConvertPixels>(slots[slot]);
    if (stub(1, true, probe8, probe16) != kRefused) return false;
    for (std::size_t earlier = 0; earlier < slot; ++earlier) {
      if (earlier == 0 || earlier == 2) continue;
      if (slots[earlier] == slots[slot]) return false;
    }
  }

  // A round trip must return every 8-bit value unchanged, or the caller's
  // scaling in the widened space would drift the image on its own.
  std::array<unsigned char, 256 * kPixel8Bytes> source{};
  for (std::size_t index = 0; index < source.size(); ++index)
    source[index] = static_cast<unsigned char>(index % 256);
  std::array<uint16_t, 256 * 4> working{};
  std::array<unsigned char, 256 * kPixel8Bytes> restored{};
  if (suite->to_working(256, true, source.data(), working.data()) != 0)
    return false;
  if (suite->from_working(256, true, working.data(), restored.data()) != 0)
    return false;
  if (source != restored) return false;

  // Draft quality converts identically: this host has no colour engine to
  // switch strategies in, and a caller that renders at both qualities must not
  // see the two disagree.
  std::array<uint16_t, 256 * 4> draft{};
  if (suite->to_working(256, false, source.data(), draft.data()) != 0)
    return false;
  if (draft != working) return false;

  // The widened range is AE's 0..32768, which is what the caller scales
  // against; 8-bit 255 must land exactly on the maximum.
  const unsigned char extremes[kPixel8Bytes] = {0, 255, 0, 255};
  uint16_t widened[4] = {1, 1, 1, 1};
  if (suite->to_working(1, false, extremes, widened) != 0) return false;
  if (widened[0] != 0 || widened[1] != kMaxChannel16 || widened[2] != 0 ||
      widened[3] != kMaxChannel16)
    return false;

  // Interior values scale against 32768 too, which the endpoints alone would
  // not show: 128 * 32768 / 255 rounds to 16448.
  const unsigned char midpoint[kPixel8Bytes] = {128, 128, 128, 128};
  uint16_t widened_midpoint[4]{};
  if (suite->to_working(1, true, midpoint, widened_midpoint) != 0) return false;
  for (uint16_t value : widened_midpoint)
    if (value != 16448) return false;

  // A value above the 16-bit maximum is clamped rather than wrapped.
  const uint16_t above_range[4] = {65535, 65535, 65535, 65535};
  unsigned char clamped[kPixel8Bytes] = {0, 0, 0, 0};
  if (suite->from_working(1, false, above_range, clamped) != 0) return false;
  for (unsigned char value : clamped)
    if (value != 255) return false;

  // Fail-closed shapes: counts outside the bound, null spans, and overlapping
  // spans are refused instead of read.
  unsigned char scratch[8 * kPixel8Bytes]{};
  uint16_t scratch16[8 * 4]{};

  // Zero converts nothing and succeeds; the bound itself is accepted, one
  // past it is not.
  if (suite->to_working(0, true, scratch, scratch16) != 0) return false;
  if (suite->from_working(0, true, scratch16, scratch) != 0) return false;

  const int32_t refused_counts[] = {-1, kMaxPixelsPerCall + 1};
  for (int32_t count : refused_counts) {
    if (suite->to_working(count, true, scratch, scratch16) != kRefused)
      return false;
    if (suite->from_working(count, true, scratch16, scratch) !=
        kRefused)
      return false;
  }
  if (suite->to_working(1, true, nullptr, scratch16) != kRefused)
    return false;
  if (suite->to_working(1, true, scratch, nullptr) != kRefused)
    return false;
  if (suite->from_working(1, true, nullptr, scratch) != kRefused)
    return false;
  if (suite->from_working(1, true, scratch16, nullptr) != kRefused)
    return false;
  // Both directions refuse an in-place conversion.
  if (suite->to_working(2, true, scratch, scratch) != kRefused)
    return false;
  if (suite->from_working(2, true, scratch16, scratch16) != kRefused)
    return false;

  // A refused call leaves the destination untouched.
  uint16_t untouched[4] = {7, 7, 7, 7};
  if (suite->to_working(-1, true, scratch, untouched) != kRefused)
    return false;
  for (uint16_t value : untouched)
    if (value != 7) return false;

  return true;
}

}  // namespace aexcompat::aefx_ace
