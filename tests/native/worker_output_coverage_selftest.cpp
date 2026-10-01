#include "worker_output_coverage.hpp"

#include <array>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <vector>

using aexcompat::worker_runtime::output_coverage::inspect;
using aexcompat::worker_runtime::output_coverage::seed;

int main() {
  int failures = 0;
  const auto check = [&](bool condition, const char* description) {
    if (!condition) { std::fprintf(stderr, "FAIL: %s\n", description); ++failures; }
  };
  constexpr int32_t width = 8, height = 6;
  for (int32_t depth : {4, 8, 16}) {
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
  std::puts(failures ? "{\"output_coverage_selftest\":\"failed\"}" :
                       "{\"output_coverage_selftest\":\"passed\"}");
  return failures ? 1 : 0;
}
