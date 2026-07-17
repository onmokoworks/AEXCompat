#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

#include <cstring>

namespace {
PF_Err mutate_and_copy(PF_EffectWorld* input, PF_EffectWorld* output) {
  if (!input || !input->data || !output || !output->data ||
      input->width != output->width || input->height != output->height ||
      input->rowbytes < input->width * 4 || output->rowbytes < output->width * 4)
    return PF_Err_BAD_CALLBACK_PARAM;
  auto* first = reinterpret_cast<A_u_char*>(input->data);
  first[0] = 255;
  first[1] = 17;
  first[2] = 34;
  first[3] = 51;
  for (A_long y = 0; y < input->height; ++y)
    std::memcpy(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                reinterpret_cast<A_u_char*>(input->data) + y * input->rowbytes,
                static_cast<std::size_t>(input->width) * 4);
  return PF_Err_NONE;
}
}

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void* extra) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER;
#if AEXCOMPAT_ADVERTISE_INPUT_WRITE
      out_data->out_flags |= PF_OutFlag_I_WRITE_INPUT_BUFFER;
#endif
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER: {
      return mutate_and_copy(&params[0]->u.ld, output);
    }
    case PF_Cmd_SMART_PRE_RENDER: {
      auto* smart = static_cast<PF_PreRenderExtra*>(extra);
      if (!smart || !smart->input || !smart->output || !smart->cb)
        return PF_Err_BAD_CALLBACK_PARAM;
      PF_RenderRequest request = smart->input->output_request;
      PF_CheckoutResult checkout{};
      const PF_Err error = smart->cb->checkout_layer(
          in_data->effect_ref, 0, 0, &request, in_data->current_time,
          in_data->time_step, in_data->time_scale, &checkout);
      if (!error) {
        smart->output->result_rect = checkout.result_rect;
        smart->output->max_result_rect = checkout.max_result_rect;
      }
      return error;
    }
    case PF_Cmd_SMART_RENDER: {
      auto* smart = static_cast<PF_SmartRenderExtra*>(extra);
      if (!smart || !smart->cb) return PF_Err_BAD_CALLBACK_PARAM;
      PF_EffectWorld* input = nullptr;
      PF_EffectWorld* smart_output = nullptr;
      PF_Err error = smart->cb->checkout_layer_pixels(in_data->effect_ref, 0, &input);
      if (!error) error = smart->cb->checkout_output(in_data->effect_ref, &smart_output);
      return error ? error : mutate_and_copy(input, smart_output);
    }
    default:
      return PF_Err_NONE;
  }
}
