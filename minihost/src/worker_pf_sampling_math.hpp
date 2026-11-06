#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <type_traits>

namespace aexcompat::sampling {

// These rounding boundaries are observable in the CPU oracle, including
// cancellation. Keep multiply and add separate even on FMA-capable machines.
template <typename T> T product(T a, T b) { volatile T value = a * b; return value; }
template <typename T> T sum(T a, T b) { volatile T value = a + b; return value; }

struct World {
  const unsigned char* data;
  int32_t stride, width, height;
};

template <typename C> using Pixel = std::array<C, 4>;

template <typename C> Pixel<C> read(const World& world, int64_t x, int64_t y) {
  Pixel<C> result{};
  if (x >= 0 && y >= 0 && x < world.width && y < world.height)
    std::memcpy(result.data(), world.data + static_cast<std::size_t>(y) * world.stride +
        static_cast<std::size_t>(x) * sizeof(result), sizeof(result));
  return result;
}

template <typename C> Pixel<C> subpixel(const World& world, int64_t x, int64_t y) {
  const uint64_t fx = static_cast<uint64_t>(x) & 65535u;
  const uint64_t fy = static_cast<uint64_t>(y) & 65535u;
  const int64_t ix = (x - static_cast<int64_t>(fx)) / 65536;
  const int64_t iy = (y - static_cast<int64_t>(fy)) / 65536;
  // Integer points preserve the original word, including transparent RGB.
  if (fx == 0 && fy == 0) return read<C>(world, ix, iy);
  const Pixel<C> pixels[]{read<C>(world, ix, iy), read<C>(world, ix+1, iy),
                          read<C>(world, ix, iy+1), read<C>(world, ix+1, iy+1)};
  Pixel<C> result{};
  if constexpr (!std::is_same_v<C, float>) {
    const uint64_t weights[]{(65536-fx)*(65536-fy), fx*(65536-fy), (65536-fx)*fy, fx*fy};
    uint64_t alpha = 0;
    for (int i = 0; i < 4; ++i) alpha += weights[i] * pixels[i][0];
    constexpr uint64_t maximum = sizeof(C) == 1 ? 255 : 32768;
    result[0] = static_cast<C>(std::min(maximum, (alpha + (uint64_t{1} << 31)) >> 32));
    if (!result[0]) return result;
    for (int c = 1; c < 4; ++c) {
      // Sum <= 2^32 * 65535^2; even the half-denominator fits uint64_t.
      uint64_t color = 0;
      for (int i = 0; i < 4; ++i) color += weights[i] * pixels[i][0] * pixels[i][c];
      result[c] = static_cast<C>(std::min(maximum, (color + alpha/2) / alpha));
    }
  } else {
    const float u = static_cast<float>(fx) / 65536.0f;
    const float v = static_cast<float>(fy) / 65536.0f;
    const float corner = product(u, v);
    const float right = sum(u, -corner);
    const float weights[]{sum(sum(1.0f, -v), -right), right, sum(v, -corner), corner};
    std::array<float, 4> alpha_weights{};
    double alpha = 0;
    for (int i = 0; i < 4; ++i) {
      if (weights[i] == 0) continue;
      alpha_weights[i] = product(weights[i], pixels[i][0]);
      alpha += alpha_weights[i];
    }
    const float denominator = static_cast<float>(alpha);
    result[0] = denominator;
    for (int c = 1; c < 4; ++c) {
      float color = 0;
      for (int i = 0; i < 4; ++i) {
        if (weights[i] == 0) continue;
        color = sum(color, product(denominator != 0 ? alpha_weights[i] : weights[i], pixels[i][c]));
      }
      result[c] = denominator != 0 ? product(color, 1.0f / denominator) : color;
    }
  }
  return result;
}

// A narrow axis samples the center through subpixel; a wide axis integrates
// intersecting pixel squares. Radius validation remains in the callback owner.
struct Axis {
  int64_t center;
  double radius;
  int64_t first, end;
  bool narrow;
  Axis(int64_t fixed_center, int32_t fixed_radius)
      : center(fixed_center), radius(fixed_radius / 65536.0), narrow(fixed_radius <= 32768) {
    first = narrow ? 0 : static_cast<int64_t>(std::floor(center/65536.0 - radius + 0.5));
    end = narrow ? 1 : static_cast<int64_t>(std::ceil(center/65536.0 + radius + 0.5));
  }
  int64_t coordinate(int64_t i) const { return narrow ? center : i * 65536; }
  double weight(int64_t i) const {
    return narrow ? 2*radius : std::max(0.0,
        std::min(center/65536.0 + radius, i+0.5) - std::max(center/65536.0 - radius, i-0.5));
  }
};

template <typename C> Pixel<C> area(const World& world, int64_t x, int64_t y, int32_t rx, int32_t ry) {
  using Accum = std::conditional_t<sizeof(C) == 1, float, double>;
  const Axis xs(x, rx), ys(y, ry);
  std::array<Accum, 4> accum{};
  for (int64_t yy = ys.first; yy < ys.end; ++yy) {
    for (int64_t xx = xs.first; xx < xs.end; ++xx) {
      const auto p = xs.narrow || ys.narrow ? subpixel<C>(world, xs.coordinate(xx), ys.coordinate(yy)) :
                                            read<C>(world, xx, yy);
      const Accum w = static_cast<Accum>(xs.weight(xx) * ys.weight(yy));
      const Accum wa = product(w, static_cast<Accum>(p[0]));
      accum[0] = sum(accum[0], wa);
      for (int c = 1; c < 4; ++c) accum[c] = sum(accum[c], product(wa, static_cast<Accum>(p[c])));
    }
  }
  Pixel<C> result{};
  if (accum[0] == 0) return result;
  constexpr Accum maximum = std::is_same_v<C, float> ? 1 : (sizeof(C) == 1 ? 255 : 32768);
  const Accum footprint = static_cast<Accum>(4*xs.radius*ys.radius);
  const Accum alpha = std::min(maximum, product(accum[0], Accum{1}/footprint));
  if (alpha == 0) return result;
  const Accum reciprocal = Accum{1}/accum[0];
  for (int c = 0; c < 4; ++c) {
    const Accum value = c == 0 ? alpha : (sizeof(C) == 1 ? accum[c]/accum[0] : product(accum[c], reciprocal));
    if constexpr (std::is_same_v<C, float>) result[c] = static_cast<float>(value);
    else result[c] = static_cast<C>(std::clamp(sum(value, Accum{0.5}), Accum{0}, maximum));
  }
  return result;
}

}  // namespace aexcompat::sampling
