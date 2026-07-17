#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

namespace {
enum Phase { kCold, kGlobalReady, kSequenceReady, kSequenceClosed };
Phase g_phase = kCold;
}

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData*,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef*, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      g_phase = kGlobalReady;
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_NOP_RENDER | PF_OutFlag_PIX_INDEPENDENT;
      out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return g_phase == kGlobalReady ? PF_Err_NONE : PF_Err_INTERNAL_STRUCT_DAMAGED;
    case PF_Cmd_SEQUENCE_SETUP:
      if (g_phase != kGlobalReady) return PF_Err_INTERNAL_STRUCT_DAMAGED;
      g_phase = kSequenceReady;
      return PF_Err_NONE;
    case PF_Cmd_SEQUENCE_SETDOWN:
      if (g_phase != kSequenceReady) return PF_Err_INTERNAL_STRUCT_DAMAGED;
      g_phase = kSequenceClosed;
      return PF_Err_NONE;
    case PF_Cmd_FRAME_SETUP:
    case PF_Cmd_RENDER:
    case PF_Cmd_FRAME_SETDOWN:
    case PF_Cmd_SMART_PRE_RENDER:
    case PF_Cmd_SMART_RENDER:
      return PF_Err_INTERNAL_STRUCT_DAMAGED;
    case PF_Cmd_GLOBAL_SETDOWN:
      if (g_phase != kSequenceClosed) return PF_Err_INTERNAL_STRUCT_DAMAGED;
      g_phase = kCold;
      return PF_Err_NONE;
    default:
      return PF_Err_NONE;
  }
}
