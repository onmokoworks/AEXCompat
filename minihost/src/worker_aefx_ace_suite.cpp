#include "worker_aefx_ace_suite.hpp"

#include "worker_suite_registry.hpp"

#include <algorithm>
#include <array>
#include <cstring>

namespace aexcompat::aefx_ace {
namespace {

constexpr int32_t kBadCallbackParam = 4;
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
                   std::size_t source_bytes,
                   std::size_t destination_bytes) noexcept {
  if (pixels <= 0 || pixels > kMaxPixelsPerCall) return false;
  if (!source || !destination) return false;
  const auto* source_begin = static_cast<const unsigned char*>(source);
  const auto* destination_begin = static_cast<const unsigned char*>(destination);
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
                     count * kPixel16Bytes))
    return kBadCallbackParam;
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
                     count * kPixel8Bytes))
    return kBadCallbackParam;
  const auto* input = static_cast<const uint16_t*>(source);
  auto* output = static_cast<unsigned char*>(destination);
  for (std::size_t pixel = 0; pixel < count; ++pixel) {
    for (std::size_t channel = 0; channel < 4; ++channel)
      output[pixel * kPixel8Bytes + channel] = narrow(input[pixel * 4 + channel]);
  }
  return 0;
}

const Suite1& table() {
  static const Suite1 suite{
      &to_working,
      reinterpret_cast<void*>(
          &worker_runtime::unsupported_suite_slot<
              worker_runtime::UnsupportedSuiteId::aefx_ace_1, 1>),
      &from_working};
  return suite;
}

}  // namespace

const Suite1* suite1() noexcept { return &table(); }

bool selftest() {
  const Suite1* suite = suite1();
  if (!suite || !suite->to_working || !suite->from_working ||
      !suite->unsupported_slot1)
    return false;

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
  const int32_t refused_counts[] = {0, -1, kMaxPixelsPerCall + 1};
  for (int32_t count : refused_counts) {
    if (suite->to_working(count, true, scratch, scratch16) != kBadCallbackParam)
      return false;
    if (suite->from_working(count, true, scratch16, scratch) !=
        kBadCallbackParam)
      return false;
  }
  if (suite->to_working(1, true, nullptr, scratch16) != kBadCallbackParam)
    return false;
  if (suite->to_working(1, true, scratch, nullptr) != kBadCallbackParam)
    return false;
  if (suite->from_working(1, true, nullptr, scratch) != kBadCallbackParam)
    return false;
  if (suite->from_working(1, true, scratch16, nullptr) != kBadCallbackParam)
    return false;
  // Both directions refuse an in-place conversion.
  if (suite->to_working(2, true, scratch, scratch) != kBadCallbackParam)
    return false;
  if (suite->from_working(2, true, scratch16, scratch16) != kBadCallbackParam)
    return false;

  // A refused call leaves the destination untouched.
  uint16_t untouched[4] = {7, 7, 7, 7};
  if (suite->to_working(-1, true, scratch, untouched) != kBadCallbackParam)
    return false;
  for (uint16_t value : untouched)
    if (value != 7) return false;

  return true;
}

}  // namespace aexcompat::aefx_ace
