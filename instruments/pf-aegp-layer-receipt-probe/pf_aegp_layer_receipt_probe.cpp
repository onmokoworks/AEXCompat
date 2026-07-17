#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"
#include "AE_GeneralPlug.h"

#include <algorithm>
#include <cstring>

namespace {
AEGP_PluginID g_plugin_id = 0;


template <typename T>
void keep_first(PF_Err& first, T candidate) {
  if (!first && candidate) first = static_cast<PF_Err>(candidate);
}

PF_Err global_setup(PF_InData* in_data, PF_OutData* out_data) {
  if (!in_data || !in_data->pica_basicP || !out_data) return PF_Err_BAD_CALLBACK_PARAM;
  out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
  out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
  out_data->out_flags2 = PF_OutFlag2_FLOAT_COLOR_AWARE;

  const AEGP_UtilitySuite3* utility = nullptr;
  PF_Err err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3,
      reinterpret_cast<const void**>(&utility)));
  if (!err && (!utility || !utility->AEGP_RegisterWithAEGP)) err = PF_Err_INVALID_CALLBACK;
  if (!err) err = utility->AEGP_RegisterWithAEGP(nullptr,
                                                  "PF AEGP Layer Receipt Probe",
                                                  &g_plugin_id);
  if (utility) keep_first(err, in_data->pica_basicP->ReleaseSuite(
                                  kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3));
  return err;
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !in_data->effect_ref || !output ||
      !output->data || output->width < 0 || output->height < 0 ||
      !g_plugin_id) {
    return PF_Err_BAD_CALLBACK_PARAM;
  }

  const AEGP_PFInterfaceSuite1* pf = nullptr;
  const AEGP_EffectSuite3* effects = nullptr;
  const AEGP_LayerRenderOptionsSuite1* options_suite = nullptr;
  const AEGP_RenderSuite5* render_suite = nullptr;
  const AEGP_WorldSuite3* world_suite = nullptr;
  const PF_WorldSuite2* pf_world_suite = nullptr;
  AEGP_EffectRefH effect = nullptr;
  AEGP_LayerRenderOptionsH options = nullptr;
  AEGP_FrameReceiptH receipt = nullptr;
  AEGP_WorldH borrowed_world = nullptr;
  PF_Err err = PF_Err_NONE;

#define ACQUIRE(name, version, target)                                            \
  do {                                                                            \
    if (!err) err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(       \
        (name), (version), reinterpret_cast<const void**>(&(target))));            \
    if (!err && !(target)) err = PF_Err_INVALID_CALLBACK;                         \
  } while (false)
  ACQUIRE(kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1, pf);
  ACQUIRE(kAEGPEffectSuite, kAEGPEffectSuiteVersion3, effects);
  ACQUIRE(kAEGPLayerRenderOptionsSuite, kAEGPLayerRenderOptionsSuiteVersion1,
          options_suite);
  ACQUIRE(kAEGPRenderSuite, kAEGPRenderSuiteVersion5, render_suite);
  ACQUIRE(kAEGPWorldSuite, kAEGPWorldSuiteVersion3, world_suite);
  ACQUIRE(kPFWorldSuite, kPFWorldSuiteVersion2, pf_world_suite);
