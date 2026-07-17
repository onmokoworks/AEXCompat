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
static_assert(offsetof(AEGP_RenderSuite5, AEGP_RenderAndCheckoutFrame) == 0 * sizeof(void*));
static_assert(offsetof(AEGP_RenderSuite5, AEGP_CheckinFrame) == 4 * sizeof(void*));
static_assert(offsetof(AEGP_RenderSuite5, AEGP_GetReceiptWorld) == 5 * sizeof(void*));
static_assert(offsetof(AEGP_RenderSuite5, AEGP_CheckinRenderedFrame) == 12 * sizeof(void*));

template <typename T>
void keep_first(PF_Err& first, T candidate) {
  if (!first && candidate) first = static_cast<PF_Err>(candidate);
}

PF_Pixel8 expected_pixel(A_long x, A_long y) {
  return {255, static_cast<A_u_char>((x * 17 + y * 3) & 0xff),
          static_cast<A_u_char>((x * 5 + y * 29) & 0xff),
          static_cast<A_u_char>((x * 11 + y * 7 + 41) & 0xff)};
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
      nullptr, "PF AEGP External Cache Roundtrip Probe", &g_plugin_id);
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
  const AEGP_RenderSuite5* renders = nullptr;
  AEGP_RenderOptionsH options = nullptr;
  AEGP_PlatformWorldH platform = nullptr;
  AEGP_WorldH writable_world = nullptr;
  AEGP_FrameReceiptH receipt = nullptr;
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
  ACQUIRE(kAEGPRenderSuite, kAEGPRenderSuiteVersion5, renders);
