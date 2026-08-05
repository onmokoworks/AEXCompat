// Regression guard for issue #828: a SmartFX plug-in that reads its own
// parameters at the frame the host says it is rendering.
//
// The host serves smart-path parameter checkouts from a hosted ledger whose
// frame time nothing configured, so the ledger answered only t=0 and every
// other frame had its first checkout refused with PF_Err_OUT_OF_MEMORY. This
// probe reproduces that from the plug-in side and nothing else: it checks out
// one slider at `in_data->current_time` in each selector that is allowed to,
// returns the host's own error verbatim when a checkout fails, and otherwise
// copies input to output.
//
// The three checkout sites are deliberate. SMART_RENDER is where Displacement
// failed; SMART_PRE_RENDER runs first, so a fix that only covered SMART_RENDER
// would still fail here; and QUERY_DYNAMIC_FLAGS runs before either, which the
// SDK explicitly permits ("the effect may examine the values of its parameters
// at the current time (except layer parameters) by checking them out").
//
// The probe never invents an error code: every failure it reports came out of
// the host.

#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_EffectCBSuites.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#include <cstring>

namespace {

constexpr A_long kSliderSlot = 1;

// A checkout at the frame's own time is the one time every plug-in may ask for
// without advertising wide time input, so a failure here is the host's.
PF_Err CheckOutOwnTime(PF_InData* in_data) {
  PF_ParamDef definition{};
  AEFX_CLR_STRUCT(definition);
  const PF_Err error = PF_CHECKOUT_PARAM(in_data, kSliderSlot, in_data->current_time,
                                         in_data->time_step, in_data->time_scale,
                                         &definition);
  if (error != PF_Err_NONE) return error;
  return PF_CHECKIN_PARAM(in_data, &definition);
}

PF_Err GlobalSetup(PF_OutData* out_data) {
  out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
  out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
  out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER |
      PF_OutFlag2_SUPPORTS_QUERY_DYNAMIC_FLAGS;
  return PF_Err_NONE;
}

PF_Err ParamsSetup(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def{};
  AEFX_CLR_STRUCT(def);
  PF_ADD_SLIDER("Amount", 0, 100, 0, 100, 50, kSliderSlot);
  out_data->num_params = kSliderSlot + 1;
  return PF_Err_NONE;
}

PF_Err SmartPreRender(PF_InData* in_data, PF_PreRenderExtra* extra) {
  if (!extra || !extra->input || !extra->output || !extra->cb)
    return PF_Err_BAD_CALLBACK_PARAM;
  const PF_Err checkout_error = CheckOutOwnTime(in_data);
  if (checkout_error != PF_Err_NONE) return checkout_error;
  PF_RenderRequest request = extra->input->output_request;
  PF_CheckoutResult input{};
  const PF_Err error = extra->cb->checkout_layer(
      in_data->effect_ref, 0, 0, &request, in_data->current_time, in_data->time_step,
      in_data->time_scale, &input);
  if (error != PF_Err_NONE) return error;
  extra->output->result_rect = input.result_rect;
  extra->output->max_result_rect = input.max_result_rect;
  return PF_Err_NONE;
}

PF_Err SmartRender(PF_InData* in_data, PF_SmartRenderExtra* extra) {
  if (!extra || !extra->cb) return PF_Err_BAD_CALLBACK_PARAM;
  const PF_Err checkout_error = CheckOutOwnTime(in_data);
  if (checkout_error != PF_Err_NONE) return checkout_error;
  PF_EffectWorld* input = nullptr;
  PF_EffectWorld* output = nullptr;
  PF_Err error = extra->cb->checkout_layer_pixels(in_data->effect_ref, 0, &input);
  if (!error) error = extra->cb->checkout_output(in_data->effect_ref, &output);
  if (error != PF_Err_NONE) return error;
  if (!input || !output || !input->data || !output->data)
    return PF_Err_BAD_CALLBACK_PARAM;
  // 8-bit only: the copy is `width * 4` bytes per row. The gate under test does
  // not depend on depth, so the probe does not open the deep paths; a caller
  // that wants one must widen this first rather than get a short copy.
  const A_long height = input->height < output->height ? input->height : output->height;
  const A_long width = input->width < output->width ? input->width : output->width;
  for (A_long y = 0; y < height; ++y)
    std::memcpy(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                reinterpret_cast<A_u_char*>(input->data) + y * input->rowbytes,
                static_cast<std::size_t>(width) * 4);
  return PF_Err_NONE;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void* extra) {
  (void)params;
  (void)output;
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      return GlobalSetup(out_data);
    case PF_Cmd_PARAMS_SETUP:
      return ParamsSetup(in_data, out_data);
    case PF_Cmd_QUERY_DYNAMIC_FLAGS:
      return CheckOutOwnTime(in_data);
    case PF_Cmd_SMART_PRE_RENDER:
      return SmartPreRender(in_data, static_cast<PF_PreRenderExtra*>(extra));
    case PF_Cmd_SMART_RENDER:
      return SmartRender(in_data, static_cast<PF_SmartRenderExtra*>(extra));
    default:
      return PF_Err_NONE;
  }
}
