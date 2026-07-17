#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

#include <cstddef>
#include <cstring>
#include <type_traits>

namespace {
AEGP_PluginID g_plugin_id = 0;

static_assert(std::is_standard_layout_v<AEGP_WorldSuite3>);
static_assert(sizeof(AEGP_WorldSuite3) == 13 * sizeof(void*));
static_assert(offsetof(AEGP_WorldSuite3, AEGP_Dispose) == 1 * sizeof(void*));
static_assert(offsetof(AEGP_WorldSuite3, AEGP_NewPlatformWorld) == 10 * sizeof(void*));
static_assert(offsetof(AEGP_WorldSuite3, AEGP_DisposePlatformWorld) == 11 * sizeof(void*));
static_assert(offsetof(AEGP_WorldSuite3, AEGP_NewReferenceFromPlatformWorld) == 12 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_WorldSuite3::AEGP_GetBaseAddr8),
                             A_Err (*)(AEGP_WorldH, PF_Pixel8**)>);
static_assert(std::is_standard_layout_v<AEGP_RenderSuite5>);
static_assert(sizeof(AEGP_RenderSuite5) == 14 * sizeof(void*));
static_assert(offsetof(AEGP_RenderSuite5, AEGP_CheckinRenderedFrame) == 12 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_RenderSuite5::AEGP_CheckinRenderedFrame),
                             A_Err (*)(AEGP_RenderOptionsH, const AEGP_TimeStamp*,
                                       A_u_long, AEGP_PlatformWorldH)>);

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
      nullptr, "PF AEGP Platform World Probe", &g_plugin_id);
  if (utility) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3));
  return err;
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !in_data->effect_ref || !output ||
      !output->data || output->width <= 0 || output->height <= 0 ||
      output->rowbytes < output->width * static_cast<A_long>(sizeof(PF_Pixel8)) ||
      !g_plugin_id) return PF_Err_BAD_CALLBACK_PARAM;

  const AEGP_PFInterfaceSuite1* pf = nullptr;
  const AEGP_LayerSuite9* layers = nullptr;
  const AEGP_RenderOptionsSuite4* options_suite = nullptr;
  const AEGP_WorldSuite3* worlds = nullptr;
  const AEGP_RenderSuite5* render_suite = nullptr;
  AEGP_RenderOptionsH options = nullptr;
  AEGP_PlatformWorldH platform = nullptr;
  AEGP_WorldH reference = nullptr;
  PF_Err err = PF_Err_NONE;

#define ACQUIRE(name, version, target) do {                                      \
  if (!err) err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(       \
      (name), (version), reinterpret_cast<const void**>(&(target))));            \
  if (!err && !(target)) err = PF_Err_INVALID_CALLBACK;                         \
} while (false)
  ACQUIRE(kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1, pf);
  ACQUIRE(kAEGPLayerSuite, kAEGPLayerSuiteVersion9, layers);
  ACQUIRE(kAEGPRenderOptionsSuite, kAEGPRenderOptionsSuiteVersion4, options_suite);
  ACQUIRE(kAEGPWorldSuite, kAEGPWorldSuiteVersion3, worlds);
  ACQUIRE(kAEGPRenderSuite, kAEGPRenderSuiteVersion5, render_suite);
