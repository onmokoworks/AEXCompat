#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"
#include "AE_GeneralPlug.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <memory>
#include <mutex>

namespace {
AEGP_PluginID g_plugin_id = 0;

template <typename T>
void keep_first(PF_Err& first, T candidate) {
  if (!first && candidate) first = static_cast<PF_Err>(candidate);
}

struct AsyncResult {
  std::mutex mutex;
  std::condition_variable ready;
  std::atomic<A_long> callback_count{0};
  AEGP_AsyncFrameRequestRefcon expected_refcon = nullptr;
  bool refcon_matches = false;
  bool complete = false;
  AEGP_AsyncRequestId callback_request_id = 0;
  A_Boolean canceled = FALSE;
  A_Err error = A_Err_NONE;
  AEGP_FrameReceiptH receipt = nullptr;
};

A_Err async_ready(AEGP_AsyncRequestId request_id, A_Boolean canceled,
                  A_Err error, AEGP_FrameReceiptH receipt,
                  AEGP_AsyncFrameRequestRefcon refcon) {
  auto* result = reinterpret_cast<AsyncResult*>(refcon);
  if (!result) return PF_Err_BAD_CALLBACK_PARAM;
  result->callback_count.fetch_add(1, std::memory_order_relaxed);
  {
    std::lock_guard<std::mutex> lock(result->mutex);
    result->refcon_matches = refcon == result->expected_refcon;
    result->callback_request_id = request_id;
    result->canceled = canceled;
    result->error = error;
    result->receipt = receipt;
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
      nullptr, "PF AEGP Async Layer Receipt Probe", &g_plugin_id);
  if (utility) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3));
  return err;
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !in_data->effect_ref || !output ||
      !output->data || output->width < 0 || output->height < 0 || !g_plugin_id)
    return PF_Err_BAD_CALLBACK_PARAM;

  const AEGP_PFInterfaceSuite1* pf = nullptr;
  const AEGP_EffectSuite3* effects = nullptr;
  const AEGP_LayerRenderOptionsSuite1* options_suite = nullptr;
  const AEGP_RenderSuite5* render_suite = nullptr;
  const AEGP_WorldSuite3* world_suite = nullptr;
  const PF_WorldSuite2* pf_world_suite = nullptr;
  AEGP_EffectRefH effect = nullptr;
  AEGP_LayerRenderOptionsH options = nullptr;
  AEGP_FrameReceiptH receipt = nullptr;
  AEGP_WorldH world = nullptr;
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
  ACQUIRE(kAEGPWorldSuite, kAEGPWorldSuiteVersion3, world_suite);
  ACQUIRE(kPFWorldSuite, kPFWorldSuiteVersion2, pf_world_suite);
