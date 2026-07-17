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
static_assert(offsetof(AEGP_WorldSuite3, AEGP_FillOutPFEffectWorld) == 8 * sizeof(void*));

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
      nullptr, "PF AEGP Owned World Probe", &g_plugin_id);
  if (utility) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kAEGPUtilitySuite, kAEGPUtilitySuiteVersion3));
  return err;
}

PF_Err exercise_depth(const AEGP_WorldSuite3* worlds, AEGP_WorldType expected,
                      A_long width, A_long height) {
  AEGP_WorldH world = nullptr;
  PF_Err err = static_cast<PF_Err>(worlds->AEGP_New(
      g_plugin_id, expected, width, height, &world));
  if (!err && !world) err = PF_Err_BAD_CALLBACK_PARAM;

  AEGP_WorldType type = AEGP_WorldType_NONE;
  A_long actual_width = 0, actual_height = 0;
  A_u_long rowbytes = 0;
  PF_EffectWorld projection{};
  if (!err) err = worlds->AEGP_GetType(world, &type);
  if (!err) err = worlds->AEGP_GetSize(world, &actual_width, &actual_height);
  if (!err) err = worlds->AEGP_GetRowBytes(world, &rowbytes);
  if (!err) err = worlds->AEGP_FillOutPFEffectWorld(world, &projection);

  PF_Pixel8* pixels8 = nullptr;
  PF_Pixel16* pixels16 = nullptr;
  PF_PixelFloat* pixels32 = nullptr;
  const A_Err base8 = worlds->AEGP_GetBaseAddr8(world, &pixels8);
  const A_Err base16 = worlds->AEGP_GetBaseAddr16(world, &pixels16);
  const A_Err base32 = worlds->AEGP_GetBaseAddr32(world, &pixels32);
  const void* selected = expected == AEGP_WorldType_8 ? static_cast<void*>(pixels8) :
      expected == AEGP_WorldType_16 ? static_cast<void*>(pixels16) : static_cast<void*>(pixels32);
  const A_u_long pixel_size = expected == AEGP_WorldType_8 ? sizeof(PF_Pixel8) :
      expected == AEGP_WorldType_16 ? sizeof(PF_Pixel16) : sizeof(PF_PixelFloat);
  if (!err && (type != expected || actual_width != width || actual_height != height ||
      rowbytes < static_cast<A_u_long>(width) * pixel_size || !selected ||
      projection.width != width || projection.height != height ||
      projection.rowbytes != static_cast<A_long>(rowbytes) || projection.data != selected ||
      (expected == AEGP_WorldType_8 ? base8 : expected == AEGP_WorldType_16 ? base16 : base32) ||
      (expected == AEGP_WorldType_8 ? (!base16 || !base32) :
       expected == AEGP_WorldType_16 ? (!base8 || !base32) : (!base8 || !base16))))
    err = PF_Err_BAD_CALLBACK_PARAM;

  if (world) {
    AEGP_WorldH stale = world;
    keep_first(err, worlds->AEGP_Dispose(world));
    world = nullptr;
    if (!err && (worlds->AEGP_GetType(stale, &type) == A_Err_NONE ||
                 worlds->AEGP_Dispose(stale) == A_Err_NONE))
      err = PF_Err_BAD_CALLBACK_PARAM;
  }
  return err;
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !output || !output->data || !g_plugin_id)
    return PF_Err_BAD_CALLBACK_PARAM;
  const AEGP_WorldSuite3* worlds = nullptr;
  PF_Err err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kAEGPWorldSuite, kAEGPWorldSuiteVersion3,
      reinterpret_cast<const void**>(&worlds)));
  if (!err && (!worlds || !worlds->AEGP_New || !worlds->AEGP_Dispose ||
      !worlds->AEGP_GetType || !worlds->AEGP_GetSize || !worlds->AEGP_GetRowBytes ||
      !worlds->AEGP_GetBaseAddr8 || !worlds->AEGP_GetBaseAddr16 ||
      !worlds->AEGP_GetBaseAddr32 || !worlds->AEGP_FillOutPFEffectWorld))
    err = PF_Err_INVALID_CALLBACK;

  AEGP_WorldH invalid = nullptr;
  if (!err && (worlds->AEGP_New(g_plugin_id, AEGP_WorldType_8, 4, 3, nullptr) == A_Err_NONE ||
      worlds->AEGP_New(g_plugin_id, AEGP_WorldType_8, 0, 3, &invalid) == A_Err_NONE || invalid ||
      worlds->AEGP_New(g_plugin_id, AEGP_WorldType_NONE, 4, 3, &invalid) == A_Err_NONE || invalid ||
      worlds->AEGP_GetType(nullptr, nullptr) == A_Err_NONE ||
      worlds->AEGP_GetSize(nullptr, nullptr, nullptr) == A_Err_NONE ||
      worlds->AEGP_GetRowBytes(nullptr, nullptr) == A_Err_NONE ||
      worlds->AEGP_GetBaseAddr8(nullptr, nullptr) == A_Err_NONE ||
      worlds->AEGP_FillOutPFEffectWorld(nullptr, nullptr) == A_Err_NONE))
    err = PF_Err_BAD_CALLBACK_PARAM;

  const AEGP_WorldType depths[] = {AEGP_WorldType_8, AEGP_WorldType_16, AEGP_WorldType_32};
  for (AEGP_WorldType depth : depths) if (!err) err = exercise_depth(worlds, depth, 11, 7);
  if (worlds) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kAEGPWorldSuite, kAEGPWorldSuiteVersion3));
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