#undef ACQUIRE

  if (!err && (!pf->AEGP_GetEffectLayer || !layers->AEGP_GetLayerSourceItem ||
      !options_suite->AEGP_NewFromItem || !options_suite->AEGP_Dispose ||
      !worlds->AEGP_NewPlatformWorld || !worlds->AEGP_DisposePlatformWorld ||
      !worlds->AEGP_NewReferenceFromPlatformWorld || !worlds->AEGP_Dispose ||
      !worlds->AEGP_GetType || !worlds->AEGP_GetSize ||
      !worlds->AEGP_GetRowBytes || !worlds->AEGP_GetBaseAddr8 ||
      !render_suite->AEGP_GetCurrentTimestamp ||
      !render_suite->AEGP_CheckinRenderedFrame)) err = PF_Err_INVALID_CALLBACK;

  AEGP_LayerH layer = nullptr;
  AEGP_ItemH item = nullptr;
  if (!err) err = pf->AEGP_GetEffectLayer(in_data->effect_ref, &layer);
  if (!err) err = layers->AEGP_GetLayerSourceItem(layer, &item);
  if (!err) err = options_suite->AEGP_NewFromItem(g_plugin_id, item, &options);

  // Invalid creation and reference paths must fail without yielding handles.
  AEGP_PlatformWorldH invalid_platform = nullptr;
  AEGP_WorldH invalid_reference = nullptr;
  if (!err && worlds->AEGP_NewPlatformWorld(
      g_plugin_id, AEGP_WorldType_8, 0, output->height, &invalid_platform) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && worlds->AEGP_NewReferenceFromPlatformWorld(
      g_plugin_id, nullptr, &invalid_reference) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;

  if (!err) err = worlds->AEGP_NewPlatformWorld(
      g_plugin_id, AEGP_WorldType_8, output->width, output->height, &platform);
  if (!err) err = worlds->AEGP_NewReferenceFromPlatformWorld(
      g_plugin_id, platform, &reference);

  AEGP_WorldType type = AEGP_WorldType_NONE;
  A_long width = 0, height = 0;
  A_u_long rowbytes = 0;
  PF_Pixel8* pixels = nullptr;
  if (!err) err = worlds->AEGP_GetType(reference, &type);
  if (!err) err = worlds->AEGP_GetSize(reference, &width, &height);
  if (!err) err = worlds->AEGP_GetRowBytes(reference, &rowbytes);
  if (!err) err = worlds->AEGP_GetBaseAddr8(reference, &pixels);
  if (!err && (type != AEGP_WorldType_8 || width != output->width ||
      height != output->height || !pixels ||
      rowbytes < static_cast<A_u_long>(width) * sizeof(PF_Pixel8)))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && worlds->AEGP_GetBaseAddr16(reference, nullptr) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) {
    for (A_long y = 0; y < height; ++y) {
      auto* row = reinterpret_cast<PF_Pixel8*>(
          reinterpret_cast<A_u_char*>(pixels) + static_cast<size_t>(y) * rowbytes);
      for (A_long x = 0; x < width; ++x)
        row[x] = {255, static_cast<A_u_char>(x), static_cast<A_u_char>(y), 73};
    }
  }

  AEGP_WorldH stale_reference = reference;
  if (reference) {
    keep_first(err, worlds->AEGP_Dispose(reference));
    reference = nullptr;
  }
  if (!err && worlds->AEGP_GetType(stale_reference, &type) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = worlds->AEGP_NewReferenceFromPlatformWorld(
      g_plugin_id, platform, &reference);

  AEGP_TimeStamp timestamp{};
  if (!err) err = render_suite->AEGP_GetCurrentTimestamp(&timestamp);
  AEGP_PlatformWorldH stale_platform = platform;
  AEGP_WorldH adopted_reference = reference;
  if (!err) {
    err = render_suite->AEGP_CheckinRenderedFrame(options, &timestamp, 1, platform);
    if (!err) platform = nullptr;
  }
  if (!err && worlds->AEGP_DisposePlatformWorld(stale_platform) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && (worlds->AEGP_GetSize(adopted_reference, &width, &height) != A_Err_NONE ||
      width != output->width || height != output->height))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err && render_suite->AEGP_CheckinRenderedFrame(
      nullptr, &timestamp, 1, nullptr) == A_Err_NONE) err = PF_Err_BAD_CALLBACK_PARAM;

  if (reference) keep_first(err, worlds->AEGP_Dispose(reference));
  if (platform) keep_first(err, worlds->AEGP_DisposePlatformWorld(platform));
  if (options) keep_first(err, options_suite->AEGP_Dispose(options));
  if (render_suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPRenderSuite, kAEGPRenderSuiteVersion5));
  if (worlds) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPWorldSuite, kAEGPWorldSuiteVersion3));
  if (options_suite) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPRenderOptionsSuite, kAEGPRenderOptionsSuiteVersion4));
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