#undef ACQUIRE

  if (!err && (!pf->AEGP_GetNewEffectForEffect || !effects->AEGP_DisposeEffect ||
      !options_suite->AEGP_NewFromUpstreamOfEffect || !options_suite->AEGP_SetWorldType ||
      !options_suite->AEGP_Dispose || !render_suite->AEGP_RenderAndCheckoutLayerFrame_Async ||
      !render_suite->AEGP_CancelAsyncRequest || !render_suite->AEGP_CheckinFrame ||
      !render_suite->AEGP_GetReceiptWorld || !world_suite->AEGP_GetType ||
      !world_suite->AEGP_GetSize || !world_suite->AEGP_GetRowBytes ||
      !world_suite->AEGP_GetBaseAddr8 || !pf_world_suite->PF_GetPixelFormat))
    err = PF_Err_INVALID_CALLBACK;

  PF_PixelFormat format = PF_PixelFormat_INVALID;
  if (!err) err = pf_world_suite->PF_GetPixelFormat(output, &format);
  if (!err && format != PF_PixelFormat_ARGB32) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && output->rowbytes < output->width * static_cast<A_long>(sizeof(PF_Pixel8)))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = pf->AEGP_GetNewEffectForEffect(g_plugin_id, in_data->effect_ref, &effect);
  if (!err) err = options_suite->AEGP_NewFromUpstreamOfEffect(g_plugin_id, effect, &options);
  if (!err) err = options_suite->AEGP_SetWorldType(options, AEGP_WorldType_8);

  auto result = std::make_unique<AsyncResult>();
  AsyncResult* callback_state = result.get();
  callback_state->expected_refcon =
      reinterpret_cast<AEGP_AsyncFrameRequestRefcon>(callback_state);
  AEGP_AsyncRequestId request_id = 0;
  if (!err) err = render_suite->AEGP_RenderAndCheckoutLayerFrame_Async(
      options, async_ready, callback_state->expected_refcon,
      &request_id);  // Render Suite5 slot 2.
  bool completed = false;
  if (!err && request_id == 0) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) {
    std::unique_lock<std::mutex> lock(callback_state->mutex);
    completed = callback_state->ready.wait_for(
        lock, std::chrono::seconds(5), [&] { return callback_state->complete; });
  }
  if (!err && !completed) {
    keep_first(err, render_suite->AEGP_CancelAsyncRequest(request_id));
    // The host still owns the callback refcon until callback delivery.
    result.release();
    if (!err) err = PF_Err_INTERNAL_STRUCT_DAMAGED;
  }
  if (!err && (callback_state->callback_count.load(std::memory_order_relaxed) != 1 ||
      callback_state->callback_request_id != request_id || !callback_state->refcon_matches ||
      callback_state->canceled ||
      callback_state->error || !callback_state->receipt)) err = PF_Err_BAD_CALLBACK_PARAM;
  if (completed) receipt = callback_state->receipt;
  if (!err) err = render_suite->AEGP_GetReceiptWorld(receipt, &world);

  AEGP_WorldType type = AEGP_WorldType_NONE;
  A_long width = 0, height = 0;
  A_u_long rowbytes = 0;
  PF_Pixel8* pixels = nullptr;
  if (!err) err = world_suite->AEGP_GetType(world, &type);
  if (!err && type != AEGP_WorldType_8) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = world_suite->AEGP_GetSize(world, &width, &height);
  if (!err) err = world_suite->AEGP_GetRowBytes(world, &rowbytes);
  if (!err) err = world_suite->AEGP_GetBaseAddr8(world, &pixels);
  if (!err && (width < 0 || height < 0 || !pixels ||
      rowbytes < static_cast<A_u_long>(width) * sizeof(PF_Pixel8)))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) {
    std::memset(output->data, 0, static_cast<size_t>(output->rowbytes) * output->height);
    const A_long copy_width = std::min(width, output->width);
    const A_long copy_height = std::min(height, output->height);
    for (A_long y = 0; y < copy_height; ++y) {
      const auto* source = reinterpret_cast<const PF_Pixel8*>(
          reinterpret_cast<const A_u_char*>(pixels) + static_cast<size_t>(y) * rowbytes);
      auto* destination = reinterpret_cast<PF_Pixel8*>(
          reinterpret_cast<A_u_char*>(output->data) + static_cast<size_t>(y) * output->rowbytes);
      for (A_long x = 0; x < copy_width; ++x)
        destination[x] = {source[x].alpha,
                          static_cast<A_u_char>(source[x].green ^ (x & 0xff)),
                          static_cast<A_u_char>(source[x].blue ^ (y & 0xff)),
                          static_cast<A_u_char>(source[x].red ^ ((x + y) & 0xff))};
    }
  }

  if (receipt) keep_first(err, render_suite->AEGP_CheckinFrame(receipt));
  if (options) keep_first(err, options_suite->AEGP_Dispose(options));
  if (effect) keep_first(err, effects->AEGP_DisposeEffect(effect));
  if (pf_world_suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kPFWorldSuite, kPFWorldSuiteVersion2));
  if (world_suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPWorldSuite, kAEGPWorldSuiteVersion3));
  if (render_suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPRenderSuite, kAEGPRenderSuiteVersion5));
  if (options_suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPLayerRenderOptionsSuite, kAEGPLayerRenderOptionsSuiteVersion1));
  if (effects) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPEffectSuite, kAEGPEffectSuiteVersion3));
  if (pf) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1));
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
