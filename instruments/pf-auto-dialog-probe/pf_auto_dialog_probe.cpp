#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

#include <cstring>

namespace {
enum Phase { kCold, kGlobalReady, kSequenceReady, kDialogComplete, kSequenceClosed };
Phase g_phase = kCold;
}

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData*,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef*, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      g_phase = kGlobalReady;
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_I_DO_DIALOG;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return g_phase == kGlobalReady ? PF_Err_NONE : PF_Err_INTERNAL_STRUCT_DAMAGED;
    case PF_Cmd_SEQUENCE_SETUP:
      if (g_phase != kGlobalReady) return PF_Err_INTERNAL_STRUCT_DAMAGED;
      g_phase = kSequenceReady;
      out_data->out_flags |= PF_OutFlag_SEND_DO_DIALOG;
      return PF_Err_NONE;
    case PF_Cmd_DO_DIALOG:
      if (g_phase != kSequenceReady) return PF_Err_INTERNAL_STRUCT_DAMAGED;
      g_phase = kDialogComplete;
      strcpy_s(out_data->return_msg, "automatic options dialog completed");
      return PF_Err_NONE;
    case PF_Cmd_SEQUENCE_SETDOWN:
      if (g_phase != kDialogComplete) return PF_Err_INTERNAL_STRUCT_DAMAGED;
      g_phase = kSequenceClosed;
      return PF_Err_NONE;
    case PF_Cmd_GLOBAL_SETDOWN:
      if (g_phase != kSequenceClosed) return PF_Err_INTERNAL_STRUCT_DAMAGED;
      g_phase = kCold;
      return PF_Err_NONE;
    default:
      return PF_Err_NONE;
  }
}
