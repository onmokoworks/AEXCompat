#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

#include <cstddef>
#include <cstring>
#include <type_traits>

namespace {
AEGP_PluginID g_plugin_id = 0;

static_assert(std::is_standard_layout_v<AEGP_RenderOptionsSuite4>);
static_assert(sizeof(AEGP_RenderOptionsSuite4) == 23 * sizeof(void*));
static_assert(offsetof(AEGP_RenderOptionsSuite4, AEGP_SetChannelOrder) == 17 * sizeof(void*));
static_assert(offsetof(AEGP_RenderOptionsSuite4, AEGP_GetChannelOrder) == 18 * sizeof(void*));
static_assert(offsetof(AEGP_RenderOptionsSuite4, AEGP_GetRenderGuideLayers) == 19 * sizeof(void*));
static_assert(offsetof(AEGP_RenderOptionsSuite4, AEGP_SetRenderGuideLayers) == 20 * sizeof(void*));
static_assert(offsetof(AEGP_RenderOptionsSuite4, AEGP_GetRenderQuality) == 21 * sizeof(void*));
static_assert(offsetof(AEGP_RenderOptionsSuite4, AEGP_SetRenderQuality) == 22 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_RenderOptionsSuite4::AEGP_SetChannelOrder),
                            A_Err (*)(AEGP_RenderOptionsH, AEGP_ChannelOrder)>);
static_assert(std::is_same_v<decltype(AEGP_RenderOptionsSuite4::AEGP_GetRenderGuideLayers),
                            A_Err (*)(AEGP_RenderOptionsH, A_Boolean*)>);
static_assert(std::is_same_v<decltype(AEGP_RenderOptionsSuite4::AEGP_SetRenderQuality),
                            A_Err (*)(AEGP_RenderOptionsH, AEGP_ItemQuality)>);

template <typename T>
void keep_first(PF_Err& first, T candidate) {
  if (!first && candidate) first = static_cast<PF_Err>(candidate);
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
      nullptr, "PF AEGP Render Options4 Tail Probe", &g_plugin_id);
  if (utility) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3));
  return err;
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !in_data->effect_ref || !output ||
      !output->data || output->height < 0 || output->rowbytes < 0 || !g_plugin_id)
    return PF_Err_BAD_CALLBACK_PARAM;

  const AEGP_PFInterfaceSuite1* pf = nullptr;
  const AEGP_LayerSuite9* layers = nullptr;
  const AEGP_RenderOptionsSuite4* suite = nullptr;
  AEGP_RenderOptionsH options = nullptr;
  PF_Err err = PF_Err_NONE;

#define ACQUIRE(name, version, target) do {                                      \
  if (!err) err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(       \
      (name), (version), reinterpret_cast<const void**>(&(target))));            \
  if (!err && !(target)) err = PF_Err_INVALID_CALLBACK;                         \
} while (false)
  ACQUIRE(kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1, pf);
  ACQUIRE(kAEGPLayerSuite, kAEGPLayerSuiteVersion9, layers);
  ACQUIRE(kAEGPRenderOptionsSuite, kAEGPRenderOptionsSuiteVersion4, suite);
#undef ACQUIRE

  if (!err && (!pf->AEGP_GetEffectLayer || !layers->AEGP_GetLayerSourceItem ||
      !suite->AEGP_NewFromItem || !suite->AEGP_Dispose ||
      !suite->AEGP_SetChannelOrder || !suite->AEGP_GetChannelOrder ||
      !suite->AEGP_GetRenderGuideLayers || !suite->AEGP_SetRenderGuideLayers ||
      !suite->AEGP_GetRenderQuality || !suite->AEGP_SetRenderQuality))
    err = PF_Err_INVALID_CALLBACK;

  AEGP_LayerH layer = nullptr;
  AEGP_ItemH item = nullptr;
  if (!err) err = pf->AEGP_GetEffectLayer(in_data->effect_ref, &layer);
  if (!err) err = layers->AEGP_GetLayerSourceItem(layer, &item);
  if (!err && !item) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = suite->AEGP_NewFromItem(g_plugin_id, item, &options);
  if (!err && !options) err = PF_Err_BAD_CALLBACK_PARAM;

  AEGP_ChannelOrder channel = AEGP_ChannelOrder_ARGB;
  A_Boolean guides = FALSE;
  AEGP_ItemQuality quality = AEGP_ItemQuality_DRAFT;
  if (!err) err = suite->AEGP_SetChannelOrder(options, AEGP_ChannelOrder_BGRA);
  if (!err) err = suite->AEGP_GetChannelOrder(options, &channel);
  if (!err && channel != AEGP_ChannelOrder_BGRA) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = suite->AEGP_SetRenderGuideLayers(options, TRUE);
  if (!err) err = suite->AEGP_GetRenderGuideLayers(options, &guides);
  if (!err && guides != TRUE) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = suite->AEGP_SetRenderQuality(options, AEGP_ItemQuality_BEST);
  if (!err) err = suite->AEGP_GetRenderQuality(options, &quality);
  if (!err && quality != AEGP_ItemQuality_BEST) err = PF_Err_BAD_CALLBACK_PARAM;

  // Invalid handles and output pointers must not report success.
  if (!err && suite->AEGP_SetChannelOrder(nullptr, AEGP_ChannelOrder_ARGB) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && suite->AEGP_GetChannelOrder(options, nullptr) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && suite->AEGP_GetRenderGuideLayers(options, nullptr) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && suite->AEGP_SetRenderGuideLayers(nullptr, FALSE) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && suite->AEGP_GetRenderQuality(options, nullptr) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && suite->AEGP_SetRenderQuality(nullptr, AEGP_ItemQuality_DRAFT) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;

  if (options) {
    AEGP_RenderOptionsH stale = options;
    keep_first(err, suite->AEGP_Dispose(options));
    options = nullptr;
    if (!err && suite->AEGP_SetChannelOrder(stale, AEGP_ChannelOrder_ARGB) == A_Err_NONE) err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && suite->AEGP_GetChannelOrder(stale, &channel) == A_Err_NONE) err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && suite->AEGP_GetRenderGuideLayers(stale, &guides) == A_Err_NONE) err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && suite->AEGP_SetRenderGuideLayers(stale, FALSE) == A_Err_NONE) err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && suite->AEGP_GetRenderQuality(stale, &quality) == A_Err_NONE) err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && suite->AEGP_SetRenderQuality(stale, AEGP_ItemQuality_DRAFT) == A_Err_NONE) err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && suite->AEGP_Dispose(stale) == A_Err_NONE) err = PF_Err_BAD_CALLBACK_PARAM;
  }

  if (suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPRenderOptionsSuite, kAEGPRenderOptionsSuiteVersion4));
  if (layers) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPLayerSuite, kAEGPLayerSuiteVersion9));
  if (pf) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1));
  if (!err) std::memset(output->data, 0, static_cast<size_t>(output->rowbytes) * output->height);
  return err;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
    PF_OutData* out_data, PF_ParamDef*[], PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP: return global_setup(in_data, out_data);
    case PF_Cmd_PARAMS_SETUP: out_data->num_params = 1; return PF_Err_NONE;
    case PF_Cmd_RENDER: return render(in_data, output);
    default: return PF_Err_NONE;
  }
}
