#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

#include <cstring>

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void* extra) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
#if AEXCOMPAT_SMART_WIDE_TEST
      out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER;
#endif
#if AEXCOMPAT_ADVERTISE_AUTOMATIC_WIDE_TIME
      out_data->out_flags2 |= PF_OutFlag2_AUTOMATIC_WIDE_TIME_INPUT;
#endif
#if AEXCOMPAT_ADVERTISE_WIDE_TIME
      out_data->out_flags |= PF_OutFlag_WIDE_TIME_INPUT;
#endif
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER: {
      PF_ParamDef checkout{};
      const PF_Err checkout_err = PF_CHECKOUT_PARAM(
          in_data, 0, in_data->current_time + in_data->time_step,
          in_data->time_step, in_data->time_scale, &checkout);
#if AEXCOMPAT_ADVERTISE_WIDE_TIME
      if (checkout_err != PF_Err_NONE) return PF_Err_INTERNAL_STRUCT_DAMAGED;
      const PF_Err checkin_err = PF_CHECKIN_PARAM(in_data, &checkout);
      if (checkin_err != PF_Err_NONE) return checkin_err;
#else
      if (checkout_err == PF_Err_NONE) {
        PF_CHECKIN_PARAM(in_data, &checkout);
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      }
#endif
      if (!output || !output->data || output->rowbytes < output->width * 4)
        return PF_Err_BAD_CALLBACK_PARAM;
      for (A_long y = 0; y < output->height; ++y)
        std::memset(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                    0x4d, static_cast<std::size_t>(output->width) * 4);
      return PF_Err_NONE;
    }
#if AEXCOMPAT_SMART_WIDE_TEST
    case PF_Cmd_SMART_PRE_RENDER: {
      auto* smart = static_cast<PF_PreRenderExtra*>(extra);
      if (!smart || !smart->input || !smart->output || !smart->cb)
        return PF_Err_BAD_CALLBACK_PARAM;
      PF_RenderRequest request = smart->input->output_request;
      PF_CheckoutResult checkout{};
      const PF_Err error = smart->cb->checkout_layer(
          in_data->effect_ref, 0, 0, &request,
          in_data->current_time + in_data->time_step,
          in_data->time_step, in_data->time_scale, &checkout);
#if AEXCOMPAT_EXPECT_SMART_DENIED
      if (error == PF_Err_NONE) return PF_Err_INTERNAL_STRUCT_DAMAGED;
      smart->output->result_rect = request.rect;
      smart->output->max_result_rect = request.rect;
      return PF_Err_NONE;
#else
      if (error != PF_Err_NONE) return error;
      smart->output->result_rect = checkout.result_rect;
      smart->output->max_result_rect = checkout.max_result_rect;
      return PF_Err_NONE;
#endif
    }
    case PF_Cmd_SMART_RENDER: {
      auto* smart = static_cast<PF_SmartRenderExtra*>(extra);
      if (!smart || !smart->cb) return PF_Err_BAD_CALLBACK_PARAM;
      PF_EffectWorld* input = nullptr;
      PF_EffectWorld* smart_output = nullptr;
      PF_Err error = smart->cb->checkout_layer_pixels(in_data->effect_ref, 0, &input);
      if (!error) error = smart->cb->checkout_output(in_data->effect_ref, &smart_output);
      if (error) return error;
      for (A_long y = 0; y < input->height; ++y)
        std::memcpy(reinterpret_cast<A_u_char*>(smart_output->data) + y * smart_output->rowbytes,
                    reinterpret_cast<A_u_char*>(input->data) + y * input->rowbytes,
                    static_cast<std::size_t>(input->width) * 4);
      return PF_Err_NONE;
    }
#endif
    default:
      return PF_Err_NONE;
  }
}
