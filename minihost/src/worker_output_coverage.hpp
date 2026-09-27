#pragma once

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <vector>

namespace aexcompat::worker_runtime::output_coverage {

// Counts pixels that still match the position/depth-specific initialization
// pattern. Byte comparison cannot prove whether a plug-in wrote an identical
// value; this reduces accidental collisions with legal solid colors, but a
// deliberately matching output remains indistinguishable from an unwritten one.
struct Result {
  bool geometry_valid{};
  bool host_validation_failed{};
  uint64_t promised_pixels{};
  uint64_t unwritten_pixels{};
  std::array<int32_t, 4> bbox{};  // left, top, exclusive right, exclusive bottom
  int32_t max_row_run{};
  int32_t max_column_run{};
};

inline unsigned char pattern_byte(int32_t x, int32_t y, int32_t pixel_bytes,
                                  int32_t byte_index) {
  // In little-endian ARGB32F, byte 3 of each channel contains the exponent.
  // 0x3F keeps every sentinel channel finite instead of confusing partial
  // coverage with a plug-in-produced NaN/Inf diagnostic.
  if (pixel_bytes == 16 && byte_index % 4 == 3) return 0x3F;
  uint64_t state = (static_cast<uint64_t>(static_cast<uint32_t>(x)) << 32) |
                   static_cast<uint32_t>(y);
  state ^= static_cast<uint64_t>(pixel_bytes) << 48;
  state ^= static_cast<uint64_t>(byte_index) * 0x9e3779b97f4a7c15ULL;
  state += 0x9e3779b97f4a7c15ULL;
  state = (state ^ (state >> 30)) * 0xbf58476d1ce4e5b9ULL;
  state = (state ^ (state >> 27)) * 0x94d049bb133111ebULL;
  const auto value = static_cast<unsigned char>((state ^ (state >> 31)) >> 56);
  return value == 0xCC ? 0xCD : value;
}

inline bool seed(unsigned char* pixels, std::size_t size, int32_t width,
                 int32_t height, int32_t rowbytes, int32_t pixel_bytes) {
  if (!pixels || width < 0 || height < 0 || rowbytes < 0 ||
      (pixel_bytes != 4 && pixel_bytes != 8 && pixel_bytes != 16) ||
      static_cast<uint64_t>(width) * pixel_bytes > static_cast<uint64_t>(rowbytes) ||
      static_cast<uint64_t>(rowbytes) * height > size)
    return false;
  for (int32_t y = 0; y < height; ++y)
    for (int32_t x = 0; x < width; ++x)
      for (int32_t b = 0; b < pixel_bytes; ++b)
        pixels[static_cast<std::size_t>(y) * rowbytes +
               static_cast<std::size_t>(x) * pixel_bytes + b] =
            pattern_byte(x, y, pixel_bytes, b);
  return true;
}

inline Result inspect(const unsigned char* pixels, std::size_t size,
                      int32_t width, int32_t height, int32_t pixel_bytes,
                      std::array<int32_t, 4> promised) {
  Result result;
  if (!pixels || width < 0 || height < 0 ||
      (pixel_bytes != 4 && pixel_bytes != 8 && pixel_bytes != 16) ||
      promised[0] < 0 || promised[1] < 0 || promised[2] < promised[0] ||
      promised[3] < promised[1] || promised[2] > width || promised[3] > height ||
      static_cast<uint64_t>(width) * height * pixel_bytes > size)
    return result;
  result.geometry_valid = true;
  result.promised_pixels = static_cast<uint64_t>(promised[2] - promised[0]) *
                           static_cast<uint64_t>(promised[3] - promised[1]);
  std::vector<int32_t> column_runs(static_cast<std::size_t>(promised[2] - promised[0]));
  for (int32_t y = promised[1]; y < promised[3]; ++y) {
    int32_t row_run = 0;
    for (int32_t x = promised[0]; x < promised[2]; ++x) {
      const std::size_t offset =
          (static_cast<std::size_t>(y) * width + x) * pixel_bytes;
      bool unwritten = true;
      for (int32_t channel_byte = 0; channel_byte < pixel_bytes; ++channel_byte)
        unwritten &= pixels[offset + channel_byte] ==
            pattern_byte(x, y, pixel_bytes, channel_byte);
      auto& column_run = column_runs[static_cast<std::size_t>(x - promised[0])];
      if (!unwritten) {
        row_run = 0;
        column_run = 0;
        continue;
      }
      ++result.unwritten_pixels;
      row_run++;
      column_run++;
      result.max_row_run = std::max(result.max_row_run, row_run);
      result.max_column_run = std::max(result.max_column_run, column_run);
      if (result.unwritten_pixels == 1) result.bbox = {x, y, x + 1, y + 1};
      else {
        result.bbox[0] = std::min(result.bbox[0], x);
        result.bbox[1] = std::min(result.bbox[1], y);
        result.bbox[2] = std::max(result.bbox[2], x + 1);
        result.bbox[3] = std::max(result.bbox[3], y + 1);
      }
    }
  }
  return result;
}

}  // namespace aexcompat::worker_runtime::output_coverage
