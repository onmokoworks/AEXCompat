// Bisect instrument for issue #695.
//
// Adobe's Displacement (SmartFX, one map layer) returns PF_Err_OUT_OF_MEMORY
// from PF_Cmd_SMART_RENDER on this host while every host callback it makes
// succeeds, and while the SDK-shaped pf-smart-timed-multilayer-probe renders
// fine through the same session. The difference has to be in what Displacement
// additionally does, so this probe does those things one step at a time and
// reports the first step that fails.
//
// The "Stage" popup selects how far to go. Each stage is a superset of the one
// before it, so the first stage that stops rendering names the host behaviour
// that Displacement trips on:
//
//   1 baseline      checkout the map with the full request, then pixels, output
//   2 geometry      + probe the map first with an empty rect and a second id
//   3 params        + PF_CHECKOUT_PARAM in both PreRender and SmartRender
//   4 pixel format  + acquire PF World Suite and query the output world format
//   5 expand        + ask for the input expanded past the layer, then use the
//                     clipped result
//
// The probe never invents an error: every failure returns the host's own error
// code, and the stage it failed in is reported through out_data->return_msg so a
// diagnostic run can read it back.

#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectSuites.h"
#include "AE_EffectCB.h"
#include "AE_EffectCBSuites.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#include <array>
#include <cstdio>
#include <cstring>

namespace {

constexpr A_long kMapSlot = 1;
constexpr A_long kStageSlot = 2;
constexpr A_long kInputCheckoutId = 0;
constexpr A_long kMapCheckoutId = 1;
constexpr A_long kMapProbeCheckoutId = 1000;
// The expansion Displacement asks for at its default maximum displacement.
constexpr A_long kExpand = 5;

enum Stage {
  kStageBaseline = 1,
  kStageGeometryProbe = 2,
  kStageCheckoutParams = 3,
  kStagePixelFormat = 4,
  kStageExpandInput = 5,
};

A_long g_stage = kStageBaseline;
// The last step the probe entered, so a failing run says where it stopped
// rather than only that it stopped.
const char* g_step = "none";

PF_Err CheckoutParams(PF_InData* in_data) {
  // Displacement checks its scalar parameters out in both PreRender and
  // SmartRender rather than reading the params array.
  PF_ParamDef parameter{};
  const PF_Err err = PF_CHECKOUT_PARAM(in_data, kStageSlot, in_data->current_time,
                                       in_data->time_step, in_data->time_scale,
                                       &parameter);
  if (err) return err;
  return PF_CHECKIN_PARAM(in_data, &parameter);
}

PF_Err SmartPreRender(PF_InData* in_data, PF_PreRenderExtra* extra) {
  if (!in_data || !extra || !extra->input || !extra->output || !extra->cb)
    return PF_Err_BAD_CALLBACK_PARAM;
  g_step = "pre_render:begin";

  if (g_stage >= kStageCheckoutParams) {
    g_step = "pre_render:checkout_param";
    const PF_Err err = CheckoutParams(in_data);
    if (err) return err;
  }

  PF_RenderRequest input_request = extra->input->output_request;
  if (g_stage >= kStageExpandInput) {
    g_step = "pre_render:expand_input_request";
    input_request.rect.left -= kExpand;
    input_request.rect.top -= kExpand;
    input_request.rect.right += kExpand;
    input_request.rect.bottom += kExpand;
  }

  g_step = "pre_render:checkout_input";
  PF_CheckoutResult input_checkout{};
  PF_Err err = extra->cb->checkout_layer(in_data->effect_ref, 0, kInputCheckoutId,
                                         &input_request, in_data->current_time,
                                         in_data->time_step, in_data->time_scale,
                                         &input_checkout);
  if (err) return err;

  if (g_stage >= kStageGeometryProbe) {
    // Displacement asks the map for its geometry first, with an empty rect and
    // a checkout id it never redeems.
    g_step = "pre_render:probe_map_geometry";
    PF_RenderRequest empty = extra->input->output_request;
    empty.rect.left = 0;
    empty.rect.top = 0;
    empty.rect.right = 0;
    empty.rect.bottom = 0;
    PF_CheckoutResult probe{};
    err = extra->cb->checkout_layer(in_data->effect_ref, kMapSlot,
                                    kMapProbeCheckoutId, &empty,
                                    in_data->current_time, in_data->time_step,
                                    in_data->time_scale, &probe);
    if (err) return err;
  }

  g_step = "pre_render:checkout_map";
  PF_RenderRequest map_request = extra->input->output_request;
  PF_CheckoutResult map_checkout{};
  err = extra->cb->checkout_layer(in_data->effect_ref, kMapSlot, kMapCheckoutId,
                                  &map_request, in_data->current_time,
                                  in_data->time_step, in_data->time_scale,
                                  &map_checkout);
  if (err) return err;

  extra->output->result_rect = input_checkout.result_rect;
  extra->output->max_result_rect = input_checkout.max_result_rect;
  g_step = "pre_render:end";
  return PF_Err_NONE;
}

template <typename Pixel>
void CopyRows(const PF_EffectWorld* source, PF_EffectWorld* output) {
  const A_long height = source->height < output->height ? source->height : output->height;
  const A_long width = source->width < output->width ? source->width : output->width;
  for (A_long y = 0; y < height; ++y) {
    const auto* in_row = reinterpret_cast<const Pixel*>(
        reinterpret_cast<const A_u_char*>(source->data) + y * source->rowbytes);
    auto* out_row = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < width; ++x) out_row[x] = in_row[x];
  }
}