#undef ACQUIRE

  if (!err && (!pf->AEGP_GetEffectLayer || !layers->AEGP_GetLayerSourceItem ||
      !options_suite->AEGP_NewFromItem || !options_suite->AEGP_Dispose ||
      !options_suite->AEGP_SetWorldType || !options_suite->AEGP_SetDownsampleFactor ||
      !options_suite->AEGP_SetRegionOfInterest || !worlds->AEGP_NewPlatformWorld ||
      !worlds->AEGP_NewReferenceFromPlatformWorld || !worlds->AEGP_Dispose ||
      !worlds->AEGP_DisposePlatformWorld || !worlds->AEGP_GetType ||
      !worlds->AEGP_GetSize || !worlds->AEGP_GetRowBytes || !worlds->AEGP_GetBaseAddr8 ||
      !renders->AEGP_GetCurrentTimestamp || !renders->AEGP_CheckinRenderedFrame ||
      !renders->AEGP_RenderAndCheckoutFrame || !renders->AEGP_GetReceiptWorld ||
      !renders->AEGP_GetRenderedRegion || !renders->AEGP_CheckinFrame))
    err = PF_Err_INVALID_CALLBACK;

  AEGP_LayerH layer = nullptr;
  AEGP_ItemH item = nullptr;
  if (!err) err = pf->AEGP_GetEffectLayer(in_data->effect_ref, &layer);
  if (!err) err = layers->AEGP_GetLayerSourceItem(layer, &item);
  if (!err) err = options_suite->AEGP_NewFromItem(g_plugin_id, item, &options);
  if (!err) err = options_suite->AEGP_SetWorldType(options, AEGP_WorldType_8);
  if (!err) err = worlds->AEGP_NewPlatformWorld(
      g_plugin_id, AEGP_WorldType_8, output->width, output->height, &platform);
  if (!err) err = worlds->AEGP_NewReferenceFromPlatformWorld(
      g_plugin_id, platform, &writable_world);

  A_long width = 0, height = 0;
  A_u_long rowbytes = 0;
  PF_Pixel8* pixels = nullptr;
  if (!err) err = worlds->AEGP_GetSize(writable_world, &width, &height);
  if (!err) err = worlds->AEGP_GetRowBytes(writable_world, &rowbytes);
  if (!err) err = worlds->AEGP_GetBaseAddr8(writable_world, &pixels);
  if (!err && (width != output->width || height != output->height || !pixels ||
      rowbytes < static_cast<A_u_long>(width) * sizeof(PF_Pixel8)))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) for (A_long y = 0; y < height; ++y) {
    auto* row = reinterpret_cast<PF_Pixel8*>(reinterpret_cast<A_u_char*>(pixels) +
                                             static_cast<size_t>(y) * rowbytes);
    for (A_long x = 0; x < width; ++x) row[x] = expected_pixel(x, y);
  }

  AEGP_TimeStamp timestamp{};
  if (!err) err = renders->AEGP_GetCurrentTimestamp(&timestamp);
  AEGP_PlatformWorldH stale_platform = platform;
  if (!err) {
    err = renders->AEGP_CheckinRenderedFrame(options, &timestamp, 1, platform);
    if (!err) platform = nullptr;
  }
  if (!err && worlds->AEGP_DisposePlatformWorld(stale_platform) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = renders->AEGP_RenderAndCheckoutFrame(options, nullptr, nullptr, &receipt);

  AEGP_WorldH receipt_world = nullptr;
  AEGP_WorldType type = AEGP_WorldType_NONE;
  A_LRect rendered_region{};
  if (!err) err = renders->AEGP_GetReceiptWorld(receipt, &receipt_world);
  if (!err) err = renders->AEGP_GetRenderedRegion(receipt, &rendered_region);
  if (!err) err = worlds->AEGP_GetType(receipt_world, &type);
  if (!err) err = worlds->AEGP_GetSize(receipt_world, &width, &height);
  if (!err) err = worlds->AEGP_GetRowBytes(receipt_world, &rowbytes);
  if (!err) err = worlds->AEGP_GetBaseAddr8(receipt_world, &pixels);
  if (!err && (type != AEGP_WorldType_8 || width != output->width ||
      height != output->height || !pixels || rendered_region.left != 0 ||
      rendered_region.top != 0 || rendered_region.right != width ||
      rendered_region.bottom != height)) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) for (A_long y = 0; y < height && !err; ++y) {
    const auto* row = reinterpret_cast<const PF_Pixel8*>(
        reinterpret_cast<const A_u_char*>(pixels) + static_cast<size_t>(y) * rowbytes);
    for (A_long x = 0; x < width; ++x) {
      const PF_Pixel8 expected = expected_pixel(x, y);
      if (std::memcmp(&row[x], &expected, sizeof(expected)) != 0) {
        err = PF_Err_BAD_CALLBACK_PARAM;
        break;
      }
    }
  }

  if (receipt) {
    AEGP_FrameReceiptH stale_receipt = receipt;
    keep_first(err, renders->AEGP_CheckinFrame(receipt));
    receipt = nullptr;
    AEGP_WorldH stale_world = nullptr;
    AEGP_WorldType stale_type = AEGP_WorldType_NONE;
    if (!err && renders->AEGP_GetReceiptWorld(stale_receipt, &stale_world) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && renders->AEGP_GetRenderedRegion(stale_receipt, &rendered_region) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && renders->AEGP_CheckinFrame(stale_receipt) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err && worlds->AEGP_GetType(receipt_world, &stale_type) == A_Err_NONE)
      err = PF_Err_BAD_CALLBACK_PARAM;
  }

  // Exercise a second cache key whose source ROI projects into a downsampled world.
  AEGP_PlatformWorldH projected_platform = nullptr;
  AEGP_WorldH projected_reference = nullptr;
  AEGP_WorldH projected_receipt_world = nullptr;
  AEGP_FrameReceiptH projected_receipt = nullptr;
  if (!err && output->width >= 31 && output->height >= 20) {
    constexpr A_short downsample_x = 2;
    constexpr A_short downsample_y = 3;
    const A_LRect source_roi{3, 2, 31, 20};
    const A_long projected_width = (output->width + downsample_x - 1) / downsample_x;
    const A_long projected_height = (output->height + downsample_y - 1) / downsample_y;
    const A_LRect expected_region{1, 0, 16, 7};
    err = options_suite->AEGP_SetDownsampleFactor(options, downsample_x, downsample_y);
    if (!err) err = options_suite->AEGP_SetRegionOfInterest(options, &source_roi);
    if (!err) err = worlds->AEGP_NewPlatformWorld(g_plugin_id, AEGP_WorldType_8,
        projected_width, projected_height, &projected_platform);
    if (!err) err = worlds->AEGP_NewReferenceFromPlatformWorld(
        g_plugin_id, projected_platform, &projected_reference);
    if (!err) err = worlds->AEGP_GetRowBytes(projected_reference, &rowbytes);
    if (!err) err = worlds->AEGP_GetBaseAddr8(projected_reference, &pixels);
    if (!err && (!pixels || rowbytes < static_cast<A_u_long>(projected_width) * sizeof(PF_Pixel8)))
      err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err) for (A_long y = 0; y < projected_height; ++y) {
      auto* row = reinterpret_cast<PF_Pixel8*>(reinterpret_cast<A_u_char*>(pixels) +
                                               static_cast<size_t>(y) * rowbytes);
      for (A_long x = 0; x < projected_width; ++x) row[x] = expected_pixel(x, y);
    }
    if (!err) {
      err = renders->AEGP_CheckinRenderedFrame(options, &timestamp, 1, projected_platform);
      if (!err) projected_platform = nullptr;
    }
    if (!err) err = renders->AEGP_RenderAndCheckoutFrame(
        options, nullptr, nullptr, &projected_receipt);
    if (!err) err = renders->AEGP_GetReceiptWorld(projected_receipt, &projected_receipt_world);
    if (!err) err = renders->AEGP_GetRenderedRegion(projected_receipt, &rendered_region);
    if (!err) err = worlds->AEGP_GetSize(projected_receipt_world, &width, &height);
    if (!err) err = worlds->AEGP_GetRowBytes(projected_receipt_world, &rowbytes);
    if (!err) err = worlds->AEGP_GetBaseAddr8(projected_receipt_world, &pixels);
    if (!err && (width != projected_width || height != projected_height || !pixels ||
        std::memcmp(&rendered_region, &expected_region, sizeof(expected_region)) != 0))
      err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err) for (A_long y = 0; y < height && !err; ++y) {
      const auto* row = reinterpret_cast<const PF_Pixel8*>(
          reinterpret_cast<const A_u_char*>(pixels) + static_cast<size_t>(y) * rowbytes);
      for (A_long x = 0; x < width; ++x) {
        const PF_Pixel8 expected = expected_pixel(x, y);
        if (std::memcmp(&row[x], &expected, sizeof(expected)) != 0) {
          err = PF_Err_BAD_CALLBACK_PARAM;
          break;
        }
      }
    }
    if (projected_receipt) {
      AEGP_FrameReceiptH stale_projected_receipt = projected_receipt;
      keep_first(err, renders->AEGP_CheckinFrame(projected_receipt));
      projected_receipt = nullptr;
      if (!err && renders->AEGP_GetRenderedRegion(
          stale_projected_receipt, &rendered_region) == A_Err_NONE)
        err = PF_Err_BAD_CALLBACK_PARAM;
    }
  }
  if (!err && renders->AEGP_CheckinRenderedFrame(nullptr, &timestamp, 1, nullptr) == A_Err_NONE)
    err = PF_Err_BAD_CALLBACK_PARAM;

  if (projected_receipt) keep_first(err, renders->AEGP_CheckinFrame(projected_receipt));
  if (projected_reference) keep_first(err, worlds->AEGP_Dispose(projected_reference));
  if (projected_platform) keep_first(err, worlds->AEGP_DisposePlatformWorld(projected_platform));
  if (receipt) keep_first(err, renders->AEGP_CheckinFrame(receipt));
  if (writable_world) keep_first(err, worlds->AEGP_Dispose(writable_world));
  if (platform) keep_first(err, worlds->AEGP_DisposePlatformWorld(platform));
  if (options) keep_first(err, options_suite->AEGP_Dispose(options));
  if (renders) keep_first(err, in_data->pica_basicP->ReleaseSuite(kAEGPRenderSuite, kAEGPRenderSuiteVersion5));
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
