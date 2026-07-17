#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

#include <cstring>

#ifndef AEXCOMPAT_RESIZE_FLAG
#define AEXCOMPAT_RESIZE_FLAG 0
#endif

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData*,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | AEXCOMPAT_RESIZE_FLAG;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_FRAME_SETUP:
      out_data->width = params[0]->u.ld.width + AEXCOMPAT_RESIZE_DELTA;
      out_data->height = params[0]->u.ld.height + AEXCOMPAT_RESIZE_DELTA;
      return PF_Err_NONE;
    case PF_Cmd_RENDER:
#if AEXCOMPAT_EXPECT_DENIED
      return PF_Err_INTERNAL_STRUCT_DAMAGED;
#else
      if (!output || !output->data || output->width <= 0 || output->height <= 0 ||
          output->rowbytes < output->width * 4)
        return PF_Err_BAD_CALLBACK_PARAM;
      for (A_long y = 0; y < output->height; ++y)
        std::memset(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                    0x7f, static_cast<std::size_t>(output->width) * 4);
      return PF_Err_NONE;
#endif
    default:
      return PF_Err_NONE;
  }
}