PF_Err SmartRender(PF_InData* in_data, PF_OutData* out_data,
                   PF_SmartRenderExtra* extra) {
  if (!in_data || !extra || !extra->cb || !extra->input)
    return PF_Err_BAD_CALLBACK_PARAM;
  g_step = "smart_render:begin";

  g_step = "smart_render:checkout_input_pixels";
  PF_EffectWorld* input = nullptr;
  PF_Err err = extra->cb->checkout_layer_pixels(in_data->effect_ref,
                                                kInputCheckoutId, &input);
  if (err) return err;

  g_step = "smart_render:checkout_output";
  PF_EffectWorld* output = nullptr;
  err = extra->cb->checkout_output(in_data->effect_ref, &output);
  if (err) return err;
  if (!output || !output->data) return PF_Err_BAD_CALLBACK_PARAM;

  PF_PixelFormat format = PF_PixelFormat_ARGB32;
  if (g_stage >= kStagePixelFormat) {
    g_step = "smart_render:get_pixel_format";
    // Acquired straight through PICA, the way the trace shows Displacement
    // acquiring "PF World Suite" v2 during SMART_RENDER.
    if (!in_data->pica_basicP) return PF_Err_BAD_CALLBACK_PARAM;
    const void* acquired = nullptr;
    if (in_data->pica_basicP->AcquireSuite(kPFWorldSuite, kPFWorldSuiteVersion2,
                                           &acquired) ||
        !acquired)
      return PF_Err_BAD_CALLBACK_PARAM;
    const auto* world_suite = static_cast<const PF_WorldSuite2*>(acquired);
    err = world_suite->PF_GetPixelFormat(output, &format);
    in_data->pica_basicP->ReleaseSuite(kPFWorldSuite, kPFWorldSuiteVersion2);
    if (err) return err;
  }

  g_step = "smart_render:checkout_map_pixels";
  PF_EffectWorld* map = nullptr;
  err = extra->cb->checkout_layer_pixels(in_data->effect_ref, kMapCheckoutId, &map);
  if (err) return err;
  if (!map || !map->data) return PF_Err_BAD_CALLBACK_PARAM;

  if (g_stage >= kStageCheckoutParams) {
    g_step = "smart_render:checkout_param";
    err = CheckoutParams(in_data);
    if (err) return err;
  }

  g_step = "smart_render:composite";
  if (!input || !input->data) return PF_Err_BAD_CALLBACK_PARAM;
  if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_PixelFloat)))
    CopyRows<PF_PixelFloat>(input, output);
  else if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_Pixel16)))
    CopyRows<PF_Pixel16>(input, output);
  else
    CopyRows<PF_Pixel8>(input, output);

  g_step = "smart_render:checkin";
  err = extra->cb->checkin_layer_pixels(in_data->effect_ref, kInputCheckoutId);
  if (!err) err = extra->cb->checkin_layer_pixels(in_data->effect_ref, kMapCheckoutId);
  if (err) return err;
  g_step = "smart_render:end";
  return PF_Err_NONE;
}

PF_Err ParamsSetup(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def{};
  AEFX_CLR_STRUCT(def);
  PF_ADD_LAYER("Map Layer", PF_LayerDefault_NONE, kMapSlot);
  AEFX_CLR_STRUCT(def);
  PF_ADD_POPUP("Stage", 5, kStageBaseline,
               "Baseline|Geometry Probe|Checkout Params|Pixel Format|Expand Input",
               kStageSlot);
  out_data->num_params = kStageSlot + 1;
  return PF_Err_NONE;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                       PF_OutData* out_data, PF_ParamDef* params[],
                                       PF_LayerDef*, void* extra) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
      out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER |
                             PF_OutFlag2_FLOAT_COLOR_AWARE;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      return ParamsSetup(in_data, out_data);
    case PF_Cmd_SMART_PRE_RENDER:
      if (params && params[kStageSlot]) g_stage = params[kStageSlot]->u.pd.value;
      return SmartPreRender(in_data, static_cast<PF_PreRenderExtra*>(extra));
    case PF_Cmd_SMART_RENDER: {
      const PF_Err err = SmartRender(in_data, out_data,
                                     static_cast<PF_SmartRenderExtra*>(extra));
      if (err && out_data) {
        // The failing step travels back with the error, so the diagnostic does
        // not have to guess which callback the probe stopped at.
        std::snprintf(out_data->return_msg, sizeof(out_data->return_msg),
                      "stage %ld stopped at %s", static_cast<long>(g_stage), g_step);
      }
      return err;
    }
    default:
      return PF_Err_NONE;
  }
}
