#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

#include <cstddef>
#include <cstring>
#include <type_traits>

namespace {
AEGP_PluginID g_plugin_id = 0;

static_assert(std::is_standard_layout_v<AEGP_RenderSuite5>);
static_assert(sizeof(AEGP_RenderSuite5) == 14 * sizeof(void*));
static_assert(offsetof(AEGP_RenderSuite5, AEGP_CheckinFrame) == 4 * sizeof(void*));
static_assert(offsetof(AEGP_RenderSuite5, AEGP_GetRenderedRegion) == 6 * sizeof(void*));
static_assert(offsetof(AEGP_RenderSuite5, AEGP_GetReceiptGuid) == 13 * sizeof(void*));

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
      nullptr, "PF AEGP Render Suite5 Probe", &g_plugin_id);
  if (utility) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3));
  return err;
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !in_data->effect_ref || !output ||
      !output->data || output->height < 0 || output->rowbytes < 0 || !g_plugin_id)
    return PF_Err_BAD_CALLBACK_PARAM;

  const AEGP_PFInterfaceSuite1* pf = nullptr;
  const AEGP_LayerRenderOptionsSuite1* layer_options = nullptr;
  const AEGP_RenderSuite5* suite = nullptr;
  const AEGP_MemorySuite1* memory = nullptr;
  AEGP_LayerH layer = nullptr;
  AEGP_LayerRenderOptionsH layer_ro = nullptr;
  AEGP_FrameReceiptH receipt = nullptr;
  AEGP_MemHandle guid = nullptr;
  PF_Err err = PF_Err_NONE;

#define ACQUIRE(name, version, target) do {                                      \
  if (!err) err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(       \
      (name), (version), reinterpret_cast<const void**>(&(target))));            \
  if (!err && !(target)) err = PF_Err_INVALID_CALLBACK;                         \
} while (false)
  ACQUIRE(kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1, pf);
  ACQUIRE(kAEGPLayerRenderOptionsSuite, kAEGPLayerRenderOptionsSuiteVersion1, layer_options);
  ACQUIRE(kAEGPRenderSuite, kAEGPRenderSuiteVersion5, suite);
  ACQUIRE(kAEGPMemorySuite, kAEGPMemorySuiteVersion1, memory);
#undef ACQUIRE

  if (!err && (!pf->AEGP_GetEffectLayer ||
      !layer_options->AEGP_NewFromLayer || !layer_options->AEGP_Dispose ||
      !suite->AEGP_RenderAndCheckoutLayerFrame || !suite->AEGP_CheckinFrame ||
      !suite->AEGP_GetRenderedRegion || !suite->AEGP_IsRenderedFrameSufficient ||
      !suite->AEGP_GetCurrentTimestamp || !suite->AEGP_HasItemChangedSinceTimestamp ||
      !suite->AEGP_IsItemWorthwhileToRender || !suite->AEGP_GetReceiptGuid ||
      !memory->AEGP_LockMemHandle || !memory->AEGP_UnlockMemHandle ||
      !memory->AEGP_FreeMemHandle)) err = PF_Err_INVALID_CALLBACK;

  if (!err) err = pf->AEGP_GetEffectLayer(in_data->effect_ref, &layer);
  if (!err && !layer) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = layer_options->AEGP_NewFromLayer(g_plugin_id, layer, &layer_ro);

  AEGP_TimeStamp timestamp{};
  A_Boolean answer = FALSE;
  const A_Time start{0, 1}, duration{1, 1};
  if (!err) err = suite->AEGP_GetCurrentTimestamp(&timestamp);
  if (!err && suite->AEGP_IsRenderedFrameSufficient(nullptr, nullptr, &answer) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && suite->AEGP_IsItemWorthwhileToRender(nullptr, &timestamp, &answer) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;

  if (!err) err = suite->AEGP_RenderAndCheckoutLayerFrame(layer_ro, nullptr, nullptr, &receipt);
  A_LRect region{};
  if (!err) err = suite->AEGP_GetRenderedRegion(receipt, &region);
  if (!err && (region.right <= region.left || region.bottom <= region.top))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = suite->AEGP_GetReceiptGuid(receipt, &guid);
  void* guid_bytes = nullptr;
  if (!err) err = memory->AEGP_LockMemHandle(guid, &guid_bytes);
  if (!err && !guid_bytes) err = PF_Err_BAD_CALLBACK_PARAM;
  if (guid_bytes) keep_first(err, memory->AEGP_UnlockMemHandle(guid));
  if (guid) {
    keep_first(err, memory->AEGP_FreeMemHandle(guid));
    AEGP_MemHandle stale_guid = guid;
    guid = nullptr;
    if (!err && memory->AEGP_FreeMemHandle(stale_guid) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
  }
  if (receipt) {
    AEGP_FrameReceiptH stale_receipt = receipt;
    keep_first(err, suite->AEGP_CheckinFrame(receipt));
    receipt = nullptr;
    A_LRect stale_region{};
    AEGP_MemHandle stale_guid = nullptr;
    if (!err && suite->AEGP_GetRenderedRegion(stale_receipt, &stale_region) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && suite->AEGP_GetReceiptGuid(stale_receipt, &stale_guid) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && suite->AEGP_CheckinFrame(stale_receipt) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
  }
  if (!err && suite->AEGP_GetCurrentTimestamp(nullptr) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && suite->AEGP_HasItemChangedSinceTimestamp(
      nullptr, &start, &duration, &timestamp, &answer) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (layer_ro) keep_first(err, layer_options->AEGP_Dispose(layer_ro));
  if (memory) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPMemorySuite, kAEGPMemorySuiteVersion1));
  if (suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPRenderSuite, kAEGPRenderSuiteVersion5));
  if (layer_options) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPLayerRenderOptionsSuite, kAEGPLayerRenderOptionsSuiteVersion1));
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
