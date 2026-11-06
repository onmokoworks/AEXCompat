#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>

namespace aexcompat::pf_world_transform::copy_sampling {

// COPY uses a 16.16 source step. Its area radius discards the low bit of
// that step, while the raw-byte area denominator uses the unhalved steps.
// These are separate quantities, observable on non-integral scale factors.
constexpr int64_t unit = 65536;
using Pixel = std::array<double, 4>;

// Preserve the oracle's round points on targets that can contract multiply/add.
template <typename T> inline T product(T a, T b) {
  volatile T value = a * b;
  return value;
}
template <typename T> inline T sum(T a, T b) {
  volatile T value = a + b;
  return value;
}

struct Source {
  const unsigned char* data;
  int32_t rowbytes, width, height, component_bytes;
  Pixel read(int64_t x, int64_t y) const {
    Pixel p{};
    if (x < 0 || y < 0 || x >= width || y >= height) return p;
    const auto* address = data + y * rowbytes + x * component_bytes * 4;
    for (int c = 0; c < 4; ++c) {
      if (component_bytes == 1)
        p[c] = address[c];
      else if (component_bytes == 2) {
        uint16_t v;
        std::memcpy(&v, address + c * 2, 2);
        p[c] = v;
      } else {
        float v;
        std::memcpy(&v, address + c * 4, 4);
        p[c] = v;
      }
    }
    return p;
  }
};

inline int64_t floor_fixed(int64_t value) {
  return value >= 0 ? value / unit : -((-value + unit - 1) / unit);
}

inline float add4(const std::array<float, 4>& v) {
  // Keep single-precision round points; do not contract products with sums.
  const float a = sum(v[0], v[1]);
  const float b = sum(a, v[2]);
  return sum(b, v[3]);
}

inline Pixel bilinear(const Source& src, int64_t x, int64_t y, bool raw) {
  const auto ix = floor_fixed(x), iy = floor_fixed(y);
  const int64_t fx = x - ix * unit, fy = y - iy * unit;
  const std::array<Pixel, 4> p{src.read(ix, iy), src.read(ix + 1, iy),
                               src.read(ix, iy + 1), src.read(ix + 1, iy + 1)};
  Pixel out{};
  if (src.component_bytes == 4) {
    const float xf = static_cast<float>(fx) / 65536.0f;
    const float yf = static_cast<float>(fy) / 65536.0f;
    const float br = product(xf, yf), tr = sum(xf, -br), bl = sum(yf, -br);
    const float rest1 = sum(1.0f, -tr), rest2 = sum(rest1, -bl);
    const std::array<float, 4> weights{sum(rest2, -br), tr, bl, br};
    std::array<float, 4> alpha{};
    bool same = true;
    for (int i = 0; i < 4; ++i) {
      alpha[i] = product(static_cast<float>(p[i][0]), weights[i]);
      same = same && p[i][0] == p[0][0];
    }
    float a = same ? static_cast<float>(p[0][0]) : add4(alpha);
    if (!same && a != 0) a = std::min(1.0f, a);
    out[0] = a;
    for (int c = 1; c < 4; ++c) {
      std::array<float, 4> terms{};
      for (int i = 0; i < 4; ++i)
        terms[i] = product(static_cast<float>(p[i][c]),
                           same || a == 0 ? weights[i] : alpha[i]);
      float v = add4(terms);
      if (!same && a != 0) v = product(v, 1.0f / a);
      out[c] = v;
    }
    return out;
  }
  std::array<int64_t, 4> weights{(unit - fx) * (unit - fy), fx * (unit - fy),
                                 (unit - fx) * fy, fx * fy};
  int64_t denominator = unit * unit;
  if (raw) {
    for (auto& w : weights) w /= unit;
    weights[0] = unit - weights[1] - weights[2] - weights[3];
    denominator = unit;
  }
  double a = 0;
  for (int i = 0; i < 4; ++i) a += p[i][0] * weights[i];
  out[0] = a / denominator;
  for (int c = 1; c < 4; ++c) {
    double sum = 0;
    for (int i = 0; i < 4; ++i)
      sum += raw ? p[i][c] * weights[i] : p[i][c] * p[i][0] * weights[i];
    out[c] = raw ? sum / denominator : (a ? sum / a : 0);
  }
  return out;
}

inline Pixel area(const Source& src, int64_t x, int64_t y, int64_t step_x,
                  int64_t step_y, bool raw) {
  const int64_t rx = step_x / 2, ry = step_y / 2;
  const int64_t left = x + unit / 2 - rx, right = x + unit / 2 + rx;
  const int64_t top = y + unit / 2 - ry, bottom = y + unit / 2 + ry;
  Pixel sums{};
  const int64_t first_x = std::max<int64_t>(0, floor_fixed(left));
  const int64_t first_y = std::max<int64_t>(0, floor_fixed(top));
  const int64_t end_x = std::min<int64_t>(src.width, (right + unit - 1) / unit);
  const int64_t end_y =
      std::min<int64_t>(src.height, (bottom + unit - 1) / unit);
  for (int64_t iy = first_y; iy < end_y; ++iy) {
    const int64_t wy =
        std::min(bottom, (iy + 1) * unit) - std::max(top, iy * unit);
    for (int64_t ix = first_x; ix < end_x; ++ix) {
      const int64_t wx =
          std::min(right, (ix + 1) * unit) - std::max(left, ix * unit);
      const double weight = static_cast<double>(wx) * wy;
      const Pixel p = src.read(ix, iy);
      if (raw) {
        for (int c = 0; c < 4; ++c)
          sums[c] += std::floor(p[c] * weight / 16777216.0);
      } else {
        sums[0] = sum(sums[0], product(p[0], weight));
        for (int c = 1; c < 4; ++c)
          sums[c] = sum(sums[c], product(product(p[c], p[0]), weight));
      }
    }
  }
  Pixel out{};
  if (raw) {
    const int64_t denominator = (step_x * step_y / unit + 128) / 256;
    if (!denominator) return out;
    for (int c = 0; c < 4; ++c)
      out[c] = std::floor((sums[c] + denominator / 2) / denominator);
    if (out[0] == 0) out = {};
  } else {
    const double denominator = static_cast<double>(rx * 2) * (ry * 2);
    if (!denominator) return out;
    out[0] = sums[0] / denominator;
    for (int c = 1; c < 4; ++c) out[c] = sums[0] ? sums[c] / sums[0] : 0;
  }
  return out;
}

inline void store(unsigned char* address, const Pixel& p, int component_bytes) {
  for (int c = 0; c < 4; ++c) {
    if (component_bytes == 4) {
      const float v = static_cast<float>(p[c]);
      std::memcpy(address + c * 4, &v, 4);
    } else {
      const double max = component_bytes == 1 ? 255 : 32768;
      const double bounded =
          std::isfinite(p[c]) ? std::clamp(p[c], 0.0, max) : 0;
      const uint16_t v = static_cast<uint16_t>(std::floor(bounded + 0.5));
      if (component_bytes == 1)
        address[c] = static_cast<uint8_t>(v);
      else
        std::memcpy(address + c * 2, &v, 2);
    }
  }
}

}  // namespace aexcompat::pf_world_transform::copy_sampling
