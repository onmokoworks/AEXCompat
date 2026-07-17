#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectUI.h"
#include "Param_Utils.h"
#include "trace_writer.hpp"

#include <new>
#include <vector>
#include <windows.h>

namespace {
enum Mode { kModeNone = 1, kModeCrash = 2, kModeHang = 3, kModeBigAlloc = 4, kModePfError = 5 };
constexpr int kModeParam = 1;
constexpr int kStageParam = 2;
enum Stage { kStageRender = 1, kStageEvent = 2 };

const char* selector_name(PF_Cmd cmd) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP: return "PF_Cmd_GLOBAL_SETUP";
    case PF_Cmd_GLOBAL_SETDOWN: return "PF_Cmd_GLOBAL_SETDOWN";
    case PF_Cmd_PARAMS_SETUP: return "PF_Cmd_PARAMS_SETUP";
    case PF_Cmd_RENDER: return "PF_Cmd_RENDER";
    case PF_Cmd_EVENT: return "PF_Cmd_EVENT";
    default: return "PF_Cmd_UNKNOWN";
  }
}

aexcompat::TraceWriter& trace() {
  static aexcompat::TraceWriter writer("after_effects_manual", "AE SDK host", "pf-crashkit");
  return writer;
}

PF_Err params_setup(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def{};
  PF_ADD_POPUP("Fault mode", 5, kModeNone,
               "none|crash|hang|bigalloc|pf_error", kModeParam);
  PF_ADD_POPUP("Fault stage", 2, kStageRender, "render|event", kStageParam);
  PF_CustomUIInfo ui{};
  ui.events = PF_CustomEFlag_EFFECT;
  PF_Err err = in_data->inter.register_ui(in_data->effect_ref, &ui);
  out_data->num_params = 3;
  if (err) return err;
  return PF_Err_NONE;
}

PF_Err inject_fault(PF_ParamDef* params[]) {
  const A_long mode = params[kModeParam]->u.pd.value;
  switch (mode) {
    case kModeNone:
      return PF_Err_NONE;
    case kModeCrash: {
      trace().error("intentional_crash", "human-selected synthetic fault");
      volatile int* pointer = nullptr;
      *pointer = 1;
      return PF_Err_INTERNAL_STRUCT_DAMAGED;
    }
    case kModeHang:
      trace().error("intentional_hang", "human-selected synthetic fault");
      Sleep(INFINITE);
      return PF_Err_NONE;
    case kModeBigAlloc:
      trace().error("intentional_bigalloc", "human-selected synthetic fault");
      try {
        std::vector<unsigned char> allocation(static_cast<std::size_t>(1024) * 1024 * 1024);
        return allocation.empty() ? PF_Err_OUT_OF_MEMORY : PF_Err_NONE;
      } catch (const std::bad_alloc&) {
        return PF_Err_OUT_OF_MEMORY;
      }
    case kModePfError:
      trace().error("intentional_pf_error", "human-selected synthetic fault");
      return PF_Err_INTERNAL_STRUCT_DAMAGED;
    default:
      return PF_Err_BAD_CALLBACK_PARAM;
  }
}

PF_Err fault_at_stage(PF_ParamDef* params[], Stage stage) {
  return params[kStageParam]->u.pd.value == stage ? inject_fault(params) : PF_Err_NONE;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef*, void* extra) {
  if (cmd == PF_Cmd_GLOBAL_SETUP) trace().session_start();
  trace().selector_dispatch(selector_name(cmd));
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      break;
    case PF_Cmd_PARAMS_SETUP:
      return params_setup(in_data, out_data);
    case PF_Cmd_RENDER:
      return fault_at_stage(params, kStageRender);
    case PF_Cmd_EVENT:
      if (extra && static_cast<PF_EventExtra*>(extra)->e_type == PF_Event_IDLE)
        return fault_at_stage(params, kStageEvent);
      break;
    case PF_Cmd_GLOBAL_SETDOWN:
      trace().session_end();
      break;
    default:
      break;
  }
  return PF_Err_NONE;
}
