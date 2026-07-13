#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "Param_Utils.h"
#include "trace_writer.hpp"

#include <cstring>

namespace {
const char* selector_name(PF_Cmd cmd) {
  switch (cmd) {
    case PF_Cmd_ABOUT: return "PF_Cmd_ABOUT";
    case PF_Cmd_GLOBAL_SETUP: return "PF_Cmd_GLOBAL_SETUP";
    case PF_Cmd_GLOBAL_SETDOWN: return "PF_Cmd_GLOBAL_SETDOWN";
    case PF_Cmd_PARAMS_SETUP: return "PF_Cmd_PARAMS_SETUP";
    case PF_Cmd_RENDER: return "PF_Cmd_RENDER";
    default: return "PF_Cmd_UNKNOWN";
  }
}

aexcompat::TraceWriter& trace() {
  static aexcompat::TraceWriter writer("after_effects_manual", "AE SDK host", "pf-null-echo");
  return writer;
}

PF_Err render(PF_InData*, PF_OutData*, PF_ParamDef* params[], PF_LayerDef* output) {
  const PF_LayerDef* input = &params[0]->u.ld;
  trace().world_descriptor(output->width, output->height, output->rowbytes, "argb8");
  const A_long rows = (input->height < output->height) ? input->height : output->height;
  const A_long bytes = (input->rowbytes < output->rowbytes) ? input->rowbytes : output->rowbytes;
  for (A_long y = 0; y < rows; ++y) {
    std::memcpy(reinterpret_cast<char*>(output->data) + y * output->rowbytes,
                reinterpret_cast<const char*>(input->data) + y * input->rowbytes, bytes);
  }
  return PF_Err_NONE;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void*) {
  trace().selector_dispatch(selector_name(cmd));
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      trace().session_start();
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER:
      return render(in_data, out_data, params, output);
    case PF_Cmd_GLOBAL_SETDOWN:
      trace().session_end();
      return PF_Err_NONE;
    default:
      return PF_Err_NONE;
  }
}
