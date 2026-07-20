// A minimal custom-UI Effect AEX probe for issue #242: it responds to a
// comp/layer click during render by opening the host color picker, storing the
// picked color in a color parameter (index 1), invalidating the control, and
// flagging the parameter changed; RENDER then fills the output with that color.
// A draw event paints one drawbot rectangle. This is the smallest effect that
// satisfies the worker's render-path custom-UI contract (color picker once,
// invalidate once, PF_EO_HANDLED_EVENT|PF_EO_UPDATE_NOW on click, changed value
// on param 1, one drawbot command on draw), so the resident-session and
// one-shot routes can be A/B'd byte-for-byte against a real custom-UI plug-in.
//
// Unlike the SDK ColorGrid sample it uses a plain color parameter instead of
// arbitrary data, so the render path needs no arb-data checkout (ColorGrid
// renders with error -5 through the minihost worker).

#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_EffectCBSuites.h"
#include "AE_EffectSuites.h"
#include "AE_EffectUI.h"
#include "AE_Macros.h"
#include "Param_Utils.h"
#include "adobesdk/DrawbotSuite.h"

#include <algorithm>
#include <cstdint>
#include <cstring>

namespace {

enum { kParamInput = 0, kParamColor, kNumParams };

constexpr A_long kColorDiskId = 1;

// Acquire/release the host App suite for the lifetime of the lease. The
// minihost worker provides PF AE App Suite versions 1/6/7; version 1 is the
// current PFAppSuite6 struct, which carries PF_AppColorPickerDialog and
// PF_InvalidateRect.
struct AppSuiteLease {
  SPBasicSuite* basic = nullptr;
  PFAppSuite6* suite = nullptr;

