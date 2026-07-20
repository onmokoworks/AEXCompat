// Layer-parameter + slider classic-render probe. Declares a PF_Param_LAYER
// (secondary layer) plus a float slider and composites both into the output on
// the classic PF_Cmd_RENDER path, so a host A/B test can prove, byte for byte,
// that the resident render session and the one-shot argv transport deliver the
// same secondary-layer pixels and the same user-parameter value. This is the
// fixture the render_session_wrapper A/B (issue #98 W1-4) needs: pf_sampling_probe
// declares num_params=1 (input only), so a secondary layer has no slot to land
// in (render_error=-3) and no user parameter can vary. Mirrors the
// pf_parameter_echo_probe slider->pixel pattern (issue #191) and adds the layer
// slot; issue #195.
//
// 8-bit only, and deliberately no PF_OutFlag2_SUPPORTS_SMART_RENDER: that keeps
// the worker on the classic params[] fill path (params[1]->u.ld is the layer,
// params[2]->u.fs_d.value is the slider) rather than the smart checkout path.

#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#include <cmath>

namespace {

// params[0] is the implicit primary input layer; the first PF_ADD_* call becomes
// params[1], the second params[2]. Keep this order in sync with PARAMS_SETUP.
enum ParamIndex { kInput = 0, kLayer = 1, kSlider = 2, kNumParams = 3 };

unsigned char clamp_channel(long value) {
  if (value < 0) return 0;
  if (value > 255) return 255;
  return static_cast<unsigned char>(value);
}

// The slider is declared over 0..255; snap it to an integer amount so the
// output is a pure integer function of (input, layer, slider) and reproduces
// byte for byte on both transports.
int slider_amount(double value) {
  if (value <= 0.0) return 0;
  if (value >= 255.0) return 255;
  return static_cast<int>(std::lround(value));
}

// Clamp-sample so the probe stays deterministic even if a world's dimensions
// differ from the output; the A/B fixture feeds equal dimensions, but a clamp
// keeps a mismatched host from reading out of bounds.
PF_Pixel sample(const PF_LayerDef* world, A_long x, A_long y) {
  const A_long cy = y < world->height ? y : world->height - 1;
  const A_long cx = x < world->width ? x : world->width - 1;
  const PF_Pixel* row = reinterpret_cast<const PF_Pixel*>(
      reinterpret_cast<const char*>(world->data) + cy * world->rowbytes);
  return row[cx];
}

PF_Err render(PF_ParamDef* params[], PF_LayerDef* output) {
  if (!params || !params[kInput] || !params[kLayer] || !params[kSlider] ||
      !output || !output->data)
    return PF_Err_BAD_CALLBACK_PARAM;
  const PF_LayerDef* input = &params[kInput]->u.ld;
  const PF_LayerDef* layer = &params[kLayer]->u.ld;
  if (!input->data || input->width <= 0 || input->height <= 0 ||
      !layer->data || layer->width <= 0 || layer->height <= 0 ||
      output->width <= 0 || output->height <= 0)
    return PF_Err_BAD_CALLBACK_PARAM;
  const int s = slider_amount(params[kSlider]->u.fs_d.value);
  for (A_long y = 0; y < output->height; ++y) {
    PF_Pixel* dst = reinterpret_cast<PF_Pixel*>(
        reinterpret_cast<char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) {
      const PF_Pixel in = sample(input, x, y);
      const PF_Pixel lp = sample(layer, x, y);
      PF_Pixel out{};
      out.alpha = 255;
      // Each channel depends on a distinct pair so any one input changing is
      // observable in the output: red = input + slider, green = layer - slider,
      // blue = mean(input, layer).
      out.red = clamp_channel(static_cast<long>(in.red) + s - 128);
      out.green = clamp_channel(static_cast<long>(lp.green) + 128 - s);
      out.blue = static_cast<unsigned char>(
          (static_cast<long>(in.blue) + lp.blue + 1) / 2);
      dst[x] = out;
    }
  }
  return PF_Err_NONE;
}

PF_Err setup_params(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def{};
  PF_ADD_LAYER("Layer", PF_LayerDefault_MYSELF, kLayer);
  PF_ADD_FLOAT_SLIDERX("Amount", 0, 255, 0, 255, 0, 1,
                       PF_ValueDisplayFlag_NONE, 0, kSlider);
  out_data->num_params = kNumParams;
  return PF_Err_NONE;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                       PF_OutData* out_data, PF_ParamDef* params[],
                                       PF_LayerDef* output, void*) {
  (void)in_data;  // consumed by the PF_ADD_* macro expansions
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      return setup_params(in_data, out_data);
    case PF_Cmd_RENDER:
      return render(params, output);
    default:
      return PF_Err_NONE;
  }
}
