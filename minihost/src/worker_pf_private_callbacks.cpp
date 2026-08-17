#include "worker_pf_private_callbacks.hpp"

#include "generated/aex_abi_contract.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <limits>
#include <new>
#include <type_traits>
#include <vector>

namespace aexcompat::pf_private {
namespace {

namespace contract = aexcompat::abi::x86_64_windows;

Hooks g_hooks{};

// Refusals name themselves on the always-on marker the broker's
// `callback_denials` parser reads (the copy_denied / flt_denied shape): the
// plug-in folds the 516 into its own frame error and names neither the
// callback nor the argument.
int32_t denied(const char* callback, const char* reason) {
  std::cerr << "stage:callback_denied callback=" << callback
            << " reason=" << reason << "\n" << std::flush;
  return kBadCallbackParam;
}

int32_t bytes_per_pixel(int32_t pixel_format) noexcept {
  switch (pixel_format) {
    case world_registry::kPixelFormatArgb32: return 4;
    case world_registry::kPixelFormatArgb64: return 8;
    case world_registry::kPixelFormatArgb128: return 16;
    default: return 0;
  }
}

// PF_GaussianBlur1D's integer kernel: w[0] = 255 and
// w[i] = (int)(PFp_GaussianValue(i / (radius + 1.0)) * 255.0) for
// 1 <= i <= ceil(radius). AE truncates each product to int and uses the same
// integer weights at every bit depth (16-bit impulse responses match them
// within one 16-bit step, 3e-5).
std::vector<int32_t> gaussian_weights(float radius) {
  const int32_t taps = static_cast<int32_t>(std::ceil(radius));
  std::vector<int32_t> weights;
  weights.reserve(static_cast<std::size_t>(taps) + 1);
  weights.push_back(255);
  const double divisor = static_cast<double>(radius) + 1.0;
  for (int32_t i = 1; i <= taps; ++i)
    weights.push_back(static_cast<int32_t>(
        gaussian_value(static_cast<double>(i) / divisor) * 255.0));
  return weights;
}

// PF_BoxBlur1D's per-pass box: half-width n = ceil(rho), the two end taps
// weigh 1 - (n - rho); AE keeps that deficit in 1/1024 units (truncated) and
// normalizes by (2n+1)*1024 - 2*deficit.
struct BoxShape {
  int32_t half_width{};
  int32_t deficit_1024{};
  int64_t denominator_1024{};
  double deficit{};
  double denominator{};
};

BoxShape box_shape(float rho) noexcept {
  BoxShape shape{};
  shape.half_width = static_cast<int32_t>(std::ceil(rho));
  shape.deficit_1024 = static_cast<int32_t>(
      (static_cast<float>(shape.half_width) - rho) * 1024.0f);
  shape.denominator_1024 =
      (static_cast<int64_t>(shape.half_width) * 2 + 1) * 1024 -
      2 * static_cast<int64_t>(shape.deficit_1024);
  shape.deficit = static_cast<double>(shape.half_width) - static_cast<double>(rho);
  shape.denominator = static_cast<double>(shape.half_width) * 2.0 + 1.0 -
      2.0 * shape.deficit;
  return shape;
}

// One interleaved ARGB line as the pass arithmetic sees it. The 8-bit path
// mirrors AE's integer code (premultiply with the +0x80 >>8 rounding, box sums
// in 1/1024, gaussian sums against the 255-scaled weights, colors
// unpremultiplied as (c * 255 + a / 2) / a); the deep paths use the same
// structure in double, values in native units (0..32768 for 16-bit, floats
// as they are).
struct Arith8 {
  using Value = int32_t;
  static constexpr double maximum = 255.0;
  static Value premultiply(Value color, Value alpha) noexcept {
    const int32_t t = color * alpha + 0x80;
    return (t + (t >> 8)) >> 8;
  }
};
struct ArithDeep {
  using Value = double;
  double maximum{};
  bool integer_depth{};
  Value premultiply(Value color, Value alpha) const noexcept {
    const double product = color * alpha / maximum;
    return integer_depth ? std::floor(product) : product;
  }
};

struct PassPlan {
  bool box{};
  int32_t iterations{1};
  BoxShape shape{};
  std::vector<int32_t> gaussian;
  bool repeat{};
  bool straight_out{};
  // Deep path only: true for 16-bit, whose AE path is still integer - every
  // pass output is rounded to a whole 16-bit step, the premultiply truncates,
  // and the unpremultiply saturates at the maximum. With those three the
  // captured 16-bit impulse / half-alpha values reproduce exactly (r=6.3
  // three-pass box: all 19 taps; half-alpha green 13106, where a rounding
  // premultiply would give 13107). False for float, which carries what it
  // carries: no rounding, no clipping (AE's float saturation was not
  // observed and an HDR value must not be clipped on a guess).
  bool integer_depth{};
  std::array<bool, 4> channel{};
};

// --- 8-bit exact path -----------------------------------------------------

void box_line8(const std::vector<int32_t>& in, std::vector<int32_t>& out,
               int32_t length, const PassPlan& plan, bool last) {
  const int32_t n = plan.shape.half_width;
  const int64_t deficit = plan.shape.deficit_1024;
  const int64_t den = plan.shape.denominator_1024;
  auto at = [&](int32_t index, int32_t channel) -> int64_t {
    if (index < 0 || index >= length) {
      if (!plan.repeat) return 0;
      index = std::clamp(index, 0, length - 1);
    }
    return in[static_cast<std::size_t>(index) * 4 + channel];
  };
  std::array<int64_t, 4> window{};
  for (int32_t i = -n; i <= n; ++i)
    for (int32_t c = 0; c < 4; ++c) window[c] += at(i, c);
  for (int32_t x = 0; x < length; ++x) {
    if (x > 0)
      for (int32_t c = 0; c < 4; ++c)
        window[c] += at(x + n, c) - at(x - n - 1, c);
    std::array<int64_t, 4> num{};
    for (int32_t c = 0; c < 4; ++c) {
      const int64_t ends = deficit ? at(x - n, c) + at(x + n, c) : 0;
      num[c] = window[c] * 1024 - ends * deficit;
    }
    auto* pixel = &out[static_cast<std::size_t>(x) * 4];
    if (last && plan.straight_out) {
      const int64_t alpha_num = plan.channel[0]
          ? num[0] : static_cast<int64_t>(in[static_cast<std::size_t>(x) * 4]) * den;
      if (alpha_num <= 0) {
        for (int32_t c = 0; c < 4; ++c)
          if (plan.channel[c]) pixel[c] = 0;
        continue;
      }
      if (plan.channel[0]) pixel[0] = static_cast<int32_t>((den / 2 + num[0]) / den);
      const int64_t half = alpha_num / 2;
      for (int32_t c = 1; c < 4; ++c)
        if (plan.channel[c])
          pixel[c] = static_cast<int32_t>(
              std::min<int64_t>(255, (num[c] * 255 + half) / alpha_num));
    } else {
      for (int32_t c = 0; c < 4; ++c)
        if (plan.channel[c])
          pixel[c] = num[c] > 0 ? static_cast<int32_t>((den / 2 + num[c]) / den) : 0;
    }
  }
}

void gaussian_line8(const std::vector<int32_t>& in, std::vector<int32_t>& out,
                    int32_t length, const PassPlan& plan) {
  const auto& w = plan.gaussian;
  const int32_t n = static_cast<int32_t>(w.size()) - 1;
  int64_t total = w[0];
  for (int32_t i = 1; i <= n; ++i) total += 2 * w[static_cast<std::size_t>(i)];
  for (int32_t x = 0; x < length; ++x) {
    std::array<int64_t, 4> sums{};
    int64_t in_bounds = 0;
    for (int32_t i = -n; i <= n; ++i) {
      const int32_t index = x + i;
      if (index < 0 || index >= length) continue;
      const int64_t weight = w[static_cast<std::size_t>(std::abs(i))];
      in_bounds += weight;
      for (int32_t c = 0; c < 4; ++c)
        sums[c] += weight * in[static_cast<std::size_t>(index) * 4 + c];
    }
    const int64_t tot = plan.repeat ? in_bounds : total;
    auto* pixel = &out[static_cast<std::size_t>(x) * 4];
    if (plan.straight_out) {
      const int64_t alpha_num = plan.channel[0]
          ? sums[0] : static_cast<int64_t>(in[static_cast<std::size_t>(x) * 4]) * tot;
      if (tot == 0 || alpha_num == 0) {
        for (int32_t c = 0; c < 4; ++c)
          if (plan.channel[c]) pixel[c] = 0;
        continue;
      }
      if (plan.channel[0]) pixel[0] = static_cast<int32_t>((sums[0] + tot / 2) / tot);
      const int64_t half = alpha_num / 2;
      for (int32_t c = 1; c < 4; ++c)
        if (plan.channel[c])
          pixel[c] = static_cast<int32_t>(
              std::min<int64_t>(255, (sums[c] * 255 + half) / alpha_num));
    } else {
      for (int32_t c = 0; c < 4; ++c)
        if (plan.channel[c])
          pixel[c] = tot ? static_cast<int32_t>((sums[c] + tot / 2) / tot) : 0;
    }
  }
}

// --- deep (16-bit / float) path, same structure in double -------------------

void box_line_deep(const std::vector<double>& in, std::vector<double>& out,
                   int32_t length, const PassPlan& plan, bool last,
                   double maximum) {
  const int32_t n = plan.shape.half_width;
  const double deficit = plan.shape.deficit;
  const double den = plan.shape.denominator;
  auto at = [&](int32_t index, int32_t channel) -> double {
    if (index < 0 || index >= length) {
      if (!plan.repeat) return 0.0;
      index = std::clamp(index, 0, length - 1);
    }
    return in[static_cast<std::size_t>(index) * 4 + channel];
  };
  std::array<double, 4> window{};
  for (int32_t i = -n; i <= n; ++i)
    for (int32_t c = 0; c < 4; ++c) window[c] += at(i, c);
  for (int32_t x = 0; x < length; ++x) {
    if (x > 0)
      for (int32_t c = 0; c < 4; ++c)
        window[c] += at(x + n, c) - at(x - n - 1, c);
    std::array<double, 4> num{};
    for (int32_t c = 0; c < 4; ++c) {
      const double ends = at(x - n, c) + at(x + n, c);
      num[c] = window[c] - ends * deficit;
    }
    auto* pixel = &out[static_cast<std::size_t>(x) * 4];
    const auto quantize = [&](double value) {
      return plan.integer_depth ? std::round(value) : value;
    };
    if (last && plan.straight_out) {
      const double alpha_num = plan.channel[0]
          ? num[0] : in[static_cast<std::size_t>(x) * 4] * den;
      // Zero alpha under a straight request zeroes the selected channels;
      // a negative float alpha lands here too, which is host policy (AE's
      // float behaviour there was not observed), not a measurement.
      if (alpha_num <= 0.0) {
        for (int32_t c = 0; c < 4; ++c)
          if (plan.channel[c]) pixel[c] = 0.0;
        continue;
      }
      if (plan.channel[0]) pixel[0] = quantize(num[0] / den);
      for (int32_t c = 1; c < 4; ++c)
        if (plan.channel[c]) {
          const double value = num[c] * maximum / alpha_num;
          pixel[c] = plan.integer_depth ? std::min(maximum, std::round(value)) : value;
        }
    } else {
      for (int32_t c = 0; c < 4; ++c)
        if (plan.channel[c]) pixel[c] = quantize(num[c] / den);
    }
  }
}

void gaussian_line_deep(const std::vector<double>& in, std::vector<double>& out,
                        int32_t length, const PassPlan& plan, double maximum) {
  const auto& w = plan.gaussian;
  const int32_t n = static_cast<int32_t>(w.size()) - 1;
  double total = w[0];
  for (int32_t i = 1; i <= n; ++i) total += 2.0 * w[static_cast<std::size_t>(i)];
  for (int32_t x = 0; x < length; ++x) {
    std::array<double, 4> sums{};
    double in_bounds = 0.0;
    for (int32_t i = -n; i <= n; ++i) {
      const int32_t index = x + i;
      if (index < 0 || index >= length) continue;
      const double weight = w[static_cast<std::size_t>(std::abs(i))];
      in_bounds += weight;
      for (int32_t c = 0; c < 4; ++c)
        sums[c] += weight * in[static_cast<std::size_t>(index) * 4 + c];
    }
    const double tot = plan.repeat ? in_bounds : total;
    auto* pixel = &out[static_cast<std::size_t>(x) * 4];
    const auto quantize = [&](double value) {
      return plan.integer_depth ? std::round(value) : value;
    };
    if (plan.straight_out) {
      const double alpha_num = plan.channel[0]
          ? sums[0] : in[static_cast<std::size_t>(x) * 4] * tot;
      if (tot <= 0.0 || alpha_num <= 0.0) {
        for (int32_t c = 0; c < 4; ++c)
          if (plan.channel[c]) pixel[c] = 0.0;
        continue;
      }
      if (plan.channel[0]) pixel[0] = quantize(sums[0] / tot);
      for (int32_t c = 1; c < 4; ++c)
        if (plan.channel[c]) {
          const double value = sums[c] * maximum / alpha_num;
          pixel[c] = plan.integer_depth ? std::min(maximum, std::round(value)) : value;
        }
    } else {
      for (int32_t c = 0; c < 4; ++c)
        if (plan.channel[c]) pixel[c] = tot > 0.0 ? quantize(sums[c] / tot) : 0.0;
    }
  }
}

// Runs one axis of the separable blur over the interleaved image. Without the
// repeat flag the box passes run on a zero-padded line (AE renders them on a
// RenderGraph intermediate that grows by the node extent, so a multi-pass
// result at the world border equals the infinitely zero-padded one); with it
// every pass clamps its reads to the line.
template <class Value, class LineBox, class LineGauss>
void run_axis(std::vector<Value>& image, int32_t width, int32_t height,
              bool horizontal, const PassPlan& plan, LineBox line_box,
              LineGauss line_gauss) {
  const int32_t length = horizontal ? width : height;
  const int32_t lines = horizontal ? height : width;
  const int32_t pad = (plan.box && !plan.repeat)
      ? plan.shape.half_width * plan.iterations : 0;
  const int32_t padded = length + 2 * pad;
  std::vector<Value> line(static_cast<std::size_t>(padded) * 4);
  std::vector<Value> next(static_cast<std::size_t>(padded) * 4);
  for (int32_t l = 0; l < lines; ++l) {
    std::fill(line.begin(), line.end(), Value{});
    for (int32_t i = 0; i < length; ++i) {
      const std::size_t source = horizontal
          ? (static_cast<std::size_t>(l) * width + i) * 4
          : (static_cast<std::size_t>(i) * width + l) * 4;
      std::copy_n(&image[source], 4, &line[static_cast<std::size_t>(i + pad) * 4]);
    }
    if (plan.box) {
      for (int32_t it = 0; it < plan.iterations; ++it) {
        next = line;
        line_box(line, next, padded, plan, it == plan.iterations - 1);
        line.swap(next);
      }
    } else {
      next = line;
      line_gauss(line, next, padded, plan);
      line.swap(next);
    }
    for (int32_t i = 0; i < length; ++i) {
      const std::size_t target = horizontal
          ? (static_cast<std::size_t>(l) * width + i) * 4
          : (static_cast<std::size_t>(i) * width + l) * 4;
      std::copy_n(&line[static_cast<std::size_t>(i + pad) * 4], 4, &image[target]);
    }
  }
}

template <class Value>
void premultiply_image(std::vector<Value>& image, const PassPlan& plan,
                       double maximum) {
  for (std::size_t p = 0; p < image.size(); p += 4) {
    const Value alpha = image[p];
    for (int32_t c = 1; c < 4; ++c) {
      if (!plan.channel[c]) continue;
      if constexpr (std::is_same_v<Value, int32_t>)
        image[p + c] = Arith8::premultiply(image[p + c], alpha);
      else
        image[p + c] = ArithDeep{maximum, plan.integer_depth}.premultiply(image[p + c], alpha);
    }
  }
}

// FLT.dll FUN_1800313f0's per-axis decision.
void plan_axis(PassPlan& plan, float radius, int32_t flags, int32_t quality) {
  const float scale = quality == 0 ? 1.4f : 1.0f;
  plan.iterations = quality == 0 ? 1 : 3;
  const float rho = (scale * radius) / 2.71f;
  plan.box = rho > 1.0f && (flags & kFlagNoBox) == 0;
  if (plan.box) {
    plan.shape = box_shape(rho);
  } else {
    plan.gaussian = gaussian_weights(radius);
  }
  plan.repeat = (flags & kFlagRepeatEdge) != 0;
  for (int32_t c = 0; c < 4; ++c) plan.channel[static_cast<std::size_t>(c)] = (flags >> c) & 1;
}

template <class Value>
void run_blur(std::vector<Value>& image, int32_t width, int32_t height,
              float radius, int32_t flags, int32_t quality, bool straight,
              double maximum) {
  const bool x_active = (flags & kFlagHorizontal) != 0 && radius != 0.0f;
  const bool y_active = (flags & kFlagVertical) != 0 && radius != 0.0f;
  if (!x_active && !y_active) return;
  const bool colors = (flags & (kFlagRed | kFlagGreen | kFlagBlue)) != 0;
  PassPlan plan{};
  plan_axis(plan, radius, flags, quality);
  plan.integer_depth = maximum > 1.0;
  // Straight input is premultiplied once, before the first pass, when any
  // color channel is blurred (Blur_1DImgOpInfo's "input needs matting");
  // straight output is unpremultiplied by the last pass of the last axis.
  if (straight && colors) premultiply_image(image, plan, maximum);
  if (x_active) {
    plan.straight_out = straight && !y_active;
    if constexpr (std::is_same_v<Value, int32_t>)
      run_axis(image, width, height, true, plan, &box_line8, &gaussian_line8);
    else
      run_axis(image, width, height, true, plan,
               [maximum](const std::vector<double>& in, std::vector<double>& out,
                         int32_t length, const PassPlan& p, bool last) {
                 box_line_deep(in, out, length, p, last, maximum);
               },
               [maximum](const std::vector<double>& in, std::vector<double>& out,
                         int32_t length, const PassPlan& p) {
                 gaussian_line_deep(in, out, length, p, maximum);
               });
  }
  if (y_active) {
    plan.straight_out = straight;
    if constexpr (std::is_same_v<Value, int32_t>)
      run_axis(image, width, height, false, plan, &box_line8, &gaussian_line8);
    else
      run_axis(image, width, height, false, plan,
               [maximum](const std::vector<double>& in, std::vector<double>& out,
                         int32_t length, const PassPlan& p, bool last) {
                 box_line_deep(in, out, length, p, last, maximum);
               },
               [maximum](const std::vector<double>& in, std::vector<double>& out,
                         int32_t length, const PassPlan& p) {
                 gaussian_line_deep(in, out, length, p, maximum);
               });
  }
}

template <class Channel>
void load_image(const world_safety::DispatchWorldFormat& world,
                std::vector<double>& image) {
  const auto* base = static_cast<const std::byte*>(world.data);
  for (int32_t y = 0; y < world.height; ++y) {
    const auto* row = base + static_cast<std::size_t>(y) * world.rowbytes;
    for (int32_t x = 0; x < world.width; ++x) {
      const auto* pixel = row + static_cast<std::size_t>(x) * 4 * sizeof(Channel);
      const std::size_t index = (static_cast<std::size_t>(y) * world.width + x) * 4;
      for (int32_t c = 0; c < 4; ++c) {
        Channel value{};
        std::memcpy(&value, pixel + c * sizeof(Channel), sizeof(value));
        image[index + c] = static_cast<double>(value);
      }
    }
  }
}

template <class Channel>
void store_image(const std::vector<double>& image,
                 const world_safety::DispatchWorldFormat& world, double maximum) {
  auto* base = static_cast<std::byte*>(world.data);
  for (int32_t y = 0; y < world.height; ++y) {
    auto* row = base + static_cast<std::size_t>(y) * world.rowbytes;
    for (int32_t x = 0; x < world.width; ++x) {
      auto* pixel = row + static_cast<std::size_t>(x) * 4 * sizeof(Channel);
      const std::size_t index = (static_cast<std::size_t>(y) * world.width + x) * 4;
      for (int32_t c = 0; c < 4; ++c) {
        Channel value{};
        if constexpr (std::is_same_v<Channel, float>) {
          value = static_cast<float>(image[index + c]);
        } else {
          value = static_cast<Channel>(
              std::lround(std::clamp(image[index + c], 0.0, maximum)));
        }
        std::memcpy(pixel + c * sizeof(Channel), &value, sizeof(value));
      }
    }
  }
}

int32_t blur_callback(const char* name, bool straight, void* in_data,
                      double radius, int32_t flags, void* world) {
  if (!in_data || !world) return denied(name, "null_argument");
  if (!g_hooks.effect_ref || !g_hooks.resolve_world)
    return denied(name, "unconfigured");
  void* effect_ref{};
  int32_t quality{};
  std::memcpy(&effect_ref, static_cast<const std::byte*>(in_data) + contract::IN_EFFECT_REF_OFFSET,
              sizeof(effect_ref));
  std::memcpy(&quality, static_cast<const std::byte*>(in_data) + contract::IN_QUALITY_OFFSET,
              sizeof(quality));
  if (effect_ref != g_hooks.effect_ref) return denied(name, "effect_ref");
  if (quality != 0 && quality != 1) return denied(name, "quality");
  if (!std::isfinite(radius) || radius < 0.0 || radius > kMaximumRadius)
    return denied(name, "radius");
  if ((flags & ~kKnownFlags) != 0) return denied(name, "unknown_flags");
  if ((flags & kFlagChannels) == 0) return denied(name, "no_channels");
  world_safety::DispatchWorldFormat format{};
  if (!g_hooks.resolve_world(world, format)) return denied(name, "unresolved_world");
  const char* reason = "refused";
  const int32_t result = blur_resolved(format, static_cast<float>(radius), flags,
                                       quality, straight, reason);
  return result == 0 ? 0 : denied(name, reason);
}

}  // namespace

double __cdecl gaussian_value(double x) noexcept {
  if (1.0 < x) return 0.0;
  const double e = std::exp(x * -2.378 * x);
  return 1.0 - (1.0 - e) * 1.102;
}

bool configure(const Hooks& hooks) noexcept {
  if (!hooks.effect_ref || !hooks.resolve_world) return false;
  g_hooks = hooks;
  return true;
}

int32_t blur_resolved(const world_safety::DispatchWorldFormat& world,
                      float radius, int32_t flags, int32_t quality,
                      bool straight, const char*& reason) {
  const int32_t pixel_bytes = bytes_per_pixel(world.pixel_format);
  if (!pixel_bytes || !world.data || world.width <= 0 || world.height <= 0) {
    reason = "world_layout";
    return kBadCallbackParam;
  }
  const int64_t tight = static_cast<int64_t>(world.width) * pixel_bytes;
  const int64_t bytes = static_cast<int64_t>(world.rowbytes) * world.height;
  if (world.rowbytes < tight || bytes <= 0 || bytes > 256LL * 1024 * 1024) {
    reason = "world_layout";
    return kBadCallbackParam;
  }
  if (!std::isfinite(radius) || radius < 0.0f || radius > kMaximumRadius ||
      (flags & ~kKnownFlags) != 0 || (flags & kFlagChannels) == 0 ||
      (quality != 0 && quality != 1)) {
    reason = "invalid_arguments";
    return kBadCallbackParam;
  }
  try {
    const std::size_t values =
        static_cast<std::size_t>(world.width) * world.height * 4;
    if (pixel_bytes == 4) {
      std::vector<int32_t> image(values);
      const auto* base = static_cast<const std::byte*>(world.data);
      for (int32_t y = 0; y < world.height; ++y) {
        const auto* row = base + static_cast<std::size_t>(y) * world.rowbytes;
        for (int32_t x = 0; x < world.width; ++x)
          for (int32_t c = 0; c < 4; ++c)
            image[(static_cast<std::size_t>(y) * world.width + x) * 4 + c] =
                static_cast<int32_t>(static_cast<unsigned char>(
                    row[static_cast<std::size_t>(x) * 4 + c]));
      }
      run_blur(image, world.width, world.height, radius, flags, quality,
               straight, 255.0);
      auto* out = static_cast<std::byte*>(world.data);
      for (int32_t y = 0; y < world.height; ++y) {
        auto* row = out + static_cast<std::size_t>(y) * world.rowbytes;
        for (int32_t x = 0; x < world.width; ++x)
          for (int32_t c = 0; c < 4; ++c)
            row[static_cast<std::size_t>(x) * 4 + c] = static_cast<std::byte>(
                std::clamp(image[(static_cast<std::size_t>(y) * world.width + x) * 4 + c],
                           0, 255));
      }
      return 0;
    }
    std::vector<double> image(values);
    const double maximum = pixel_bytes == 8 ? 32768.0 : 1.0;
    if (pixel_bytes == 8) load_image<uint16_t>(world, image);
    else load_image<float>(world, image);
    run_blur(image, world.width, world.height, radius, flags, quality, straight,
             maximum);
    if (pixel_bytes == 8) store_image<uint16_t>(image, world, maximum);
    else store_image<float>(image, world, maximum);
    return 0;
  } catch (const std::bad_alloc&) {
    reason = "allocation_failed";
    return kBadCallbackParam;
  }
}

int32_t __cdecl blur_straight(void* in_data, void*, double radius, void*,
                              int32_t flags, void* world) {
  return blur_callback("private_blur_straight", true, in_data, radius, flags, world);
}

int32_t __cdecl blur_premultiplied(void* in_data, void*, double radius, void*,
                                   int32_t flags, void* world) {
  return blur_callback("private_blur_premultiplied", false, in_data, radius, flags,
                       world);
}

}  // namespace aexcompat::pf_private
