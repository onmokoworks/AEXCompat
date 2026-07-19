// Parameter echo probe: renders its float slider value into every output
// pixel so a host test can assert, byte for byte, which parameter value a
// frame was rendered with. Built for the render session v:2 per-frame
// parameter tests (issue #107): two session frames with different slider
// values must produce two different, self-computable output patterns.

#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#include <cmath>

namespace {

constexpr int kEchoSlider = 1;

unsigned char clamp_channel(double value) {
  if (value <= 0.0) return 0;
  if (value >= 255.0) return 255;
  return static_cast<unsigned char>(std::lround(value));
}

PF_Err render(PF_ParamDef* params[], PF_LayerDef* output) {
  if (!params || !params[kEchoSlider] || !output || !output->data)
    return PF_Err_BAD_CALLBACK_PARAM;
  const double value = params[kEchoSlider]->u.fs_d.value;
  PF_Pixel pixel{};
  pixel.alpha = 255;
  pixel.red = clamp_channel(value);
  pixel.green = static_cast<unsigned char>(255 - pixel.red);
  pixel.blue = 128;
  for (A_long y = 0; y < output->height; ++y) {
    PF_Pixel* row = reinterpret_cast<PF_Pixel*>(
        reinterpret_cast<char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) row[x] = pixel;
  }
  return PF_Err_NONE;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                       PF_OutData* out_data, PF_ParamDef* params[],
                                       PF_LayerDef* output, void*) {
  (void)in_data;  // consumed by the PF_ADD_FLOAT_SLIDERX macro expansion
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP: {
      PF_ParamDef def{};
      PF_ADD_FLOAT_SLIDERX("Echo", 0, 255, 0, 255, 0, 1,
                           PF_ValueDisplayFlag_NONE, 0, kEchoSlider);
      out_data->num_params = 2;
      return PF_Err_NONE;
    }
    case PF_Cmd_RENDER:
      return render(params, output);
    default:
      return PF_Err_NONE;
  }
}
