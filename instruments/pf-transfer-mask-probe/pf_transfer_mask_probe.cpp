#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"

#include <cstddef>
#include <cstring>
#include <type_traits>

namespace {
static_assert(std::is_standard_layout_v<PF_WorldTransformSuite1>);
static_assert(offsetof(PF_WorldTransformSuite1, transfer_rect) == 5 * sizeof(void*));
static_assert(offsetof(PF_MaskWorld, offset) == sizeof(PF_EffectWorld));

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
  PF_EffectWorld source{}, destination{}, mask_pixels{};
  bool source_created = false, destination_created = false, mask_created = false;
  if (!err) { err = worlds->PF_NewWorld(in_data->effect_ref, 3, 1, TRUE,
      PF_PixelFormat_ARGB32, &source); source_created = !err; }
  if (!err) { err = worlds->PF_NewWorld(in_data->effect_ref, 3, 1, TRUE,
      PF_PixelFormat_ARGB32, &destination); destination_created = !err; }
  if (!err) { err = worlds->PF_NewWorld(in_data->effect_ref, 3, 1, TRUE,
      PF_PixelFormat_ARGB32, &mask_pixels); mask_created = !err; }
  PF_Rect full{0, 0, 3, 1};
  PF_CompositeMode mode{};
  mode.xfer = PF_Xfer_IN_FRONT;
  mode.opacity = PF_MAX_CHAN8;
  mode.opacitySu = PF_MAX_CHAN16;
  mode.rgb_only = FALSE;
  PF_MaskWorld mask{};
  mask.mask = mask_pixels;
  mask.offset = PF_Point{0, 0};
  mask.what_is_mask = PF_MaskFlag_NONE;
  if (!err) {
    for (A_long x = 0; x < 3; ++x) *pixel(source, x, 0) = PF_Pixel8{255, 200, 0, 0};
    *pixel(mask.mask, 0, 0) = PF_Pixel8{0, 0, 0, 0};
    *pixel(mask.mask, 1, 0) = PF_Pixel8{128, 128, 128, 128};
    *pixel(mask.mask, 2, 0) = PF_Pixel8{255, 255, 255, 255};
    err = transforms->transfer_rect(in_data->effect_ref, PF_Quality_HI,
        PF_MF_Alpha_PREMUL, PF_Field_FRAME, &full, &source, &mode, &mask, 0, 0,
        &destination);
  }
  if (!err && (pixel(destination, 0, 0)->red != 0 ||
      pixel(destination, 1, 0)->red != 100 || pixel(destination, 2, 0)->red != 200))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) {
    std::memset(destination.data, 0, static_cast<size_t>(destination.rowbytes));
    mask.what_is_mask = PF_MaskFlag_INVERTED;
    err = transforms->transfer_rect(in_data->effect_ref, PF_Quality_HI,
        PF_MF_Alpha_PREMUL, PF_Field_FRAME, &full, &source, &mode, &mask, 0, 0,
        &destination);
  }
  if (!err && (pixel(destination, 0, 0)->red != 200 ||
      pixel(destination, 1, 0)->red != 100 || pixel(destination, 2, 0)->red != 0))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) {
    std::memset(destination.data, 0, static_cast<size_t>(destination.rowbytes));
    mask.what_is_mask = PF_MaskFlag_LUMINANCE;
    err = transforms->transfer_rect(in_data->effect_ref, PF_Quality_HI,
        PF_MF_Alpha_PREMUL, PF_Field_FRAME, &full, &source, &mode, &mask, 0, 0,
        &destination);
  }
  if (!err && (pixel(destination, 0, 0)->red != 0 ||
      pixel(destination, 1, 0)->red != 100 || pixel(destination, 2, 0)->red != 200))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) {
    std::memset(destination.data, 0, static_cast<size_t>(destination.rowbytes));
    mask.what_is_mask = PF_MaskFlag_NONE;
    mask.offset.h = 1;
    err = transforms->transfer_rect(in_data->effect_ref, PF_Quality_HI,
        PF_MF_Alpha_PREMUL, PF_Field_FRAME, &full, &source, &mode, &mask, 0, 0,
        &destination);
  }
  if (!err && (pixel(destination, 0, 0)->red != 0 ||
      pixel(destination, 1, 0)->red != 0 || pixel(destination, 2, 0)->red != 100))
    err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) {
    const PF_Pixel8 sentinel{90, 90, 90, 90};
    *pixel(destination, 0, 0) = sentinel;
    mask.what_is_mask = static_cast<PF_MaskFlags>(4);
    const PF_Err rejected = transforms->transfer_rect(in_data->effect_ref, PF_Quality_HI,
        PF_MF_Alpha_PREMUL, PF_Field_FRAME, &full, &source, &mode, &mask, 0, 0,
        &destination);
    if (rejected != PF_Err_BAD_CALLBACK_PARAM ||
        std::memcmp(pixel(destination, 0, 0), &sentinel, sizeof(sentinel)))
      err = PF_Err_BAD_CALLBACK_PARAM;
  }
  if (mask_created) keep_first(err, worlds->PF_DisposeWorld(in_data->effect_ref, &mask_pixels));
  if (destination_created) keep_first(err, worlds->PF_DisposeWorld(in_data->effect_ref, &destination));
  if (source_created) keep_first(err, worlds->PF_DisposeWorld(in_data->effect_ref, &source));
  if (worlds) keep_first(err, in_data->pica_basicP->ReleaseSuite(kPFWorldSuite, kPFWorldSuiteVersion2));
  if (transforms) keep_first(err, in_data->pica_basicP->ReleaseSuite(
      kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1));
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
