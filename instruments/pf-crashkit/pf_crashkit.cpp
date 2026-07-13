#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "Param_Utils.h"
#include "trace_writer.hpp"

#include <new>
#include <vector>
#include <windows.h>

namespace {
enum Mode { kModeNone = 1, kModeCrash = 2, kModeHang = 3, kModeBigAlloc = 4, kModePfError = 5 };
constexpr int kModeParam = 1;

const char* selector_name(PF_Cmd cmd) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP: return "PF_Cmd_GLOBAL_SETUP";
    case PF_Cmd_GLOBAL_SETDOWN: return "PF_Cmd_GLOBAL_SETDOWN";
    case PF_Cmd_PARAMS_SETUP: return "PF_Cmd_PARAMS_SETUP";
    case PF_Cmd_RENDER: return "PF_Cmd_RENDER";
    default: return "PF_Cmd_UNKNOWN";
  }
}

aexcompat::TraceWriter& trace() {
  static aexcompat::TraceWriter writer("after_effects_manual", "AE SDK host", "pf-crashkit");
  return writer;
}

PF_Err params_setup(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef mode{};
  PF_ADD_POPUP("Fault mode", 5, kModeNone,
               "none|crash|hang|bigalloc|pf_error", kModeParam);
  out_data->num_params = 2;
  return PF_Err_NONE;
}

PF_Err render(PF_ParamDef* params[]) {
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
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef*, void*) {
  if (cmd == PF_Cmd_GLOBAL_SETUP) trace().session_start();
  trace().selector_dispatch(selector_name(cmd));
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      break;
    case PF_Cmd_PARAMS_SETUP:
      return params_setup(in_data, out_data);
    case PF_Cmd_RENDER:
      return render(params);
    case PF_Cmd_GLOBAL_SETDOWN:
      trace().session_end();
      break;
    default:
      break;
  }
  return PF_Err_NONE;
}
