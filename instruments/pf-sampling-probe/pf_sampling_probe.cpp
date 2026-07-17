#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstring>
#include <type_traits>

namespace {
constexpr PF_Fixed kOne = 1L << 16;
constexpr PF_Fixed kHalf = 1L << 15;

enum class SampleCase : A_long {
  kNearest = 0,
  kNearestHalf,
  kSubpixelHalf,
  kArea,
  kEdge,
  kOutside,
  kCount
};

struct SuiteLease {
  SPBasicSuite* basic = nullptr;
  const char* name = nullptr;
  A_long version = 0;
  const void* suite = nullptr;

  PF_Err Acquire(SPBasicSuite* basic_suite, const char* suite_name,
                 A_long suite_version) {
    basic = basic_suite;
    name = suite_name;
    version = suite_version;
    if (!basic) return PF_Err_BAD_CALLBACK_PARAM;
    return basic->AcquireSuite(name, version, &suite);
  }

  ~SuiteLease() {
    if (basic && suite) basic->ReleaseSuite(name, version);
  }
};

PF_SampPB MakeParams(PF_EffectWorld* input, SampleCase sample_case) {
  PF_SampPB params{};
  params.src = input;
  params.samp_behave = PF_SampleEdgeBehav_ZERO;
  if (sample_case == SampleCase::kArea) {
    params.x_radius = kHalf;
    params.y_radius = kHalf;
    params.area = kOne;
  }
  return params;
}

void Coordinates(const PF_EffectWorld& input, SampleCase sample_case,
                 PF_Fixed* x, PF_Fixed* y) {
  const A_long center_x = std::max<A_long>(0, input.width / 2);
  const A_long center_y = std::max<A_long>(0, input.height / 2);
  *x = center_x * kOne;
  *y = center_y * kOne;
  if (sample_case == SampleCase::kNearestHalf ||
      sample_case == SampleCase::kSubpixelHalf ||
      sample_case == SampleCase::kArea) {
    *x += kHalf;
    *y += kHalf;
  } else if (sample_case == SampleCase::kEdge) {
    *x = -kHalf;
    *y = center_y * kOne;
  } else if (sample_case == SampleCase::kOutside) {
    *x = (input.width + 1) * kOne;
    *y = (input.height + 1) * kOne;
  }
}

template <typename Pixel, typename Suite>
PF_Err Sample(const Suite& suite, PF_ProgPtr effect_ref, SampleCase sample_case,
              PF_Fixed x, PF_Fixed y, const PF_SampPB* params, Pixel* pixel);

template <typename Pixel>
std::array<double, 4> PixelAt(const PF_EffectWorld& world, A_long x, A_long y) {
  if (x < 0 || y < 0 || x >= world.width || y >= world.height) return {};
  const auto* row = reinterpret_cast<const A_u_char*>(world.data) + y * world.rowbytes;
  const Pixel& p = reinterpret_cast<const Pixel*>(row)[x];
  return {double(p.alpha), double(p.red), double(p.green), double(p.blue)};
}

template <typename Pixel>
std::array<double, 4> Oracle(const PF_EffectWorld& world, SampleCase kind,
                             PF_Fixed fx, PF_Fixed fy) {
  const double x = fx / 65536.0, y = fy / 65536.0;
  if (kind != SampleCase::kSubpixelHalf && kind != SampleCase::kArea)
    return PixelAt<Pixel>(world, A_long(std::floor(x + .5)), A_long(std::floor(y + .5)));
  const A_long x0 = A_long(std::floor(x)), y0 = A_long(std::floor(y));
  std::array<std::array<double, 4>, 4> p = {
      PixelAt<Pixel>(world, x0, y0), PixelAt<Pixel>(world, x0 + 1, y0),
      PixelAt<Pixel>(world, x0, y0 + 1), PixelAt<Pixel>(world, x0 + 1, y0 + 1)};
  std::array<double, 4> out{};
  if (kind == SampleCase::kSubpixelHalf) {
    for (const auto& pixel : p)
      for (size_t c = 0; c < 4; ++c) out[c] += pixel[c] * .25;
    return out;
  }
  const double maximum = std::is_same_v<Pixel, PF_Pixel16> ? 32768.0 :
      (std::is_same_v<Pixel, PF_PixelFloat> ? 1.0 : 255.0);
  double alpha_sum = 0.0;
  for (const auto& pixel : p) {
    const double alpha = pixel[0] / maximum;
    alpha_sum += alpha;
    for (size_t c = 1; c < 4; ++c) out[c] += alpha * pixel[c];
  }
  out[0] = alpha_sum * maximum * .25;
  for (size_t c = 1; c < 4; ++c) out[c] = alpha_sum ? out[c] / alpha_sum : 0.0;
  return out;
}

template <typename Pixel>
bool OracleMatches(const Pixel& p, const std::array<double, 4>& expected) {
  const std::array<double, 4> actual = {double(p.alpha), double(p.red),
                                        double(p.green), double(p.blue)};
  const double tolerance = std::is_same_v<Pixel, PF_PixelFloat> ? 1e-6 : .500001;
  for (size_t c = 0; c < 4; ++c)
    if (std::abs(actual[c] - expected[c]) > tolerance) return false;
  return true;
}

template <>
PF_Err Sample<PF_Pixel, PF_Sampling8Suite1>(
    const PF_Sampling8Suite1& suite, PF_ProgPtr effect_ref,
    SampleCase sample_case, PF_Fixed x, PF_Fixed y,
    const PF_SampPB* params, PF_Pixel* pixel) {
  if (sample_case == SampleCase::kSubpixelHalf)
    return suite.subpixel_sample(effect_ref, x, y, params, pixel);
  if (sample_case == SampleCase::kArea)
    return suite.area_sample(effect_ref, x, y, params, pixel);
  return suite.nn_sample(effect_ref, x, y, params, pixel);
}

template <>
PF_Err Sample<PF_Pixel16, PF_Sampling16Suite1>(
    const PF_Sampling16Suite1& suite, PF_ProgPtr effect_ref,
    SampleCase sample_case, PF_Fixed x, PF_Fixed y,
    const PF_SampPB* params, PF_Pixel16* pixel) {
  if (sample_case == SampleCase::kSubpixelHalf)
    return suite.subpixel_sample16(effect_ref, x, y, params, pixel);
  if (sample_case == SampleCase::kArea)
    return suite.area_sample16(effect_ref, x, y, params, pixel);
  return suite.nn_sample16(effect_ref, x, y, params, pixel);
}

template <>
PF_Err Sample<PF_PixelFloat, PF_SamplingFloatSuite1>(
    const PF_SamplingFloatSuite1& suite, PF_ProgPtr effect_ref,
    SampleCase sample_case, PF_Fixed x, PF_Fixed y,
    const PF_SampPB* params, PF_PixelFloat* pixel) {
  if (sample_case == SampleCase::kSubpixelHalf)
    return suite.subpixel_sample_float(effect_ref, x, y, params, pixel);
  if (sample_case == SampleCase::kArea)
    return suite.area_sample_float(effect_ref, x, y, params, pixel);
  return suite.nn_sample_float(effect_ref, x, y, params, pixel);
}

template <typename Pixel, typename Suite>
PF_Err RenderTyped(PF_InData* in_data, PF_EffectWorld* input,
                   PF_EffectWorld* output, const Suite& suite) {
  Pixel samples[static_cast<A_long>(SampleCase::kCount)]{};
  for (A_long index = 0; index < static_cast<A_long>(SampleCase::kCount); ++index) {
    const auto sample_case = static_cast<SampleCase>(index);
    PF_Fixed x = 0;
    PF_Fixed y = 0;
    Coordinates(*input, sample_case, &x, &y);
    const PF_SampPB params = MakeParams(input, sample_case);
    const PF_Err error = Sample<Pixel>(suite, in_data->effect_ref, sample_case,
                                       x, y, &params, &samples[index]);
    if (error != PF_Err_NONE) return error;
    if (!OracleMatches(samples[index], Oracle<Pixel>(*input, sample_case, x, y)))
      return PF_Err_BAD_CALLBACK_PARAM;
  }

  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x)
      row[x] = samples[x % static_cast<A_long>(SampleCase::kCount)];
  }
  return PF_Err_NONE;
}

