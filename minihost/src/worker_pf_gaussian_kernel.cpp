#include "worker_pf_gaussian_kernel.hpp"
#include "worker_pf_private_callbacks.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace aexcompat::pf_gaussian_kernel {
namespace {
constexpr int32_t kAErrParameter = 3;      // A_Err_PARAMETER, what PF.dll answers
constexpr int32_t kPfBadCallbackParam = 516;
constexpr uint32_t kFlag1D = 1u << 0;         // PF_KernelFlag_1D
constexpr uint32_t kFlagNormalized = 1u << 1; // PF_KernelFlag_NORMALIZED

// PFp_GaussianValue (PF.dll export): the bell AE uses for every kernel it
// generates, shared with the private get_callback_addr id -5 answer
// (worker_pf_private_callbacks.cpp, issue #985) so the RE-derived constants
// live in one place. `d` is the distance from the kernel centre over
// (radius + 1).
using aexcompat::pf_private::gaussian_value;

int32_t truncate_to_int(double value) {
  if (!(value == value)) return 0;
  if (value >= static_cast<double>(std::numeric_limits<int32_t>::max()))
    return std::numeric_limits<int32_t>::max();
  if (value <= static_cast<double>(std::numeric_limits<int32_t>::min()))
    return std::numeric_limits<int32_t>::min();
  return static_cast<int32_t>(value);
}
}  // namespace

int32_t __cdecl gaussian_kernel(void* effect_ref, double radius, uint32_t flags,
                                double multiplier, int32_t* diameter, void* kernel) {
  if (!effect_ref) return kPfBadCallbackParam;
  // AE's own refusals: negative radius, missing outputs.
  if (!(radius >= 0.0) || !diameter || !kernel) return kAErrParameter;
  const double ceiled = std::ceil(radius);
  if (ceiled > static_cast<double>(kMaxRadius)) return kPfBadCallbackParam;
  const int32_t r = static_cast<int32_t>(ceiled);
  const bool one_dimensional = (flags & kFlag1D) != 0;
  const int32_t y_extent = one_dimensional ? 0 : r;
  const int32_t row_length = 2 * r + 1;
  const bool scale = multiplier != 1.0;
  const double divisor = radius + 1.0;
  auto* out = static_cast<int32_t*>(kernel);
  double sum = 0.0;
  int32_t index = 0;
  for (int32_t y = -y_extent; y <= y_extent; ++y) {
    // AE walks each row from its left edge to the centre and mirrors every
    // entry to the right half; the centre (ix == 0) is written once.
    for (int32_t ix = r; ix >= 0; --ix) {
      const int32_t y2 = y * y;
      const double distance =
          (y2 == 0 ? static_cast<double>(ix)
                   : std::sqrt(static_cast<double>(ix * ix + y2))) / divisor;
      double value = gaussian_value(distance) * 255.0;
      if (scale) value *= multiplier;
      const int32_t stored = std::clamp(truncate_to_int(value + 0.5), 0, 255);
      sum += value;
      out[index] = stored;
      if (ix != 0) {
        sum += value;
        out[index + ix * 2] = stored;
      }
      ++index;
    }
    index += r;  // (r + 1) entries written, skip the mirrored r
  }
  if ((flags & kFlagNormalized) != 0 && sum > 0.0) {
    const int32_t entries = (2 * y_extent + 1) * row_length;
    // 255 per stored entry: (2 y_extent + 1) rows of (2 r + 1). PF.dll forms
    // it as (y_extent * 0x1fe + 0xff) * (2 r + 1) in int arithmetic; computed
    // in double here so the largest accepted radius cannot overflow it.
    const double target =
        (static_cast<double>(y_extent) * 0x1fe + 0xff) * static_cast<double>(row_length);
    const double factor = target / sum;
    for (int32_t i = 0; i < entries; ++i)
      out[i] = truncate_to_int(static_cast<double>(out[i]) * factor);
  }
  *diameter = row_length;
  return 0;
}

}  // namespace aexcompat::pf_gaussian_kernel
