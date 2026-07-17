#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

#include <cstring>

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
#if AEXCOMPAT_ADVERTISE_SHUTTER
      out_data->out_flags |= PF_OutFlag_I_USE_SHUTTER_ANGLE;
#endif
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER:
      if (!in_data || in_data->shutter_angle != 32768 || in_data->shutter_phase != -16384)
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      if (!output || !output->data || output->rowbytes < output->width * 4)
        return PF_Err_BAD_CALLBACK_PARAM;
      for (A_long y = 0; y < output->height; ++y)
        std::memset(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                    0x36, static_cast<std::size_t>(output->width) * 4);
      return PF_Err_NONE;
    default:
      return PF_Err_NONE;
  }
}