  explicit AppSuiteLease(SPBasicSuite* basic_suite) : basic(basic_suite) {
    if (basic) {
      const void* acquired = nullptr;
      if (basic->AcquireSuite(kPFAppSuite, kPFAppSuiteVersion6, &acquired) ==
          kSPNoError) {
        suite = const_cast<PFAppSuite6*>(
            static_cast<const PFAppSuite6*>(acquired));
      }
    }
  }
  ~AppSuiteLease() {
    if (basic && suite) basic->ReleaseSuite(kPFAppSuite, kPFAppSuiteVersion6);
  }
};

PF_Err GlobalSetup(PF_OutData* out_data) {
  out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
  out_data->out_flags = PF_OutFlag_CUSTOM_UI | PF_OutFlag_DEEP_COLOR_AWARE |
                        PF_OutFlag_PIX_INDEPENDENT;
  out_data->out_flags2 = PF_OutFlag2_FLOAT_COLOR_AWARE;
  return PF_Err_NONE;
}

PF_Err ParamsSetup(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def{};
  AEFX_CLR_STRUCT(def);
  // Param index 1: the color the click sets and RENDER paints.
  PF_ADD_COLOR("Color", 128, 128, 128, kColorDiskId);

  PF_CustomUIInfo ci{};
  AEFX_CLR_STRUCT(ci);
  ci.events = PF_CustomEFlag_EFFECT;
  ci.comp_ui_width = ci.comp_ui_height = 0;
  ci.comp_ui_alignment = PF_UIAlignment_NONE;
  ci.layer_ui_width = ci.layer_ui_height = 0;
  ci.layer_ui_alignment = PF_UIAlignment_NONE;
  ci.preview_ui_width = ci.preview_ui_height = 0;
  const PF_Err err = (*in_data->inter.register_ui)(in_data->effect_ref, &ci);
  out_data->num_params = kNumParams;
  return err;
}

PF_Err DoClick(PF_InData* in_data, PF_ParamDef* params[], PF_EventExtra* extra) {
  AppSuiteLease app(in_data->pica_basicP);
  if (!app.suite) return PF_Err_BAD_CALLBACK_PARAM;

  const PF_Pixel current = params[kParamColor]->u.cd.value;
  PF_PixelFloat sample{};
  sample.alpha = current.alpha / 255.0;
  sample.red = current.red / 255.0;
  sample.green = current.green / 255.0;
  sample.blue = current.blue / 255.0;

  PF_PixelFloat picked = sample;
  PF_Err err = (*app.suite->PF_AppColorPickerDialog)("Custom UI Probe", &sample,
                                                     TRUE, &picked);
  if (err != PF_Err_NONE) return err;

  const auto to8 = [](PF_FpShort value) -> A_u_char {
    const double scaled = static_cast<double>(value) * 255.0 + 0.5;
    return static_cast<A_u_char>(std::clamp(scaled, 0.0, 255.0));
  };
  PF_Pixel updated{};
  updated.alpha = to8(picked.alpha);
  updated.red = to8(picked.red);
  updated.green = to8(picked.green);
  updated.blue = to8(picked.blue);
  params[kParamColor]->u.cd.value = updated;

  PF_Rect inval = extra->effect_win.current_frame;
  err = (*app.suite->PF_InvalidateRect)(extra->contextH, &inval);
  if (err != PF_Err_NONE) return err;

  extra->evt_out_flags |= PF_EO_HANDLED_EVENT | PF_EO_UPDATE_NOW;
  params[kParamColor]->uu.change_flags |= PF_ChangeFlag_CHANGED_VALUE;
  return PF_Err_NONE;
}

PF_Err DrawEvent(PF_InData* in_data, PF_EventExtra* extra) {
  SPBasicSuite* basic = in_data->pica_basicP;
  if (!basic) return PF_Err_BAD_CALLBACK_PARAM;

  // Acquire the drawing reference and paint one rectangle onto the control
  // surface so the worker observes at least one drawbot command (its draw
  // contract requires paint/fill/stroke/overlay > 0).
  const void* effect_ui_p = nullptr;
  if (basic->AcquireSuite(kPFEffectCustomUISuite, kPFEffectCustomUISuiteVersion1,
                          &effect_ui_p) != kSPNoError ||
      !effect_ui_p) {
    return PF_Err_BAD_CALLBACK_PARAM;
  }
  const auto* effect_ui =
      static_cast<const PF_EffectCustomUISuite1*>(effect_ui_p);

  PF_Err err = PF_Err_NONE;
  DRAWBOT_DrawRef drawing_ref = nullptr;
  err = (*effect_ui->PF_GetDrawingReference)(extra->contextH, &drawing_ref);
  if (err == PF_Err_NONE && drawing_ref) {
    const void* drawbot_p = nullptr;
    if (basic->AcquireSuite(kDRAWBOT_DrawSuite, kDRAWBOT_DrawSuite_VersionCurrent,
                            &drawbot_p) == kSPNoError &&
        drawbot_p) {
      const auto* drawbot =
          static_cast<const DRAWBOT_DrawbotSuiteCurrent*>(drawbot_p);
      DRAWBOT_SupplierRef supplier_ref = nullptr;
      DRAWBOT_SurfaceRef surface_ref = nullptr;
      (*drawbot->GetSupplier)(drawing_ref, &supplier_ref);
      (*drawbot->GetSurface)(drawing_ref, &surface_ref);
      const void* supplier_p = nullptr;
      // Acquire the Supplier and Surface suites in separate steps, each with a
      // matching release, so a failure to acquire the Surface suite does not
      // leak the already-acquired Supplier suite (a chained && would skip both
      // releases). The minihost worker always provides both, but the probe may
      // also run under real AE.
      if (supplier_ref && surface_ref &&
          basic->AcquireSuite(kDRAWBOT_SupplierSuite,
                              kDRAWBOT_SupplierSuite_VersionCurrent,
                              &supplier_p) == kSPNoError) {
        const void* surface_p = nullptr;
        if (basic->AcquireSuite(kDRAWBOT_SurfaceSuite,
                                kDRAWBOT_SurfaceSuite_VersionCurrent,
                                &surface_p) == kSPNoError) {
          const auto* supplier =
              static_cast<const DRAWBOT_SupplierSuiteCurrent*>(supplier_p);
          const auto* surface =
              static_cast<const DRAWBOT_SurfaceSuiteCurrent*>(surface_p);
          DRAWBOT_ColorRGBA color{0.9f, 0.2f, 0.6f, 1.0f};  // red, green, blue, alpha
          DRAWBOT_BrushRef brush = nullptr;
          if ((*supplier->NewBrush)(supplier_ref, &color, &brush) == kSPNoError &&
              brush) {
            DRAWBOT_RectF32 rect{2.0f, 2.0f, 8.0f, 8.0f};  // left, top, width, height
            (*surface->PaintRect)(surface_ref, &color, &rect);
            (*supplier->ReleaseObject)(
                reinterpret_cast<DRAWBOT_ObjectRef>(brush));
          }
          basic->ReleaseSuite(kDRAWBOT_SurfaceSuite,
                              kDRAWBOT_SurfaceSuite_VersionCurrent);
        }
        basic->ReleaseSuite(kDRAWBOT_SupplierSuite,
                            kDRAWBOT_SupplierSuite_VersionCurrent);
      }
      basic->ReleaseSuite(kDRAWBOT_DrawSuite, kDRAWBOT_DrawSuite_VersionCurrent);
    }
  }
  basic->ReleaseSuite(kPFEffectCustomUISuite, kPFEffectCustomUISuiteVersion1);
  extra->evt_out_flags |= PF_EO_HANDLED_EVENT;
  return err;
}

PF_Err HandleEvent(PF_InData* in_data, PF_OutData* /*out_data*/,
                   PF_ParamDef* params[], PF_LayerDef* /*output*/,
                   PF_EventExtra* extra) {
  if (!extra) return PF_Err_BAD_CALLBACK_PARAM;
  switch (extra->e_type) {
    case PF_Event_DO_CLICK:
      return DoClick(in_data, params, extra);
    case PF_Event_DRAW:
      return DrawEvent(in_data, extra);
    default:
      return PF_Err_NONE;
  }
}

template <typename Pixel>
void FillTyped(PF_LayerDef* output, const PF_Pixel& color, double maximum) {
  const auto scale = [maximum](A_u_char value) {
    return static_cast<decltype(Pixel::alpha)>(value / 255.0 * maximum);
  };
  Pixel pixel{};
  pixel.alpha = scale(color.alpha);
  pixel.red = scale(color.red);
  pixel.green = scale(color.green);
  pixel.blue = scale(color.blue);
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) row[x] = pixel;
  }
}

