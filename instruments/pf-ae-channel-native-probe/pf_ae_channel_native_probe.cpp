#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_ChannelSuites.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace {
template <typename Pixel, typename Channel, int Maximum>
PF_Err RenderCoverage(const PF_ChannelChunk& chunk, PF_LayerDef* output) {
  if (!chunk.dataPV || !output || !output->data || chunk.dimensionL != 1 ||
      chunk.widthL != output->width || chunk.heightL != output->height)
    return PF_Err_BAD_CALLBACK_PARAM;
  for (A_long y = 0; y < output->height; ++y) {
    const auto* source = reinterpret_cast<const float*>(
        static_cast<const std::byte*>(chunk.dataPV) +
        static_cast<std::ptrdiff_t>(y) * chunk.row_bytesL);
    auto* target = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) {
      const float coverage = std::clamp(source[x], 0.0f, 1.0f);
      Channel value{};
      if constexpr (Maximum == 1) value = coverage;
      else value = static_cast<Channel>(coverage * Maximum + 0.5f);
      target[x].alpha = static_cast<Channel>(Maximum);
      target[x].red = target[x].green = target[x].blue = value;
    }
  }
  return PF_Err_NONE;
}

PF_Err Render(PF_InData* in_data, PF_OutData* out_data, PF_LayerDef* output) {
  if (!in_data || !out_data || !output || !in_data->pica_basicP)
    return PF_Err_BAD_CALLBACK_PARAM;
  const PF_ChannelSuite1* suite = nullptr;
  PF_Err error = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFChannelSuite1, kPFChannelSuiteVersion1,
      reinterpret_cast<const void**>(&suite)));
  PF_ChannelRef ref{};
  PF_ChannelDesc desc{};
  PF_ChannelChunk chunk{};
  PF_Boolean found = FALSE;
  if (!error && suite)
    error = suite->PF_GetLayerChannelTypedRefAndDesc(
        in_data->effect_ref, 0, PF_ChannelType_COVERAGE, &found, &ref, &desc);
  if (!error && !found) error = PF_Err_UNRECOGNIZED_PARAM_TYPE;
  if (!error)
    error = suite->PF_CheckoutLayerChannel(in_data->effect_ref, &ref,
        in_data->current_time, in_data->time_step, in_data->time_scale,
        PF_DataType_FLOAT, &chunk);
  if (!error) {
    if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_PixelFloat)))
      error = RenderCoverage<PF_PixelFloat, PF_FpShort, 1>(chunk, output);
    else if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_Pixel16)))
      error = RenderCoverage<PF_Pixel16, A_u_short, PF_MAX_CHAN16>(chunk, output);
    else
      error = RenderCoverage<PF_Pixel8, A_u_char, PF_MAX_CHAN8>(chunk, output);
  }
  if (chunk.dataPV) {
    const PF_Err checkin = suite->PF_CheckinLayerChannel(in_data->effect_ref, &ref, &chunk);
    if (!error) error = checkin;
  }
  in_data->pica_basicP->ReleaseSuite(kPFChannelSuite1, kPFChannelSuiteVersion1);
  return error;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
      out_data->out_flags2 = PF_OutFlag2_FLOAT_COLOR_AWARE;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER:
      return Render(in_data, out_data, output);
    default:
      return PF_Err_NONE;
  }
}