template <typename Pixel, typename Suite>
PF_Err AcquireAndRender(PF_InData* in_data, PF_EffectWorld* input,
                        PF_EffectWorld* output, const char* suite_name,
                        A_long suite_version) {
  SuiteLease lease;
  const PF_Err error = lease.Acquire(in_data->pica_basicP, suite_name, suite_version);
  if (error != PF_Err_NONE) return error;
  if (!lease.suite) return PF_Err_BAD_CALLBACK_PARAM;
  return RenderTyped<Pixel>(in_data, input, output,
                            *static_cast<const Suite*>(lease.suite));
}

PF_Err Render(PF_InData* in_data, PF_ParamDef* params[], PF_LayerDef* output) {
  if (!in_data || !params || !params[0] || !output || !output->data)
    return PF_Err_BAD_CALLBACK_PARAM;
  PF_EffectWorld* input = &params[0]->u.ld;
  if (!input->data || input->width <= 0 || input->height <= 0 ||
      output->width <= 0 || output->height <= 0)
    return PF_Err_BAD_CALLBACK_PARAM;

  const A_long bytes_per_pixel = output->rowbytes / output->width;
  if (bytes_per_pixel >= static_cast<A_long>(sizeof(PF_PixelFloat)))
    return AcquireAndRender<PF_PixelFloat, PF_SamplingFloatSuite1>(
        in_data, input, output, kPFSamplingFloatSuite,
        kPFSamplingFloatSuiteVersion1);
  if (bytes_per_pixel >= static_cast<A_long>(sizeof(PF_Pixel16)))
    return AcquireAndRender<PF_Pixel16, PF_Sampling16Suite1>(
        in_data, input, output, kPFSampling16Suite, kPFSampling16SuiteVersion1);
  if (bytes_per_pixel >= static_cast<A_long>(sizeof(PF_Pixel)))
    return AcquireAndRender<PF_Pixel, PF_Sampling8Suite1>(
        in_data, input, output, kPFSampling8Suite, kPFSampling8SuiteVersion1);
  return PF_Err_BAD_CALLBACK_PARAM;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data,
                                        PF_ParamDef* params[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT |
                            PF_OutFlag_DEEP_COLOR_AWARE;
      out_data->out_flags2 = PF_OutFlag2_FLOAT_COLOR_AWARE;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER:
      return Render(in_data, params, output);
    default:
      return PF_Err_NONE;
  }
}
