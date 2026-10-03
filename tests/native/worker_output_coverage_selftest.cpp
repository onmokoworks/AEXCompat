#include "worker_output_coverage.hpp"

#include <array>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <vector>

using aexcompat::worker_runtime::output_coverage::inspect;
using aexcompat::worker_runtime::output_coverage::seed;
using aexcompat::worker_runtime::output_coverage::seed_geometry_valid;

int main() {
  int failures = 0;
  const auto check = [&](bool condition, const char* description) {
    if (!condition) { std::fprintf(stderr, "FAIL: %s\n", description); ++failures; }
  };
  constexpr int32_t width = 8, height = 6;
  for (int32_t depth : {4, 8, 16}) {
    struct GeometryCase {
      int32_t width, height, rowbytes;
      std::size_t size;
      bool accepted;
    };
    const GeometryCase cases[] = {
        {3, 2, 3 * depth, static_cast<std::size_t>(6 * depth), true},
        {3, 2, 3 * depth + 5, static_cast<std::size_t>(6 * depth + 10), true},
        {0, 0, 0, 0, true}, {0, 2, 0, 0, true},
        {3, 0, 3 * depth, 0, true},
        {-1, 2, 3 * depth, 512, false},
        {3, -1, 3 * depth, 512, false},
        {3, 2, -1, 512, false},
        {3, 2, 3 * depth - 1, 512, false},
        {3, 2, 3 * depth, static_cast<std::size_t>(6 * depth - 1), false},
        {std::numeric_limits<int32_t>::max(), 2,
         std::numeric_limits<int32_t>::max(), 512, false},
        {3, std::numeric_limits<int32_t>::max(), 3 * depth, 512, false},
    };
    for (const auto& geometry : cases) {
      std::vector<unsigned char> pixels(512, 0xCC);
      const auto before = pixels;
      check(seed_geometry_valid(pixels.data(), geometry.size, geometry.width,
                                geometry.height, geometry.rowbytes, depth) ==
                geometry.accepted,
            "early geometry check preserves acceptance");
      check(pixels == before, "early geometry check never changes output bytes");
      check(seed(pixels.data(), geometry.size, geometry.width, geometry.height,
                 geometry.rowbytes, depth) == geometry.accepted,
            "seed preserves geometry acceptance");
      if (!geometry.accepted || geometry.width == 0 || geometry.height == 0)
        check(pixels == before, "refused and empty seeds do not write");
    }
    check(!seed_geometry_valid(nullptr, 512, 3, 2, 3 * depth, depth) &&
              !seed(nullptr, 512, 3, 2, 3 * depth, depth),
          "null output is refused before initialization");
    for (int32_t odd_width : {1, 7, 17}) {
      constexpr int32_t rows = 13;
      std::vector<unsigned char> packed(odd_width * rows * depth, 0xCC);
      check(seed(packed.data(), packed.size(), odd_width, rows,
                 odd_width * depth, depth), "packed odd world can be seeded");
      if (odd_width == 17) {
        // Frozen byte fingerprints of the original initialization, independent
        // of the refactored geometry helper and of inspect's pattern generator.
        uint64_t fingerprint = 14695981039346656037ULL;
        for (const auto byte : packed)
          fingerprint = (fingerprint ^ byte) * 1099511628211ULL;
        const uint64_t expected = depth == 4 ? 0x8e702832acf03dc1ULL :
            depth == 8 ? 0xeb2a75d6871ff104ULL : 0x847373bbca4d3368ULL;
        check(fingerprint == expected, "all logical seed bytes are unchanged");
      }
      for (int32_t padding : {0, 3, 13}) for (std::size_t alignment : {0u, 1u, 7u}) {
        const int32_t stride = odd_width * depth + padding;
        const std::size_t size = static_cast<std::size_t>(stride) * rows;
        const std::size_t prefix = 16 + alignment;
        std::vector<unsigned char> storage(prefix + size + 16, 0xA5);
        auto* pixels = storage.data() + prefix;
        std::memset(pixels, 0xCC, size);
        check(seed(pixels, size, odd_width, rows, stride, depth),
              "odd padded unaligned world can be seeded");
        for (int32_t y = 0; y < rows; ++y) {
          check(std::memcmp(pixels + static_cast<std::size_t>(y) * stride,
                            packed.data() + static_cast<std::size_t>(y) * odd_width * depth,
                            static_cast<std::size_t>(odd_width) * depth) == 0,
                "strided and packed worlds have the same logical pixels");
          for (int32_t b = odd_width * depth; b < stride; ++b)
            check(pixels[static_cast<std::size_t>(y) * stride + b] == 0xCC,
                  "row padding is untouched");
        }
        for (std::size_t b = 0; b < prefix; ++b)
          check(storage[b] == 0xA5, "leading guard is untouched");
        for (std::size_t b = prefix + size; b < storage.size(); ++b)
          check(storage[b] == 0xA5, "trailing guard is untouched");
      }
    }
    // A change in any byte, including the last one, is sufficient. Every
    // other pixel still matches the sentinel, so counts, bbox and runs must
    // continue to identify them rather than accepting the whole frame.
    for (int32_t changed_byte = 0; changed_byte < depth; ++changed_byte) {
      std::vector<unsigned char> pixels(width * height * depth);
      check(seed(pixels.data(), pixels.size(), width, height, width * depth,
                 depth), "single-byte mutation can be seeded");
      pixels[changed_byte] ^= 1;
      const auto result = inspect(pixels.data(), pixels.size(), width, height,
                                  depth, {0, 0, width, height});
      check(result.geometry_valid && result.promised_pixels == width * height &&
                result.unwritten_pixels == width * height - 1,
            "one differing byte changes only its pixel classification");
      check(result.bbox == std::array<int32_t, 4>{0, 0, width, height} &&
                result.max_row_run == width && result.max_column_run == height,
            "matching pixels retain exact bbox and runs after a byte mutation");
      pixels[changed_byte] ^= 1;
      const auto restored = inspect(pixels.data(), pixels.size(), width, height,
                                    depth, {0, 0, width, height});
      check(restored.unwritten_pixels == width * height,
            "restored sentinel remains unwritten");
    }
    for (int mutation = 0; mutation < 6; ++mutation) {
      std::vector<unsigned char> pixels(width * height * depth, 7);
      check(seed(pixels.data(), pixels.size(), width, height, width * depth,
                 depth), "output pattern can be seeded");
      if (depth == 16)
        for (std::size_t offset = 0; offset < pixels.size(); offset += 4) {
          float channel{};
          std::memcpy(&channel, pixels.data() + offset, sizeof(channel));
          check(std::isfinite(channel), "float32 sentinel channel is finite");
        }
      uint64_t expected = 0;
      for (int32_t y = 0; y < height; ++y) for (int32_t x = 0; x < width; ++x) {
        const bool leave = mutation == 0 ? x == 3 && y == 2 :
            mutation == 1 ? y == 2 : mutation == 2 ? x == 3 :
            mutation == 3 ? x >= width / 2 :
            mutation == 4 ? y >= height / 2 : false;
        if (!leave) {
          const auto offset = (y * width + x) * depth;
          for (int32_t b = 0; b < depth; ++b) pixels[offset + b] = 7;
        } else {
          ++expected;
        }
      }
      const auto result = inspect(pixels.data(), pixels.size(), width, height,
                                  depth, {0, 0, width, height});
      check(result.geometry_valid && result.promised_pixels == width * height,
            "full output geometry is accepted");
      check(result.unwritten_pixels == expected, "mutation count matches");
      check((result.unwritten_pixels != 0) == (mutation != 5),
            "written and unwritten variants differ");
      if (mutation == 3) {
        check(result.bbox == std::array<int32_t, 4>{4, 0, 8, 6},
              "right-half bbox is exact");
        check(result.max_row_run == 4 && result.max_column_run == 6,
              "right-half run lengths are exact");
      }
      if (mutation == 5) {
        check(result.bbox == std::array<int32_t, 4>{},
              "fully written output has an empty bbox");
      }
    }
    std::vector<unsigned char> outside(width * height * depth, 0xCC);
    check(seed(outside.data(), outside.size(), width, height, width * depth,
               depth), "cropped output pattern can be seeded");
    for (int32_t y = 1; y < 5; ++y) for (int32_t x = 2; x < 6; ++x)
      for (int32_t b = 0; b < depth; ++b)
        outside[(y * width + x) * depth + b] = 7;
    const auto cropped = inspect(outside.data(), outside.size(), width, height,
                                 depth, {2, 1, 6, 5});
    check(cropped.geometry_valid && cropped.unwritten_pixels == 0,
          "initial pixels outside promised rect are ignored");
    const auto empty = inspect(outside.data(), outside.size(), width, height,
                               depth, {2, 1, 2, 1});
    check(empty.geometry_valid && empty.promised_pixels == 0 &&
              empty.unwritten_pixels == 0, "empty result is valid");
    std::vector<unsigned char> legal(width * height * depth, 0xCC);
    const auto solid = inspect(legal.data(), legal.size(), width, height,
                               depth, {0, 0, width, height});
    check(solid.geometry_valid && solid.unwritten_pixels == 0,
          "legal solid 0xCC pixel content is not the initial pattern");
  }
  for (int32_t unsupported_depth : {-1, 0, 1, 3, 5, 12, 32}) {
    std::vector<unsigned char> pixels(128, 0xCC);
    const auto before = pixels;
    check(!seed_geometry_valid(pixels.data(), pixels.size(), 2, 2, 64,
                               unsupported_depth) &&
              !seed(pixels.data(), pixels.size(), 2, 2, 64, unsupported_depth) &&
              pixels == before,
          "unsupported depth is refused without writing");
  }
  std::puts(failures ? "{\"output_coverage_selftest\":\"failed\"}" :
                       "{\"output_coverage_selftest\":\"passed\"}");
  return failures ? 1 : 0;
}
