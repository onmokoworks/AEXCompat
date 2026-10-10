// Host catalog parameter probe: identifies its host from the suite catalog
// and registers a different parameter type for each answer, the way Sapphire
// does (issue #1764). GLOBAL_SETUP records whether PF Path Data Suite v1 is
// offered; PARAMS_SETUP then registers a BUTTON when it was and a CHECKBOX
// when it was not. Discovery and rendering run in different worker processes,
// so a host whose catalog differs between them hands the render worker a
// launch payload built for the other parameter table. RENDER writes what this
// process saw into every pixel so a host test can compare it with discovery.

#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_EffectSuites.h"
#include "AE_Macros.h"
#include "Param_Utils.h"
#include "SPBasic.h"

#include <cstdlib>
#include <cstring>

namespace {

constexpr int kCatalogParameter = 1;

bool g_path_data_offered = false;
bool g_registered_button = false;

PF_Err record_catalog(PF_InData* in_data) {
  g_path_data_offered = false;
  if (!in_data || !in_data->pica_basicP) return PF_Err_BAD_CALLBACK_PARAM;
  const void* suite = nullptr;
  const SPErr acquired = in_data->pica_basicP->AcquireSuite(
      kPFPathDataSuite, kPFPathDataSuiteVersion1, &suite);
  if (acquired == kSPNoError && suite) {
    g_path_data_offered = true;
    in_data->pica_basicP->ReleaseSuite(kPFPathDataSuite, kPFPathDataSuiteVersion1);
  }
  return PF_Err_NONE;
}

PF_Err params_setup(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def{};
  g_registered_button = g_path_data_offered;
  if (g_registered_button)
    PF_ADD_BUTTON("Catalog", "Run", 0, PF_ParamFlag_SUPERVISE, kCatalogParameter);
  else
    PF_ADD_CHECKBOXX("Catalog", FALSE, 0, kCatalogParameter);
  out_data->num_params = 2;
  // Owned-fixture failure controls for session launch diagnostics. These do
  // not alter the host's validation or run any installed plug-in.
  const char* failure = std::getenv("AEXCOMPAT_CATALOG_PROBE_FAILURE");
  if (failure && std::strcmp(failure, "params_setup") == 0)
    return PF_Err_BAD_CALLBACK_PARAM;
  if (failure && std::strcmp(failure, "parameter_count") == 0)
    out_data->num_params = 3;
  return PF_Err_NONE;
}

// Red reports the parameter type this process registered (255 = BUTTON,
// 0 = CHECKBOX) and green whether its GLOBAL_SETUP was offered the suite.
PF_Err render(PF_LayerDef* output) {
  if (!output || !output->data) return PF_Err_BAD_CALLBACK_PARAM;
  PF_Pixel pixel{};
  pixel.alpha = 255;
  pixel.red = g_registered_button ? 255 : 0;
  pixel.green = g_path_data_offered ? 255 : 0;
  pixel.blue = 128;
  for (A_long y = 0; y < output->height; ++y) {
    PF_Pixel* row = reinterpret_cast<PF_Pixel*>(
        reinterpret_cast<char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) row[x] = pixel;
  }
  return PF_Err_NONE;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                       PF_OutData* out_data, PF_ParamDef* params[],
                                       PF_LayerDef* output, void*) {
  (void)params;
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return record_catalog(in_data);
    case PF_Cmd_PARAMS_SETUP:
      return params_setup(in_data, out_data);
    case PF_Cmd_RENDER:
      return render(output);
    default:
      return PF_Err_NONE;
  }
}
