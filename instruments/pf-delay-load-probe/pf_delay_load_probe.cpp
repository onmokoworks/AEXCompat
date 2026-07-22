#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#include <cstring>

extern "C" __declspec(dllimport) int issue60_delay_value();

extern "C" __declspec(dllexport) int Issue60InvokeDelayLoad() {
  return issue60_delay_value();
}

using PluginDataCallback = int32_t(__cdecl*)(
    void*, const unsigned char*, const unsigned char*, const unsigned char*,
    const unsigned char*, int32_t, int32_t, int32_t, int32_t);

extern "C" __declspec(dllexport) int32_t __cdecl PluginDataEntryFunction(
    void* context, PluginDataCallback callback, void*, const char*, const char*) {
  if (!callback) return 4;
  return callback(
      context, reinterpret_cast<const unsigned char*>("Delay Load Probe"),
      reinterpret_cast<const unsigned char*>("AEXCompat Delay Load Probe"),
      reinterpret_cast<const unsigned char*>("AEXCompat Tests"),
      reinterpret_cast<const unsigned char*>("EffectMain"),
      static_cast<int32_t>('eFKT'), 13, 28, 8);
}

namespace {
PF_Err render(PF_ParamDef* params[], PF_LayerDef* output) {
  const PF_LayerDef* input = &params[0]->u.ld;
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
                                        PF_OutData* out_data,
                                        PF_ParamDef* params[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      {
        PF_ParamDef def{};
        PF_ADD_FLOAT_SLIDERX("Gate", 0, 1, 0, 1, 0, 1,
                             PF_ValueDisplayFlag_NONE, 0, 1);
        out_data->num_params = 2;
      }
      return PF_Err_NONE;
    case PF_Cmd_SEQUENCE_SETUP:
      // The first reference to issue60_delay_dependency.dll intentionally
      // occurs after initial plug-in admission.
      return Issue60InvokeDelayLoad() == 0x60 ? PF_Err_NONE : PF_Err_INTERNAL_STRUCT_DAMAGED;
    case PF_Cmd_RENDER:
      return render(params, output);
    default:
      return PF_Err_NONE;
  }
}
