#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"
#include "Param_Utils.h"

#include <algorithm>
#include <cstdlib>
#include <cstring>

namespace {
enum ParamIndex { kInput = 0, kOperation = 1, kDirection = 2, kPlacement = 3, kNumParams = 4 };
enum Operation { kPremultiply = 1, kColor8 = 2, kColor16 = 3, kColorFloat = 4 };

template <typename Pixel, typename Component>
void seed_world(PF_EffectWorld* world, Component maximum) {
  // Columns intentionally straddle alpha zero, half-alpha rounding, and full alpha.
  static const double alpha[] = {0.0, 1.0 / 255.0, 0.5, 128.0 / 255.0, 1.0};
  static const double red[] = {1.0, 1.0, 1.0, 127.0 / 255.0, 1.0};
  for (A_long y = 0; y < world->height; ++y) {
    Pixel* row = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(world->data) + y * world->rowbytes);
    for (A_long x = 0; x < world->width; ++x) {
      const int i = static_cast<int>(x % 5);
      row[x].alpha = static_cast<Component>(alpha[i] * maximum + 0.5);
      row[x].red = static_cast<Component>(red[i] * maximum + 0.5);
      row[x].green = static_cast<Component>((y & 1 ? 0.25 : 0.75) * maximum + 0.5);
      row[x].blue = static_cast<Component>((i & 1 ? 0.501 : 0.499) * maximum + 0.5);
    }
  }
}

template <>
void seed_world<PF_PixelFloat, PF_FpShort>(PF_EffectWorld* world, PF_FpShort) {
  static const PF_FpShort alpha[] = {0.0f, 1.0f / 255.0f, 0.5f, 128.0f / 255.0f, 1.0f};
  static const PF_FpShort red[] = {1.0f, 1.0f, 1.0f, 127.0f / 255.0f, 1.0f};
  for (A_long y = 0; y < world->height; ++y) {
    PF_PixelFloat* row = reinterpret_cast<PF_PixelFloat*>(
        reinterpret_cast<A_u_char*>(world->data) + y * world->rowbytes);
    for (A_long x = 0; x < world->width; ++x) {
      const int i = static_cast<int>(x % 5);
      row[x] = {alpha[i], red[i], y & 1 ? 0.25f : 0.75f, i & 1 ? 0.501f : 0.499f};
    }
  }
}

PF_Err setup_params(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def{};
  PF_ADD_POPUP("Suite operation", 4, kPremultiply,
               "premultiply|premultiply_color (8)|premultiply_color16|premultiply_color_float",
               kOperation);
  PF_ADD_POPUP("Direction", 2, 1, "forward|reverse", kDirection);
  PF_ADD_POPUP("Placement", 2, 1, "in-place|separate source/destination", kPlacement);
  out_data->num_params = kNumParams;
  return PF_Err_NONE;
}

PF_Err render(PF_InData* in_data, PF_ParamDef* params[], PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !output || !output->data)
    return PF_Err_BAD_CALLBACK_PARAM;

  const PF_FillMatteSuite2* suite = nullptr;
  PF_Err err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFFillMatteSuite, kPFFillMatteSuiteVersion2,
      reinterpret_cast<const void**>(&suite)));
  if (err || !suite) return err ? err : PF_Err_INVALID_CALLBACK;

  const PF_WorldSuite2* world_suite = nullptr;
  err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFWorldSuite, kPFWorldSuiteVersion2,
      reinterpret_cast<const void**>(&world_suite)));
  if (err || !world_suite) {
    in_data->pica_basicP->ReleaseSuite(kPFFillMatteSuite, kPFFillMatteSuiteVersion2);
    return err ? err : PF_Err_INVALID_CALLBACK;
  }

  const A_long operation = params[kOperation]->u.pd.value;
  const A_long forward = params[kDirection]->u.pd.value == 1;
  const bool in_place = params[kPlacement]->u.pd.value == 1;
  PF_EffectWorld source = *output;
  void* source_data = nullptr;
  PF_PixelFormat format = PF_PixelFormat_INVALID;
  err = world_suite->PF_GetPixelFormat(output, &format);
  const bool format_matches =
      operation == kPremultiply ||
      (operation == kColor8 && format == PF_PixelFormat_ARGB32) ||
      (operation == kColor16 && format == PF_PixelFormat_ARGB64) ||
      (operation == kColorFloat && format == PF_PixelFormat_ARGB128);
  if (err || !format_matches) {
    in_data->pica_basicP->ReleaseSuite(kPFWorldSuite, kPFWorldSuiteVersion2);
    in_data->pica_basicP->ReleaseSuite(kPFFillMatteSuite, kPFFillMatteSuiteVersion2);
    return err ? err : PF_Err_BAD_CALLBACK_PARAM;
  }

  const std::size_t bytes = static_cast<std::size_t>(output->rowbytes) * output->height;
  if (!in_place && operation != kPremultiply) {
    source_data = std::malloc(bytes);
    if (!source_data) {
      in_data->pica_basicP->ReleaseSuite(kPFFillMatteSuite, kPFFillMatteSuiteVersion2);
      return PF_Err_OUT_OF_MEMORY;
    }
    source.data = reinterpret_cast<PF_PixelPtr>(source_data);
  }

  PF_EffectWorld* seed = source_data ? &source : output;
  if (operation == kColorFloat) {
    seed_world<PF_PixelFloat, PF_FpShort>(seed, 1.0f);
    const PF_PixelFloat color = {0.25f, 0.8f, 0.4f, 0.6f};
    err = suite->premultiply_color_float(in_data->effect_ref, seed, &color, forward, output);
  } else if (operation == kColor16) {
    seed_world<PF_Pixel16, A_u_short>(seed, PF_MAX_CHAN16);
    const PF_Pixel16 color = {PF_MAX_CHAN16 / 4, PF_MAX_CHAN16 * 4 / 5,
                              PF_MAX_CHAN16 * 2 / 5, PF_MAX_CHAN16 * 3 / 5};
    err = suite->premultiply_color16(in_data->effect_ref, seed, &color, forward, output);
  } else if (operation == kColor8 || format == PF_PixelFormat_ARGB32) {
    seed_world<PF_Pixel, A_u_char>(seed, PF_MAX_CHAN8);
    if (operation == kPremultiply) {
      err = suite->premultiply(in_data->effect_ref, forward, output);
    } else {
      const PF_Pixel color = {64, 204, 102, 153};
      err = suite->premultiply_color(in_data->effect_ref, seed, &color, forward, output);
    }
  } else if (format == PF_PixelFormat_ARGB64) {
    seed_world<PF_Pixel16, A_u_short>(seed, PF_MAX_CHAN16);
    err = suite->premultiply(in_data->effect_ref, forward, output);
  } else {
    seed_world<PF_PixelFloat, PF_FpShort>(seed, 1.0f);
    err = suite->premultiply(in_data->effect_ref, forward, output);
  }

  std::free(source_data);
  const SPErr world_release_err = in_data->pica_basicP->ReleaseSuite(
      kPFWorldSuite, kPFWorldSuiteVersion2);
  const SPErr release_err = in_data->pica_basicP->ReleaseSuite(
      kPFFillMatteSuite, kPFFillMatteSuiteVersion2);
  if (err) return err;
  if (world_release_err) return static_cast<PF_Err>(world_release_err);
  return static_cast<PF_Err>(release_err);
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
      out_data->out_flags2 = PF_OutFlag2_FLOAT_COLOR_AWARE;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      return setup_params(in_data, out_data);
    case PF_Cmd_RENDER:
      return render(in_data, params, output);
    default:
      return PF_Err_NONE;
  }
}