PF_Err Render(PF_InData* /*in_data*/, PF_ParamDef* params[], PF_LayerDef* output) {
  if (!params || !params[kParamColor] || !output || !output->data ||
      output->width <= 0 || output->height <= 0)
    return PF_Err_BAD_CALLBACK_PARAM;
  const PF_Pixel color = params[kParamColor]->u.cd.value;
  const A_long bytes_per_pixel = output->rowbytes / output->width;
  if (bytes_per_pixel >= static_cast<A_long>(sizeof(PF_PixelFloat)))
    FillTyped<PF_PixelFloat>(output, color, 1.0);
  else if (bytes_per_pixel >= static_cast<A_long>(sizeof(PF_Pixel16)))
    FillTyped<PF_Pixel16>(output, color, 32768.0);
  else
    FillTyped<PF_Pixel>(output, color, 255.0);
  return PF_Err_NONE;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data,
                                        PF_ParamDef* params[],
                                        PF_LayerDef* output, void* extra) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      return GlobalSetup(out_data);
    case PF_Cmd_PARAMS_SETUP:
      return ParamsSetup(in_data, out_data);
    case PF_Cmd_EVENT:
      return HandleEvent(in_data, out_data, params, output,
                         static_cast<PF_EventExtra*>(extra));
    case PF_Cmd_RENDER:
      return Render(in_data, params, output);
    default:
      return PF_Err_NONE;
  }
}
