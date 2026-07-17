#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"

#include <algorithm>

namespace {
constexpr A_long kCaseCount = 8;

template <typename Pixel, typename Component>
void seed_world(PF_EffectWorld* world, Component maximum, bool source) {
  for (A_long y = 0; y < world->height; ++y) {
    auto* row = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(world->data) + y * world->rowbytes);
    for (A_long x = 0; x < world->width; ++x) {
      const A_long phase = (x + 3 * y) % 7;
      if (source) {
        row[x].alpha = static_cast<Component>(((phase + 1) * maximum) / 8);
        row[x].red = static_cast<Component>(((x % 5) + 1) * maximum / 5);
        row[x].green = static_cast<Component>(((y % 4) + 1) * maximum / 4);
        row[x].blue = static_cast<Component>(((phase % 3) + 1) * maximum / 3);
      } else {
        row[x].alpha = static_cast<Component>(maximum * 3 / 4);
        row[x].red = static_cast<Component>(((y & 1) ? 1 : 3) * maximum / 8);
        row[x].green = static_cast<Component>(((x & 1) ? 3 : 1) * maximum / 8);
        row[x].blue = static_cast<Component>(maximum / 4);
      }
    }
  }
}

PF_Err invoke(PF_WorldTransformSuite1 const* suite, PF_ProgPtr effect_ref,
              PF_EffectWorld* source, PF_EffectWorld* dest, PF_Rect rect,
              A_long opacity, A_long x, A_long y, PF_Field field,
              PF_XferMode mode) {
  return suite->composite_rect(effect_ref, &rect, opacity, source, x, y,
                               field, mode, dest);
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !output || !output->data ||
      output->width < 16 || output->height < 2 || output->rowbytes <= 0) {
    return PF_Err_BAD_CALLBACK_PARAM;
  }

  const PF_WorldTransformSuite1* suite = nullptr;
  PF_Err err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1,
      reinterpret_cast<const void**>(&suite)));
  if (err || !suite || !suite->composite_rect) {
    return err ? err : PF_Err_INVALID_CALLBACK;
  }

  const PF_WorldSuite2* world_suite = nullptr;
  err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFWorldSuite, kPFWorldSuiteVersion2,
      reinterpret_cast<const void**>(&world_suite)));
  if (err || !world_suite || !world_suite->PF_NewWorld ||
      !world_suite->PF_DisposeWorld || !world_suite->PF_GetPixelFormat) {
    const SPErr release_err = in_data->pica_basicP->ReleaseSuite(
        kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1);
    return err ? err : (release_err ? static_cast<PF_Err>(release_err)
                                    : PF_Err_INVALID_CALLBACK);
  }

  PF_EffectWorld source{};
  PF_PixelFormat format = PF_PixelFormat_INVALID;
  err = world_suite->PF_GetPixelFormat(output, &format);
  if (!err && format != PF_PixelFormat_ARGB32 &&
      format != PF_PixelFormat_ARGB64) {
    err = PF_Err_BAD_CALLBACK_PARAM;
  }
  bool source_created = false;
  if (!err) {
    err = world_suite->PF_NewWorld(in_data->effect_ref, output->width,
                                   output->height, TRUE, format, &source);
    source_created = !err;
  }
  if (!err && format == PF_PixelFormat_ARGB64) {
    seed_world<PF_Pixel16, A_u_short>(&source, PF_MAX_CHAN16, true);
    seed_world<PF_Pixel16, A_u_short>(output, PF_MAX_CHAN16, false);
  } else if (!err) {
    seed_world<PF_Pixel, A_u_char>(&source, PF_MAX_CHAN8, true);
    seed_world<PF_Pixel, A_u_char>(output, PF_MAX_CHAN8, false);
  }

  const A_long band = std::max<A_long>(2, output->width / kCaseCount);
  auto band_rect = [&](A_long index) {
    PF_Rect rect{};
    rect.left = index * band;
    rect.top = 0;
    rect.right = std::min(output->width, rect.left + band);
    rect.bottom = output->height;
    return rect;
  };

  PF_Rect rect = band_rect(0);
  if (!err) {
    err = invoke(suite, in_data->effect_ref, &source, output, rect, 255,
                 rect.left, 0, PF_Field_FRAME, PF_Xfer_COPY);
  }
  if (!err) {
    rect = band_rect(1);
    err = invoke(suite, in_data->effect_ref, &source, output, rect, 128,
                 rect.left, 0, PF_Field_FRAME, PF_Xfer_COPY);
  }
  if (!err) {
    rect = band_rect(2);
    err = invoke(suite, in_data->effect_ref, &source, output, rect, 255,
                 rect.left, 0, PF_Field_FRAME, PF_Xfer_IN_FRONT);
  }
  if (!err) {
    rect = band_rect(3);
    err = invoke(suite, in_data->effect_ref, &source, output, rect, 255,
                 rect.left, 0, PF_Field_FRAME, PF_Xfer_BEHIND);
  }
  if (!err) {
    rect = band_rect(4);
    rect.left -= band / 2;
    err = invoke(suite, in_data->effect_ref, &source, output, rect, 255,
                 4 * band - band / 2, 0, PF_Field_FRAME, PF_Xfer_COPY);
  }
  if (!err) {
    rect = band_rect(5);
    err = invoke(suite, in_data->effect_ref, &source, output, rect, 255,
                 rect.left, 0, PF_Field_UPPER, PF_Xfer_COPY);
  }
  if (!err) {
    rect = band_rect(6);
    err = invoke(suite, in_data->effect_ref, &source, output, rect, 255,
                 rect.left, 0, PF_Field_LOWER, PF_Xfer_COPY);
  }
  if (!err) {
    rect = band_rect(7);
    if (rect.right - rect.left > 1) {
      --rect.right;
      err = invoke(suite, in_data->effect_ref, output, output, rect, 255,
                   rect.left + 1, 0, PF_Field_FRAME, PF_Xfer_COPY);
    }
  }

  PF_Err dispose_err = PF_Err_NONE;
  if (source_created) {
    dispose_err = world_suite->PF_DisposeWorld(in_data->effect_ref, &source);
  }
  const SPErr world_release_err = in_data->pica_basicP->ReleaseSuite(
      kPFWorldSuite, kPFWorldSuiteVersion2);
  const SPErr release_err = in_data->pica_basicP->ReleaseSuite(
      kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1);
  if (err) return err;
  if (dispose_err) return dispose_err;
  if (world_release_err) return static_cast<PF_Err>(world_release_err);
  return static_cast<PF_Err>(release_err);
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data,
                                        PF_ParamDef*[], PF_LayerDef* output,
                                        void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT |
                            PF_OutFlag_DEEP_COLOR_AWARE;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER:
      return render(in_data, output);
    default:
      return PF_Err_NONE;
  }
}
