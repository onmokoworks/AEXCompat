#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

#include <cstdint>

namespace {

bool RectEmpty(const PF_LRect& rect) {
  return rect.left >= rect.right || rect.top >= rect.bottom;
}

bool SameRect(const PF_LRect& rect, A_long left, A_long top, A_long right, A_long bottom) {
  return rect.left == left && rect.top == top && rect.right == right && rect.bottom == bottom;
}

PF_Err CheckoutInput(PF_InData* in_data, PF_PreRenderExtra* extra,
                     const PF_LRect& request_rect, PF_CheckoutResult* result) {
  PF_RenderRequest request = extra->input->output_request;
  request.rect = request_rect;
  return extra->cb->checkout_layer(in_data->effect_ref, 0, 0, &request,
                                   in_data->current_time, in_data->time_step,
                                   in_data->time_scale, result);
}

// Probe modes are selected through the generic render time the host already
// supplies (current_time modulo 4), so no host-side probe-specific branch is
// needed to reach any scenario.
enum class Mode : A_long {
  VerifyIntersection = 0,
  ExtraPixels = 1,
  FlaglessOverrun = 2,
  EmptyResult = 3,
};

Mode ProbeMode(const PF_InData* in_data) {
  return static_cast<Mode>(((in_data->current_time % 4) + 4) % 4);
}

PF_Err SmartPreRender(PF_InData* in_data, PF_PreRenderExtra* extra) {
  if (!in_data || !extra || !extra->input || !extra->output || !extra->cb)
    return PF_Err_BAD_CALLBACK_PARAM;
  const A_long width = in_data->width;
  const A_long height = in_data->height;
  if (width < 8 || height < 4) return PF_Err_BAD_CALLBACK_PARAM;
  const PF_LRect full{0, 0, width, height};
  PF_CheckoutResult checkout{};
  switch (ProbeMode(in_data)) {
    case Mode::VerifyIntersection: {
      // A full-frame request answers full availability.
      PF_Err err = CheckoutInput(in_data, extra, full, &checkout);
      if (err) return err;
      if (!SameRect(checkout.result_rect, 0, 0, width, height) ||
          !SameRect(checkout.max_result_rect, 0, 0, width, height))
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      // A sub-rect request is answered with exactly the intersection while
      // max_result_rect stays the full layer extent.
      const PF_LRect sub{2, 1, width - 2, height - 1};
      err = CheckoutInput(in_data, extra, sub, &checkout);
      if (err) return err;
      if (!SameRect(checkout.result_rect, sub.left, sub.top, sub.right, sub.bottom) ||
          !SameRect(checkout.max_result_rect, 0, 0, width, height))
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      // An oversized request clamps to the layer extent; max_result_rect must
      // not vary with the request.
      const PF_LRect oversized{-10, -10, width + 10, height + 10};
      err = CheckoutInput(in_data, extra, oversized, &checkout);
      if (err) return err;
      if (!SameRect(checkout.result_rect, 0, 0, width, height) ||
          !SameRect(checkout.max_result_rect, 0, 0, width, height))
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      // A disjoint request is answered with a legally empty rect while
      // max_result_rect still reports the full layer extent.
      const PF_LRect disjoint{width + 5, height + 5, width + 6, height + 6};
      err = CheckoutInput(in_data, extra, disjoint, &checkout);
      if (err) return err;
      if (!RectEmpty(checkout.result_rect) ||
          !SameRect(checkout.max_result_rect, 0, 0, width, height))
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      // The final full checkout is the one that backs the render.
      err = CheckoutInput(in_data, extra, full, &checkout);
      if (err) return err;
      extra->output->result_rect = full;
      extra->output->max_result_rect = full;
      return PF_Err_NONE;
    }
    case Mode::EmptyResult: {
      const PF_Err err = CheckoutInput(in_data, extra, full, &checkout);
      if (err) return err;
      extra->output->result_rect = PF_LRect{0, 0, 0, 0};
      extra->output->max_result_rect = full;
      return PF_Err_NONE;
    }
    case Mode::ExtraPixels:
    case Mode::FlaglessOverrun: {
      const PF_Err err = CheckoutInput(in_data, extra, full, &checkout);
      if (err) return err;
      const PF_LRect expanded{-2, -2, width + 2, height + 2};
      extra->output->result_rect = expanded;
      extra->output->max_result_rect = expanded;
      if (ProbeMode(in_data) == Mode::ExtraPixels)
        extra->output->flags = PF_RenderOutputFlag_RETURNS_EXTRA_PIXELS;
      return PF_Err_NONE;
    }
  }
  return PF_Err_BAD_CALLBACK_PARAM;
}

template <typename Pixel, typename Channel>
void FillWorld(PF_EffectWorld* world, Channel opaque) {
  for (A_long y = 0; y < world->height; ++y) {
    auto* row = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(world->data) + y * world->rowbytes);
    for (A_long x = 0; x < world->width; ++x) {
      auto* channels = reinterpret_cast<Channel*>(&row[x]);
      channels[0] = opaque;
      channels[1] = opaque;
      channels[2] = Channel(0);
      channels[3] = opaque;
    }
  }
}

PF_Err SmartRender(PF_InData* in_data, PF_SmartRenderExtra* extra) {
  if (!in_data || !extra || !extra->cb) return PF_Err_BAD_CALLBACK_PARAM;
  // The empty-result pre-render promised nothing, so the host must never
  // invoke the render selector for it.
  if (ProbeMode(in_data) == Mode::EmptyResult) return PF_Err_INTERNAL_STRUCT_DAMAGED;
  PF_EffectWorld* input = nullptr;
  PF_EffectWorld* output = nullptr;
  PF_Err err = extra->cb->checkout_layer_pixels(in_data->effect_ref, 0, &input);
  if (!err) err = extra->cb->checkout_output(in_data->effect_ref, &output);
  if (err) return err;
  if (!input || !input->data || !output || !output->data) return PF_Err_BAD_CALLBACK_PARAM;
  if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_PixelFloat)))
    FillWorld<PF_PixelFloat, PF_FpShort>(output, 1.0f);
  else if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_Pixel16)))
    FillWorld<PF_Pixel16, A_u_short>(output, PF_MAX_CHAN16);
  else
    FillWorld<PF_Pixel8, A_u_char>(output, PF_MAX_CHAN8);
  return extra->cb->checkin_layer_pixels(in_data->effect_ref, 0);
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef*, void* extra) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
      out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER |
                             PF_OutFlag2_FLOAT_COLOR_AWARE;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_SMART_PRE_RENDER:
      return SmartPreRender(in_data, static_cast<PF_PreRenderExtra*>(extra));
    case PF_Cmd_SMART_RENDER:
      return SmartRender(in_data, static_cast<PF_SmartRenderExtra*>(extra));
    default:
      return PF_Err_NONE;
  }
}
