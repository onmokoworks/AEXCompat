// Behavioral coverage for the packed ARGB8/RGBA8 transport used by the
// Classic and Smart render paths.  This deliberately exercises both the
// low-level permutation and build_argb_input, so a fast implementation cannot
// silently diverge from the worker's strided-world behavior.

#include "render_pixel_transport.hpp"
#include "render_subsystem.h"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

namespace {

constexpr unsigned char kGuard = 0xA7;
constexpr std::size_t kGuardBytes = 32;
constexpr std::array<std::size_t, 10> kPixelCounts{
    0, 1, 2, 3, 7, 15, 16, 17, 255, 1919};

int failures = 0;

void check(bool condition, const char* what) {
  if (condition) return;
  std::fprintf(stderr, "FAIL: %s\n", what);
  ++failures;
}

void check_case(bool condition, const char* what, std::size_t pixels,
                std::size_t source_offset, std::size_t destination_offset) {
  if (condition) return;
  std::fprintf(stderr,
               "FAIL: %s (pixels=%zu, source_offset=%zu, "
               "destination_offset=%zu)\n",
               what, pixels, source_offset, destination_offset);
  ++failures;
}

std::vector<unsigned char> make_rgba(std::size_t pixels) {
  std::vector<unsigned char> rgba(pixels * 4);
  for (std::size_t pixel = 0; pixel < pixels; ++pixel) {
    rgba[pixel * 4 + 0] = static_cast<unsigned char>(pixel * 17u + 3u);
    rgba[pixel * 4 + 1] = static_cast<unsigned char>(pixel * 29u + 5u);
    rgba[pixel * 4 + 2] = static_cast<unsigned char>(pixel * 43u + 7u);
    rgba[pixel * 4 + 3] = static_cast<unsigned char>(pixel * 61u + 11u);
  }
  return rgba;
}

std::vector<unsigned char> reference_rgba_to_argb(
    const std::vector<unsigned char>& rgba) {
  std::vector<unsigned char> argb(rgba.size());
  for (std::size_t offset = 0; offset < rgba.size(); offset += 4) {
    argb[offset + 0] = rgba[offset + 3];
    argb[offset + 1] = rgba[offset + 0];
    argb[offset + 2] = rgba[offset + 1];
    argb[offset + 3] = rgba[offset + 2];
  }
  return argb;
}

std::vector<unsigned char> reference_argb_to_rgba(
    const std::vector<unsigned char>& argb) {
  std::vector<unsigned char> rgba(argb.size());
  for (std::size_t offset = 0; offset < argb.size(); offset += 4) {
    rgba[offset + 0] = argb[offset + 1];
    rgba[offset + 1] = argb[offset + 2];
    rgba[offset + 2] = argb[offset + 3];
    rgba[offset + 3] = argb[offset + 0];
  }
  return rgba;
}

bool bytes_equal(const unsigned char* actual,
                 const std::vector<unsigned char>& expected) {
  return expected.empty() ||
      std::memcmp(actual, expected.data(), expected.size()) == 0;
}

bool outside_region_is(const std::vector<unsigned char>& bytes,
                       std::size_t begin, std::size_t end,
                       unsigned char expected) {
  for (std::size_t index = 0; index < bytes.size(); ++index) {
    if ((index < begin || index >= end) && bytes[index] != expected) return false;
  }
  return true;
}

void bulk_disjoint_offsets_and_guards() {
  using aexcompat::render_pixel_transport::argb8_to_rgba8_pixels;
  using aexcompat::render_pixel_transport::rgba8_to_argb8_pixels;

  // The zero-length contract permits null pointers and must not dereference
  // either side.
  rgba8_to_argb8_pixels(nullptr, nullptr, 0);
  argb8_to_rgba8_pixels(nullptr, nullptr, 0);

  for (const auto pixels : kPixelCounts) {
    const auto rgba = make_rgba(pixels);
    const auto expected_argb = reference_rgba_to_argb(rgba);
    for (std::size_t source_offset = 0; source_offset < 16; ++source_offset) {
      for (std::size_t destination_offset = 0; destination_offset < 16;
           ++destination_offset) {
        const std::size_t source_begin = kGuardBytes + source_offset;
        const std::size_t destination_begin = kGuardBytes + destination_offset;
        std::vector<unsigned char> source(
            source_begin + rgba.size() + kGuardBytes, kGuard);
        std::vector<unsigned char> destination(
            destination_begin + rgba.size() + kGuardBytes, kGuard);
        std::copy(rgba.begin(), rgba.end(), source.begin() + source_begin);
        const auto source_before = source;

        rgba8_to_argb8_pixels(destination.data() + destination_begin,
                             source.data() + source_begin, pixels);
        check_case(source == source_before, "forward conversion changed source",
                   pixels, source_offset, destination_offset);
        check_case(bytes_equal(destination.data() + destination_begin,
                               expected_argb),
                   "forward conversion did not match byte permutation", pixels,
                   source_offset, destination_offset);
        check_case(outside_region_is(destination, destination_begin,
                                     destination_begin + rgba.size(), kGuard),
                   "forward conversion wrote outside destination", pixels,
                   source_offset, destination_offset);

        std::vector<unsigned char> roundtrip(
            source_begin + rgba.size() + kGuardBytes, kGuard);
        argb8_to_rgba8_pixels(roundtrip.data() + source_begin,
                             destination.data() + destination_begin, pixels);
        check_case(bytes_equal(roundtrip.data() + source_begin, rgba),
                   "inverse conversion did not restore RGBA", pixels,
                   destination_offset, source_offset);
        check_case(outside_region_is(roundtrip, source_begin,
                                     source_begin + rgba.size(), kGuard),
                   "inverse conversion wrote outside destination", pixels,
                   destination_offset, source_offset);
      }
    }
  }
}

void bulk_exact_alias_roundtrips() {
  using aexcompat::render_pixel_transport::argb8_to_rgba8_pixels;
  using aexcompat::render_pixel_transport::rgba8_to_argb8_pixels;

  for (const auto pixels : kPixelCounts) {
    const auto rgba = make_rgba(pixels);
    const auto argb = reference_rgba_to_argb(rgba);
    for (std::size_t offset = 0; offset < 16; ++offset) {
      const std::size_t begin = kGuardBytes + offset;
      std::vector<unsigned char> forward(
          begin + rgba.size() + kGuardBytes, kGuard);
      std::copy(rgba.begin(), rgba.end(), forward.begin() + begin);
      rgba8_to_argb8_pixels(forward.data() + begin, forward.data() + begin,
                            pixels);
      check_case(bytes_equal(forward.data() + begin, argb),
                 "in-place forward conversion was not exact", pixels, offset,
                 offset);
      check_case(outside_region_is(forward, begin, begin + rgba.size(), kGuard),
                 "in-place forward conversion changed guards", pixels, offset,
                 offset);
      argb8_to_rgba8_pixels(forward.data() + begin, forward.data() + begin,
                            pixels);
      check_case(bytes_equal(forward.data() + begin, rgba),
                 "in-place forward/inverse roundtrip changed bytes", pixels,
                 offset, offset);

      std::vector<unsigned char> inverse(
          begin + argb.size() + kGuardBytes, kGuard);
      std::copy(argb.begin(), argb.end(), inverse.begin() + begin);
      argb8_to_rgba8_pixels(inverse.data() + begin, inverse.data() + begin,
                            pixels);
      check_case(bytes_equal(inverse.data() + begin, rgba),
                 "in-place inverse conversion was not exact", pixels, offset,
                 offset);
      rgba8_to_argb8_pixels(inverse.data() + begin, inverse.data() + begin,
                            pixels);
      check_case(bytes_equal(inverse.data() + begin, argb),
                 "in-place inverse/forward roundtrip changed bytes", pixels,
                 offset, offset);
      check_case(outside_region_is(inverse, begin, begin + argb.size(), kGuard),
                 "in-place inverse conversion changed guards", pixels, offset,
                 offset);
    }
  }
}

void every_channel_byte_value_survives() {
  using aexcompat::render_pixel_transport::argb8_to_rgba8_pixels;
  using aexcompat::render_pixel_transport::rgba8_to_argb8_pixels;

  std::vector<unsigned char> rgba(256 * 4);
  for (std::size_t value = 0; value < 256; ++value) {
    rgba[value * 4 + 0] = static_cast<unsigned char>(value);
    rgba[value * 4 + 1] = static_cast<unsigned char>(value * 73u + 19u);
    rgba[value * 4 + 2] = static_cast<unsigned char>(255u - value);
    rgba[value * 4 + 3] = static_cast<unsigned char>(value * 151u + 37u);
  }
  const auto expected_argb = reference_rgba_to_argb(rgba);
  std::vector<unsigned char> converted(rgba.size());
  rgba8_to_argb8_pixels(converted.data(), rgba.data(), 256);
  check(converted == expected_argb,
        "all 256 independent channel values must map to ARGB exactly");
  argb8_to_rgba8_pixels(converted.data(), converted.data(), 256);
  check(converted == rgba,
        "all 256 independent channel values must survive an in-place roundtrip");
}

bool strided_matches(const std::vector<unsigned char>& destination,
                     const std::vector<unsigned char>& packed,
                     int32_t width, int32_t height, int32_t pixel_bytes,
                     int32_t rowbytes) {
  const auto row_size = static_cast<std::size_t>(width) * pixel_bytes;
  for (int32_t y = 0; y < height; ++y) {
    const auto row = static_cast<std::size_t>(y) * rowbytes;
    if (std::memcmp(destination.data() + row,
                    packed.data() + static_cast<std::size_t>(y) * row_size,
                    row_size) != 0)
      return false;
    for (std::size_t offset = row_size;
         offset < static_cast<std::size_t>(rowbytes); ++offset) {
      if (destination[row + offset] != kGuard) return false;
    }
  }
  return true;
}

void build_argb8_case(int32_t rowbytes, const char* description) {
  constexpr int32_t width = 13;
  constexpr int32_t height = 9;
  const auto rgba = make_rgba(static_cast<std::size_t>(width) * height);
  const auto expected = reference_rgba_to_argb(rgba);
  aexcompat::render::ImageRequest request{};
  request.width = width;
  request.height = height;
  request.pixel_bytes = 4;
  request.rowbytes = rowbytes;
  std::vector<unsigned char> logical;
  std::vector<unsigned char> strided(
      static_cast<std::size_t>(rowbytes) * height, kGuard);
  const bool built = aexcompat::render::build_argb_input(
      request, &rgba, logical, strided.data());
  check(built, description);
  if (!built) return;
  check(logical == expected, "build_argb_input ARGB8 logical bytes drifted");
  check(strided_matches(strided, expected, width, height, 4, rowbytes),
        "build_argb_input ARGB8 striding or row padding drifted");
}

void classic_argb8_packed_and_padded_worlds() {
  build_argb8_case(13 * 4, "packed odd-dimension ARGB8 input must build");
  build_argb8_case(13 * 4 + 9,
                   "odd-stride padded ARGB8 input must build");
}

void store_u16(std::vector<unsigned char>& bytes, std::size_t offset,
               uint16_t value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

void store_float(std::vector<unsigned char>& bytes, std::size_t offset,
                 float value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

void unchanged_external_depth(int32_t pixel_bytes, int32_t padding) {
  constexpr int32_t width = 5;
  constexpr int32_t height = 3;
  const auto rgba = make_rgba(static_cast<std::size_t>(width) * height);
  const int32_t rowbytes = width * pixel_bytes + padding;
  aexcompat::render::ImageRequest request{};
  request.width = width;
  request.height = height;
  request.pixel_bytes = pixel_bytes;
  request.rowbytes = rowbytes;
  std::vector<unsigned char> logical;
  std::vector<unsigned char> strided(
      static_cast<std::size_t>(rowbytes) * height, kGuard);
  const bool built = aexcompat::render::build_argb_input(
      request, &rgba, logical, strided.data());
  check(built, "legacy 16/32-bit external input must build");
  if (!built) return;

  std::vector<unsigned char> expected(
      static_cast<std::size_t>(width) * height * pixel_bytes, 0);
  for (std::size_t pixel = 0; pixel < rgba.size() / 4; ++pixel) {
    const std::array<unsigned char, 4> argb{
        rgba[pixel * 4 + 3], rgba[pixel * 4 + 0],
        rgba[pixel * 4 + 1], rgba[pixel * 4 + 2]};
    for (std::size_t channel = 0; channel < 4; ++channel) {
      const auto offset = pixel * pixel_bytes + channel * (pixel_bytes / 4);
      if (pixel_bytes == 8) {
        const auto value = static_cast<uint16_t>(
            (static_cast<uint32_t>(argb[channel]) * 32768u + 127u) / 255u);
        store_u16(expected, offset, value);
      } else {
        store_float(expected, offset, argb[channel] / 255.0f);
      }
    }
  }
  check(logical == expected,
        "legacy 16/32-bit external-input channel formula drifted");
  check(strided_matches(strided, expected, width, height, pixel_bytes, rowbytes),
        "legacy 16/32-bit striding or row padding drifted");
}

void unchanged_null_gradient(int32_t pixel_bytes, int32_t padding) {
  constexpr int32_t width = 5;
  constexpr int32_t height = 3;
  const int32_t rowbytes = width * pixel_bytes + padding;
  aexcompat::render::ImageRequest request{};
  request.width = width;
  request.height = height;
  request.pixel_bytes = pixel_bytes;
  request.rowbytes = rowbytes;
  std::vector<unsigned char> logical;
  std::vector<unsigned char> strided(
      static_cast<std::size_t>(rowbytes) * height, kGuard);
  const bool built = aexcompat::render::build_argb_input(
      request, nullptr, logical, strided.data());
  check(built, "legacy null-input gradient must build");
  if (!built) return;

  std::vector<unsigned char> expected(
      static_cast<std::size_t>(width) * height * pixel_bytes, 0);
  for (int32_t y = 0; y < height; ++y) {
    for (int32_t x = 0; x < width; ++x) {
      auto* pixel = expected.data() +
          (static_cast<std::size_t>(y) * width + x) * pixel_bytes;
      pixel[0] = 255;
      pixel[1] = static_cast<unsigned char>(x * 255 / (width - 1));
      pixel[2] = static_cast<unsigned char>(y * 255 / (height - 1));
      pixel[3] = static_cast<unsigned char>(
          (x + y) * 255 / (width + height - 2));
    }
  }
  check(logical == expected,
        "legacy null-input gradient formula or zero tail drifted");
  check(strided_matches(strided, expected, width, height, pixel_bytes, rowbytes),
        "legacy null-input gradient striding or padding drifted");
}

void unchanged_non_argb8_and_null_input_paths() {
  unchanged_external_depth(8, 7);
  unchanged_external_depth(16, 11);
  unchanged_null_gradient(4, 5);
  unchanged_null_gradient(8, 7);
  unchanged_null_gradient(16, 11);
}

}  // namespace

int main() {
  bulk_disjoint_offsets_and_guards();
  bulk_exact_alias_roundtrips();
  every_channel_byte_value_survives();
  classic_argb8_packed_and_padded_worlds();
  unchanged_non_argb8_and_null_input_paths();
  if (failures == 0)
    std::printf("{\"worker_pixel_transport_selftest\":\"passed\"}\n");
  return failures == 0 ? 0 : 1;
}
