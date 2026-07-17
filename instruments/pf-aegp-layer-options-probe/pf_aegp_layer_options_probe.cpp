#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

#include <cstring>

namespace {
AEGP_PluginID g_plugin_id = 0;

template <typename T>
void keep_first(PF_Err& first, T candidate) {
  if (!first && candidate) first = static_cast<PF_Err>(candidate);
}

bool same_time(const A_Time& left, const A_Time& right) {
  return left.value == right.value && left.scale == right.scale;
}

PF_Err global_setup(PF_InData* in_data, PF_OutData* out_data) {
  if (!in_data || !in_data->pica_basicP || !out_data) return PF_Err_BAD_CALLBACK_PARAM;
  out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
  out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;

  const AEGP_UtilitySuite3* utility = nullptr;
  PF_Err err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3,
      reinterpret_cast<const void**>(&utility)));
  if (!err && (!utility || !utility->AEGP_RegisterWithAEGP)) err = PF_Err_INVALID_CALLBACK;
  if (!err) err = utility->AEGP_RegisterWithAEGP(
      nullptr, "PF AEGP Layer Options Probe", &g_plugin_id);
  if (utility) keep_first(err, in_data->pica_basicP->ReleaseSuite(
                                  kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3));
  return err;
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !in_data->effect_ref || !output ||
      !output->data || output->height < 0 || output->rowbytes < 0 || !g_plugin_id) {
    return PF_Err_BAD_CALLBACK_PARAM;
  }

  const AEGP_PFInterfaceSuite1* pf = nullptr;
  const AEGP_EffectSuite3* effects = nullptr;
  const AEGP_LayerRenderOptionsSuite1* suite = nullptr;
  AEGP_LayerH layer = nullptr;
  AEGP_EffectRefH effect = nullptr;
  AEGP_LayerRenderOptionsH from_layer = nullptr;
  AEGP_LayerRenderOptionsH original = nullptr;
  AEGP_LayerRenderOptionsH copy = nullptr;
  PF_Err err = PF_Err_NONE;

#define ACQUIRE(name, version, target)                                            \
  do {                                                                            \
    if (!err) err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(       \
        (name), (version), reinterpret_cast<const void**>(&(target))));            \
    if (!err && !(target)) err = PF_Err_INVALID_CALLBACK;                         \
  } while (false)
  ACQUIRE(kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1, pf);
  ACQUIRE(kAEGPEffectSuite, kAEGPEffectSuiteVersion3, effects);
  ACQUIRE(kAEGPLayerRenderOptionsSuite, kAEGPLayerRenderOptionsSuiteVersion1, suite);