#undef ACQUIRE

  if (!err && (!pf->AEGP_GetNewEffectForEffect || !effects->AEGP_DisposeEffect ||
               !options_suite->AEGP_NewFromUpstreamOfEffect ||
               !options_suite->AEGP_Dispose || !options_suite->AEGP_SetWorldType ||
               !render_suite->AEGP_RenderAndCheckoutLayerFrame ||
               !render_suite->AEGP_GetReceiptWorld || !render_suite->AEGP_CheckinFrame ||
               !world_suite->AEGP_GetType || !world_suite->AEGP_GetSize ||
               !world_suite->AEGP_GetRowBytes || !world_suite->AEGP_GetBaseAddr8 ||
               !world_suite->AEGP_GetBaseAddr16 || !world_suite->AEGP_GetBaseAddr32 ||
               !pf_world_suite->PF_GetPixelFormat)) {
    err = PF_Err_INVALID_CALLBACK;
  }
  PF_PixelFormat output_format = PF_PixelFormat_INVALID;
  AEGP_WorldType requested_type = AEGP_WorldType_NONE;
  size_t pixel_size = 0;
  if (!err) err = pf_world_suite->PF_GetPixelFormat(output, &output_format);
  if (!err) {
    switch (output_format) {
      case PF_PixelFormat_ARGB32:
        requested_type = AEGP_WorldType_8;
        pixel_size = sizeof(PF_Pixel8);
        break;
      case PF_PixelFormat_ARGB64:
        requested_type = AEGP_WorldType_16;
        pixel_size = sizeof(PF_Pixel16);
        break;
      case PF_PixelFormat_ARGB128:
        requested_type = AEGP_WorldType_32;
        pixel_size = sizeof(PF_PixelFloat);
        break;
      default:
        err = PF_Err_BAD_CALLBACK_PARAM;
        break;
    }
  }
  if (!err && output->rowbytes < output->width * static_cast<A_long>(pixel_size))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = pf->AEGP_GetNewEffectForEffect(g_plugin_id, in_data->effect_ref, &effect);
  if (!err) err = options_suite->AEGP_NewFromUpstreamOfEffect(g_plugin_id, effect, &options);
  if (!err) err = options_suite->AEGP_SetWorldType(options, requested_type);
  if (!err) err = render_suite->AEGP_RenderAndCheckoutLayerFrame(
      options, nullptr, nullptr, &receipt);
  if (!err) err = render_suite->AEGP_GetReceiptWorld(receipt, &borrowed_world);

  AEGP_WorldType type = AEGP_WorldType_NONE;
  A_long width = 0, height = 0;
  A_u_long rowbytes = 0;
  void* pixels = nullptr;
  if (!err) err = world_suite->AEGP_GetType(borrowed_world, &type);
  if (!err && type != requested_type) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = world_suite->AEGP_GetSize(borrowed_world, &width, &height);
  if (!err) err = world_suite->AEGP_GetRowBytes(borrowed_world, &rowbytes);
  if (!err && type == AEGP_WorldType_8)
    err = world_suite->AEGP_GetBaseAddr8(borrowed_world,
                                         reinterpret_cast<PF_Pixel8**>(&pixels));
  if (!err && type == AEGP_WorldType_16)
    err = world_suite->AEGP_GetBaseAddr16(borrowed_world,
                                          reinterpret_cast<PF_Pixel16**>(&pixels));
  if (!err && type == AEGP_WorldType_32)
    err = world_suite->AEGP_GetBaseAddr32(borrowed_world,
                                          reinterpret_cast<PF_PixelFloat**>(&pixels));
  if (!err && (width < 0 || height < 0 || !pixels ||
               rowbytes < static_cast<A_u_long>(width) * pixel_size))
    err = PF_Err_BAD_CALLBACK_PARAM;

  if (!err) {
    std::memset(output->data, 0, static_cast<size_t>(output->rowbytes) * output->height);
    const A_long copy_width = std::min(width, output->width);
    const A_long copy_height = std::min(height, output->height);
    for (A_long y = 0; y < copy_height; ++y) {
      const auto* source_bytes =
          reinterpret_cast<const A_u_char*>(pixels) + static_cast<size_t>(y) * rowbytes;
      auto* destination_bytes =
          reinterpret_cast<A_u_char*>(output->data) + static_cast<size_t>(y) * output->rowbytes;
      for (A_long x = 0; x < copy_width; ++x) {
        if (type == AEGP_WorldType_8) {
          const auto* source = reinterpret_cast<const PF_Pixel8*>(source_bytes);
          auto* destination = reinterpret_cast<PF_Pixel8*>(destination_bytes);
          destination[x] = {source[x].alpha,
                            static_cast<A_u_char>(source[x].green ^ (x & 0xff)),
                            static_cast<A_u_char>(source[x].blue ^ (y & 0xff)),
                            static_cast<A_u_char>(source[x].red ^ ((x + y) & 0xff))};
        } else if (type == AEGP_WorldType_16) {
          const auto* source = reinterpret_cast<const PF_Pixel16*>(source_bytes);
          auto* destination = reinterpret_cast<PF_Pixel16*>(destination_bytes);
          destination[x] = {source[x].alpha,
                            static_cast<A_u_short>(source[x].green ^ (x & 0xffff)),
                            static_cast<A_u_short>(source[x].blue ^ (y & 0xffff)),
                            static_cast<A_u_short>(source[x].red ^ ((x + y) & 0xffff))};
        } else {
          const auto* source = reinterpret_cast<const PF_PixelFloat*>(source_bytes);
          auto* destination = reinterpret_cast<PF_PixelFloat*>(destination_bytes);
          destination[x] = {source[x].alpha,
                            source[x].green + static_cast<float>(x) / 65536.0F,
                            source[x].blue + static_cast<float>(y) / 65536.0F,
                            source[x].red + static_cast<float>(x + y) / 65536.0F};
        }
      }
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
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER: return render(in_data, output);
    default: return PF_Err_NONE;
  }
}
