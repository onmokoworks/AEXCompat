#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "SPBasic.h"
#include "trace_writer.hpp"

namespace {
const char* selector_name(PF_Cmd cmd) {
  switch (cmd) {
    case PF_Cmd_ABOUT: return "PF_Cmd_ABOUT";
    case PF_Cmd_GLOBAL_SETUP: return "PF_Cmd_GLOBAL_SETUP";
    case PF_Cmd_GLOBAL_SETDOWN: return "PF_Cmd_GLOBAL_SETDOWN";
    case PF_Cmd_PARAMS_SETUP: return "PF_Cmd_PARAMS_SETUP";
    case PF_Cmd_SEQUENCE_SETUP: return "PF_Cmd_SEQUENCE_SETUP";
    case PF_Cmd_SEQUENCE_SETDOWN: return "PF_Cmd_SEQUENCE_SETDOWN";
    case PF_Cmd_RENDER: return "PF_Cmd_RENDER";
    default: return "PF_Cmd_UNKNOWN";
  }
}

aexcompat::TraceWriter& trace() {
  static aexcompat::TraceWriter writer("after_effects_manual", "AE SDK host",
                                       "pf-callback-tracer");
  return writer;
}

void trace_world(const PF_InData* in_data, const PF_LayerDef* output) {
  if (output) {
    trace().world_descriptor(output->width, output->height, output->rowbytes, "argb8");
  }
  if (in_data) trace().callback_invoke();
}

void census_suites(PF_InData* in_data) {
  if (!in_data || !in_data->pica_basicP) return;
  struct SuiteRequest { const char* name; A_long version; };
  const SuiteRequest requests[] = {
      {"PF World Suite", 2}, {"PF Iterate8 Suite", 1}, {"PF Handle Suite", 1}};
  for (const auto& request : requests) {
    const void* suite = nullptr;
    const SPErr acquired = in_data->pica_basicP->AcquireSuite(request.name, request.version, &suite);
    trace().suite_acquire(request.name, request.version, acquired == kSPNoError && suite != nullptr);
    if (acquired == kSPNoError && suite) {
      in_data->pica_basicP->ReleaseSuite(request.name, request.version);
      trace().suite_release(request.name, request.version, true);
    }
  }
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef* output, void*) {
  if (cmd == PF_Cmd_GLOBAL_SETUP) trace().session_start();
  trace().selector_dispatch(selector_name(cmd));
  trace_world(in_data, output);
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      census_suites(in_data);
      break;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      break;
    case PF_Cmd_GLOBAL_SETDOWN:
      trace().session_end();
      break;
    default:
      break;
  }
  return PF_Err_NONE;
}
