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
static_assert(offsetof(AEGP_WorldSuite3, AEGP_New) == 0 * sizeof(void*));
static_assert(offsetof(AEGP_WorldSuite3, AEGP_Dispose) == 1 * sizeof(void*));
static_assert(offsetof(AEGP_WorldSuite3, AEGP_FastBlur) == 9 * sizeof(void*));

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
      nullptr, "PF AEGP Fast Blur Probe", &g_plugin_id);
  if (utility) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3));
  return err;
}

void seed_impulses(PF_Pixel8* pixels, A_u_long rowbytes) {
  std::memset(pixels, 0, static_cast<size_t>(rowbytes) * 7);
  auto row = [pixels, rowbytes](A_long y) {
    return reinterpret_cast<PF_Pixel8*>(reinterpret_cast<A_u_char*>(pixels) + y * rowbytes);
  };
  row(3)[5] = {255, 240, 80, 20};
  row(1)[2] = {192, 12, 160, 48};
  row(5)[8] = {128, 32, 64, 224};
}

bool has_blurred_pixels(const PF_Pixel8* pixels, A_u_long rowbytes) {
  unsigned nonzero = 0;
  unsigned alpha_sum = 0;
  for (A_long y = 0; y < 7; ++y) {
    const auto* row = reinterpret_cast<const PF_Pixel8*>(
        reinterpret_cast<const A_u_char*>(pixels) + y * rowbytes);
    for (A_long x = 0; x < 11; ++x) {
      if (row[x].alpha || row[x].red || row[x].green || row[x].blue) ++nonzero;
      alpha_sum += row[x].alpha;
    }
  }
  return nonzero > 3 && alpha_sum > 0;
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !output || !output->data || !g_plugin_id)
    return PF_Err_BAD_CALLBACK_PARAM;
  const AEGP_WorldSuite3* worlds = nullptr;
  PF_Err err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kAEGPWorldSuite, kAEGPWorldSuiteVersion3,
      reinterpret_cast<const void**>(&worlds)));
  if (!err && (!worlds || !worlds->AEGP_New || !worlds->AEGP_Dispose ||
      !worlds->AEGP_GetBaseAddr8 || !worlds->AEGP_FastBlur)) err = PF_Err_INVALID_CALLBACK;

  AEGP_WorldH world = nullptr;
  if (!err) err = static_cast<PF_Err>(worlds->AEGP_New(
      g_plugin_id, AEGP_WorldType_8, 11, 7, &world));
  PF_Pixel8* pixels = nullptr;
  A_u_long rowbytes = 0;
  if (!err) err = static_cast<PF_Err>(worlds->AEGP_GetBaseAddr8(world, &pixels));
  if (!err) err = static_cast<PF_Err>(worlds->AEGP_GetRowBytes(world, &rowbytes));
  if (!err && (!pixels || rowbytes < 11 * sizeof(PF_Pixel8))) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) seed_impulses(pixels, rowbytes);
  if (!err) err = static_cast<PF_Err>(worlds->AEGP_FastBlur(
      2.0, PF_MF_Alpha_STRAIGHT, PF_Quality_HI, world));
  if (!err && !has_blurred_pixels(pixels, rowbytes)) err = PF_Err_BAD_CALLBACK_PARAM;

  if (!err && (worlds->AEGP_FastBlur(2.0, PF_MF_Alpha_STRAIGHT, PF_Quality_HI, nullptr) == A_Err_NONE ||
      worlds->AEGP_FastBlur(-1.0, PF_MF_Alpha_STRAIGHT, PF_Quality_HI, world) == A_Err_NONE))
    err = PF_Err_BAD_CALLBACK_PARAM;

  if (!err) {
    const A_long copy_width = output->width < 11 ? output->width : 11;
    const A_long copy_height = output->height < 7 ? output->height : 7;
    std::memset(output->data, 0, static_cast<size_t>(output->rowbytes) * output->height);
    for (A_long y = 0; y < copy_height; ++y) {
      std::memcpy(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                  reinterpret_cast<const A_u_char*>(pixels) + y * rowbytes,
                  static_cast<size_t>(copy_width) * sizeof(PF_Pixel8));
    }
  }

  if (world) {
    AEGP_WorldH stale = world;
    keep_first(err, worlds->AEGP_Dispose(world));
    world = nullptr;
    if (!err && (worlds->AEGP_FastBlur(2.0, PF_MF_Alpha_STRAIGHT, PF_Quality_HI, stale) == A_Err_NONE ||
                 worlds->AEGP_Dispose(stale) == A_Err_NONE)) err = PF_Err_BAD_CALLBACK_PARAM;
  }
  if (worlds) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kAEGPWorldSuite, kAEGPWorldSuiteVersion3));
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
