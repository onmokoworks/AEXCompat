#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

#include <cstring>

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void*) {
  if (cmd == PF_Cmd_GLOBAL_SETUP) {
    out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
    out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
  } else if (cmd == PF_Cmd_PARAMS_SETUP) {
    PF_ParamDef path{};
    path.param_type = PF_Param_PATH;
    path.uu.id = 1;
    std::strcpy(path.name, "Observed Path");
    path.u.path_d.dephault = AEXCOMPAT_PATH_DEFAULT;
    PF_Err err = PF_ADD_PARAM(in_data, -1, &path);
    out_data->num_params = 2;
    return err;
  } else if (cmd == PF_Cmd_RENDER) {
    if (!params || !params[0] || !params[1] ||
        params[1]->u.path_d.path_id != AEXCOMPAT_EXPECT_PATH_ID)
      return PF_Err_BAD_CALLBACK_PARAM;
    if (!output || !output->data || !params[0]->u.ld.data ||
        output->width != params[0]->u.ld.width || output->height != params[0]->u.ld.height)
      return PF_Err_BAD_CALLBACK_PARAM;
    for (A_long y = 0; y < output->height; ++y)
      std::memcpy(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                  reinterpret_cast<A_u_char*>(params[0]->u.ld.data) + y * params[0]->u.ld.rowbytes,
                  static_cast<size_t>(output->width) * 4);
  }
  return PF_Err_NONE;
}
