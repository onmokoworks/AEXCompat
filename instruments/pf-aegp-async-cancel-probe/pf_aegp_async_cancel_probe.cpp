#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"
#include "AE_GeneralPlug.h"

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <mutex>

namespace {
AEGP_PluginID g_plugin_id = 0;

template <typename T>
void keep_first(PF_Err& first, T candidate) {
  if (!first && candidate) first = static_cast<PF_Err>(candidate);
}

struct CancelResult {
  std::mutex mutex;
  std::condition_variable ready;
  std::atomic<A_long> callback_count{0};
  AEGP_AsyncFrameRequestRefcon expected_refcon = nullptr;
  AEGP_AsyncRequestId callback_request_id = 0;
  A_Boolean canceled = FALSE;
  A_Err error = A_Err_NONE;
  AEGP_FrameReceiptH receipt = nullptr;
  bool refcon_matches = false;
  bool complete = false;
};

A_Err async_ready(AEGP_AsyncRequestId request_id, A_Boolean canceled,
                  A_Err error, AEGP_FrameReceiptH receipt,
                  AEGP_AsyncFrameRequestRefcon refcon) {
  auto* result = reinterpret_cast<CancelResult*>(refcon);
  if (!result) return PF_Err_BAD_CALLBACK_PARAM;
  result->callback_count.fetch_add(1, std::memory_order_relaxed);
  {
    std::lock_guard<std::mutex> lock(result->mutex);
    result->callback_request_id = request_id;
    result->canceled = canceled;
    result->error = error;
    result->receipt = receipt;
    result->refcon_matches = refcon == result->expected_refcon;
    result->complete = true;
  }
  result->ready.notify_one();
  return A_Err_NONE;
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
      nullptr, "PF AEGP Async Cancel Probe", &g_plugin_id);
  if (utility) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3));
  return err;
}

PF_Err render(PF_InData* in_data) {
  if (!in_data || !in_data->pica_basicP || !in_data->effect_ref || !g_plugin_id)
    return PF_Err_BAD_CALLBACK_PARAM;
  const AEGP_PFInterfaceSuite1* pf = nullptr;
  const AEGP_EffectSuite3* effects = nullptr;
  const AEGP_LayerRenderOptionsSuite1* options_suite = nullptr;
  const AEGP_RenderSuite5* render_suite = nullptr;
  AEGP_EffectRefH effect = nullptr;
  AEGP_LayerRenderOptionsH options = nullptr;
  PF_Err err = PF_Err_NONE;

#define ACQUIRE(name, version, target) do {                                      \
  if (!err) err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(       \
      (name), (version), reinterpret_cast<const void**>(&(target))));            \
  if (!err && !(target)) err = PF_Err_INVALID_CALLBACK;                         \
} while (false)
  ACQUIRE(kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1, pf);
  ACQUIRE(kAEGPEffectSuite, kAEGPEffectSuiteVersion3, effects);
  ACQUIRE(kAEGPLayerRenderOptionsSuite, kAEGPLayerRenderOptionsSuiteVersion1, options_suite);
  ACQUIRE(kAEGPRenderSuite, kAEGPRenderSuiteVersion5, render_suite);
#undef ACQUIRE

  if (!err && (!pf->AEGP_GetNewEffectForEffect || !effects->AEGP_DisposeEffect ||
      !options_suite->AEGP_NewFromUpstreamOfEffect || !options_suite->AEGP_SetWorldType ||
      !options_suite->AEGP_Dispose || !render_suite->AEGP_RenderAndCheckoutLayerFrame_Async ||
      !render_suite->AEGP_CancelAsyncRequest)) err = PF_Err_INVALID_CALLBACK;
  if (!err) err = pf->AEGP_GetNewEffectForEffect(g_plugin_id, in_data->effect_ref, &effect);
  if (!err) err = options_suite->AEGP_NewFromUpstreamOfEffect(g_plugin_id, effect, &options);
  if (!err) err = options_suite->AEGP_SetWorldType(options, AEGP_WorldType_8);

  CancelResult result;
  result.expected_refcon = reinterpret_cast<AEGP_AsyncFrameRequestRefcon>(&result);
  AEGP_AsyncRequestId request_id = 0;
  if (!err) err = render_suite->AEGP_RenderAndCheckoutLayerFrame_Async(
      options, async_ready, result.expected_refcon, &request_id);
  // Deliberately no work occurs between submission and cancellation.
  if (!err && request_id == 0) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = render_suite->AEGP_CancelAsyncRequest(request_id);
  bool delivered = false;
  if (!err) {
    std::unique_lock<std::mutex> lock(result.mutex);
    delivered = result.ready.wait_for(
        lock, std::chrono::seconds(5), [&] { return result.complete; });
  }
  if (!err && (!delivered || result.callback_count.load(std::memory_order_relaxed) != 1 ||
      result.callback_request_id != request_id || !result.refcon_matches ||
      !result.canceled || result.error != A_Err_NONE || result.receipt != nullptr))
    err = PF_Err_BAD_CALLBACK_PARAM;

  if (options) keep_first(err, options_suite->AEGP_Dispose(options));
  if (effect) keep_first(err, effects->AEGP_DisposeEffect(effect));
  if (render_suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPRenderSuite, kAEGPRenderSuiteVersion5));
  if (options_suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPLayerRenderOptionsSuite, kAEGPLayerRenderOptionsSuiteVersion1));
  if (effects) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPEffectSuite, kAEGPEffectSuiteVersion3));
  if (pf) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1));
  return err;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef*, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP: return global_setup(in_data, out_data);
    case PF_Cmd_PARAMS_SETUP: out_data->num_params = 1; return PF_Err_NONE;
    case PF_Cmd_RENDER: return render(in_data);
    default: return PF_Err_NONE;
  }
}