#undef ACQUIRE

  if (!err && (!pf->AEGP_GetEffectLayer || !pf->AEGP_GetNewEffectForEffect ||
               !effects->AEGP_DisposeEffect || !suite->AEGP_NewFromLayer ||
               !suite->AEGP_NewFromUpstreamOfEffect || !suite->AEGP_Duplicate ||
               !suite->AEGP_Dispose || !suite->AEGP_SetTime || !suite->AEGP_GetTime ||
               !suite->AEGP_SetTimeStep || !suite->AEGP_GetTimeStep ||
               !suite->AEGP_SetWorldType || !suite->AEGP_GetWorldType ||
               !suite->AEGP_SetDownsampleFactor || !suite->AEGP_GetDownsampleFactor ||
               !suite->AEGP_SetMatteMode || !suite->AEGP_GetMatteMode)) {
    err = PF_Err_INVALID_CALLBACK;
  }

  if (!err) err = pf->AEGP_GetEffectLayer(in_data->effect_ref, &layer);
  if (!err && !layer) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = suite->AEGP_NewFromLayer(g_plugin_id, layer, &from_layer);
  if (!err && !from_layer) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = pf->AEGP_GetNewEffectForEffect(g_plugin_id, in_data->effect_ref, &effect);
  if (!err) err = suite->AEGP_NewFromUpstreamOfEffect(g_plugin_id, effect, &original);

  const A_Time wanted_time{37, 1001};
  const A_Time wanted_step{41, 30000};
  if (!err) err = suite->AEGP_SetTime(original, wanted_time);
  if (!err) err = suite->AEGP_SetTimeStep(original, wanted_step);
  if (!err) err = suite->AEGP_SetWorldType(original, AEGP_WorldType_16);
  if (!err) err = suite->AEGP_SetDownsampleFactor(original, 3, 5);
  if (!err) err = suite->AEGP_SetMatteMode(original, AEGP_MatteMode_PREMUL_BLACK);

  A_Time got_time{}, got_step{};
  AEGP_WorldType got_type = AEGP_WorldType_NONE;
  A_short got_x = 0, got_y = 0;
  AEGP_MatteMode got_matte = AEGP_MatteMode_STRAIGHT;
  if (!err) err = suite->AEGP_GetTime(original, &got_time);
  if (!err) err = suite->AEGP_GetTimeStep(original, &got_step);
  if (!err) err = suite->AEGP_GetWorldType(original, &got_type);
  if (!err) err = suite->AEGP_GetDownsampleFactor(original, &got_x, &got_y);
  if (!err) err = suite->AEGP_GetMatteMode(original, &got_matte);
  if (!err && (!same_time(got_time, wanted_time) || !same_time(got_step, wanted_step) ||
               got_type != AEGP_WorldType_16 || got_x != 3 || got_y != 5 ||
               got_matte != AEGP_MatteMode_PREMUL_BLACK)) err = PF_Err_BAD_CALLBACK_PARAM;

  if (!err) err = suite->AEGP_Duplicate(g_plugin_id, original, &copy);
  if (!err && (!copy || copy == original)) err = PF_Err_BAD_CALLBACK_PARAM;
  const A_Time copy_time{99, 1001};
  if (!err) err = suite->AEGP_SetTime(copy, copy_time);
  if (!err) err = suite->AEGP_SetWorldType(copy, AEGP_WorldType_32);
  if (!err) err = suite->AEGP_SetDownsampleFactor(copy, 7, 9);
  if (!err) err = suite->AEGP_SetMatteMode(copy, AEGP_MatteMode_PREMUL_BG_COLOR);
  if (!err) err = suite->AEGP_GetTime(original, &got_time);
  if (!err) err = suite->AEGP_GetWorldType(original, &got_type);
  if (!err) err = suite->AEGP_GetDownsampleFactor(original, &got_x, &got_y);
  if (!err) err = suite->AEGP_GetMatteMode(original, &got_matte);
  if (!err && (!same_time(got_time, wanted_time) || got_type != AEGP_WorldType_16 ||
               got_x != 3 || got_y != 5 || got_matte != AEGP_MatteMode_PREMUL_BLACK)) {
    err = PF_Err_BAD_CALLBACK_PARAM;
  }

  if (copy) { keep_first(err, suite->AEGP_Dispose(copy)); copy = nullptr; }
  if (original) {
    AEGP_LayerRenderOptionsH stale = original;
    keep_first(err, suite->AEGP_Dispose(original));
    original = nullptr;
    if (!err && suite->AEGP_GetTime(stale, &got_time) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && suite->AEGP_Dispose(stale) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
  }
  if (from_layer) { keep_first(err, suite->AEGP_Dispose(from_layer)); from_layer = nullptr; }
  if (effect) { keep_first(err, effects->AEGP_DisposeEffect(effect)); effect = nullptr; }
  if (suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(
                                kAEGPLayerRenderOptionsSuite,
                                kAEGPLayerRenderOptionsSuiteVersion1));
  if (effects) keep_first(err, in_data->pica_basicP->ReleaseSuite(
                                  kAEGPEffectSuite, kAEGPEffectSuiteVersion3));
  if (pf) keep_first(err, in_data->pica_basicP->ReleaseSuite(
                             kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1));
  if (!err) std::memset(output->data, 0,
                        static_cast<size_t>(output->rowbytes) * output->height);
  return err;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP: return global_setup(in_data, out_data);
    case PF_Cmd_PARAMS_SETUP: out_data->num_params = 1; return PF_Err_NONE;
    case PF_Cmd_RENDER: return render(in_data, output);
    default: return PF_Err_NONE;
  }
}
