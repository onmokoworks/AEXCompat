#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"

#include <cstddef>
#include <cstring>
#include <type_traits>

namespace {
static_assert(std::is_standard_layout_v<PF_WorldTransformSuite1>);
static_assert(sizeof(PF_WorldTransformSuite1) == 7 * sizeof(void*));
static_assert(offsetof(PF_WorldTransformSuite1, transfer_rect) == 5 * sizeof(void*));

template <typename T> void keep_first(PF_Err& first, T candidate) {
  if (!first && candidate) first = static_cast<PF_Err>(candidate);
}

PF_Pixel8* pixel(PF_EffectWorld& world, A_long x, A_long y) {
  return reinterpret_cast<PF_Pixel8*>(reinterpret_cast<A_u_char*>(world.data) +
                                      static_cast<size_t>(y) * world.rowbytes) + x;
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !in_data->effect_ref || !output || !output->data)
    return PF_Err_BAD_CALLBACK_PARAM;
  const PF_WorldTransformSuite1* transforms = nullptr;
  const PF_WorldSuite2* worlds = nullptr;
  PF_Err err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1,
      reinterpret_cast<const void**>(&transforms)));
  if (!err) err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFWorldSuite, kPFWorldSuiteVersion2, reinterpret_cast<const void**>(&worlds)));
  if (!err && (!transforms || !transforms->transfer_rect || !worlds ||
      !worlds->PF_NewWorld || !worlds->PF_DisposeWorld)) err = PF_Err_INVALID_CALLBACK;
  PF_EffectWorld source{}, destination{};
  bool source_created = false, destination_created = false;
  if (!err) { err = worlds->PF_NewWorld(in_data->effect_ref, 3, 2, TRUE,
      PF_PixelFormat_ARGB32, &source); source_created = !err; }
  if (!err) { err = worlds->PF_NewWorld(in_data->effect_ref, 3, 2, TRUE,
      PF_PixelFormat_ARGB32, &destination); destination_created = !err; }
  PF_Rect full{0, 0, 3, 2};
  PF_CompositeMode mode{};
  mode.xfer = PF_Xfer_IN_FRONT;
  mode.opacity = PF_MAX_CHAN8;
  mode.opacitySu = PF_MAX_CHAN16;
  mode.rgb_only = FALSE;
  if (!err) {
    *pixel(source, 0, 0) = PF_Pixel8{128, 200, 20, 100};
    *pixel(destination, 0, 0) = PF_Pixel8{64, 20, 100, 200};
    err = transforms->transfer_rect(in_data->effect_ref, PF_Quality_HI,
        PF_MF_Alpha_STRAIGHT, PF_Field_FRAME, &full, &source, &mode, nullptr, 0, 0,
        &destination);
  }
  const PF_Pixel8 source_over{160, 164, 36, 120};
  if (!err && std::memcmp(pixel(destination, 0, 0), &source_over, sizeof(source_over)))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) {
    *pixel(source, 0, 0) = PF_Pixel8{128, 200, 20, 100};
    *pixel(destination, 0, 0) = PF_Pixel8{64, 20, 100, 200};
    mode.xfer = PF_Xfer_DIFFERENCE;
    mode.opacity = 128;
    mode.opacitySu = 16448;
    mode.rgb_only = TRUE;
    err = transforms->transfer_rect(in_data->effect_ref, PF_Quality_HI,
        PF_MF_Alpha_STRAIGHT, PF_Field_FRAME, &full, &source, &mode, nullptr, 0, 0,
        &destination);
  }
  const PF_Pixel8 difference{64, 100, 90, 150};
  if (!err && std::memcmp(pixel(destination, 0, 0), &difference, sizeof(difference)))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (destination_created) keep_first(err, worlds->PF_DisposeWorld(in_data->effect_ref, &destination));
  if (source_created) keep_first(err, worlds->PF_DisposeWorld(in_data->effect_ref, &source));
  if (worlds) keep_first(err, in_data->pica_basicP->ReleaseSuite(kPFWorldSuite, kPFWorldSuiteVersion2));
  if (transforms) keep_first(err, in_data->pica_basicP->ReleaseSuite(kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1));
  if (!err) std::memset(output->data, 0, static_cast<size_t>(output->rowbytes) * output->height);
  return err;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
    PF_OutData* out_data, PF_ParamDef*[], PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP: out_data->num_params = 1; return PF_Err_NONE;
    case PF_Cmd_RENDER: return render(in_data, output);
    default: return PF_Err_NONE;
  }
}
